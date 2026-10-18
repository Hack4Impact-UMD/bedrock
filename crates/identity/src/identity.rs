use db::PgPool;
use uuid::Uuid;

use crate::{
    auth::{generate_api_key, hash_api_key},
    keys::{ApiKey, ApiKeyRepo, PostgresApiKeyRepo},
    projects::{PostgresProjectRepo, Project, ProjectRepo},
};

const FIRST_KEY_NAME: &str = "default";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProjectId(pub Uuid);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ApiKeyId(pub Uuid);

#[derive(Debug, thiserror::Error)]
pub enum IdentityError {
    #[error("API key is invalid")]
    InvalidKey,
    #[error("API key has been revoked")]
    Revoked,
    #[error("API key not found")]
    KeyNotFound,
    #[error("project not found")]
    ProjectNotFound,
    #[error("project slug is already taken")]
    SlugTaken,
    #[error("project slug must be 3-40 lowercase letters, digits or hyphens")]
    InvalidSlug,
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

/// Show only once. A newly created key. `plaintext` is only available here and is never stored.
pub struct NewApiKey {
    pub key: ApiKey,
    pub plaintext: String,
}

#[derive(Clone)]
pub struct Identity<K: ApiKeyRepo, P: ProjectRepo> {
    keys: K,
    projects: P,
}

impl<K: ApiKeyRepo, P: ProjectRepo> Identity<K, P> {
    pub fn new(keys: K, projects: P) -> Self {
        Self { keys, projects }
    }

    pub async fn authenticate_api_key(&self, key: &str) -> Result<ProjectId, IdentityError> {
        let (prefix, hash) = hash_api_key(key).ok_or(IdentityError::InvalidKey)?;
        let key = self
            .keys
            .lookup_key_from_hash_prefix(prefix, &hash)
            .await?
            .ok_or(IdentityError::InvalidKey)?;

        if key.revoked_at.is_some() {
            return Err(IdentityError::Revoked);
        }

        self.keys.mark_used(ApiKeyId(key.id)).await?;

        Ok(ProjectId(key.project_id))
    }

    pub async fn create_api_key(
        &self,
        project: ProjectId,
        name: &str,
    ) -> Result<NewApiKey, IdentityError> {
        let plaintext = generate_api_key();
        let (prefix, hash) = hash_api_key(&plaintext).expect("generated keys always have a prefix");
        let key = self.keys.create(prefix, &hash, name, project).await?;

        Ok(NewApiKey { key, plaintext })
    }

    pub async fn get_api_key(
        &self,
        project: ProjectId,
        id: ApiKeyId,
    ) -> Result<ApiKey, IdentityError> {
        self.keys
            .get(project, id)
            .await?
            .ok_or(IdentityError::KeyNotFound)
    }

    pub async fn list_api_keys(&self, project: ProjectId) -> Result<Vec<ApiKey>, IdentityError> {
        Ok(self.keys.list(project).await?)
    }

    pub async fn revoke_api_key(
        &self,
        project: ProjectId,
        id: ApiKeyId,
    ) -> Result<ApiKey, IdentityError> {
        self.keys
            .revoke(project, id)
            .await?
            .ok_or(IdentityError::KeyNotFound)
    }

    pub async fn delete_api_key(
        &self,
        project: ProjectId,
        id: ApiKeyId,
    ) -> Result<(), IdentityError> {
        if !self.keys.delete(project, id).await? {
            return Err(IdentityError::KeyNotFound);
        }

        Ok(())
    }

    /// Creates a project together with its first API key, so a project never exists without one.
    pub async fn create_project(
        &self,
        slug: &str,
        name: &str,
    ) -> Result<(Project, NewApiKey), IdentityError> {
        let plaintext = generate_api_key();
        let (prefix, hash) = hash_api_key(&plaintext).expect("generated keys always have a prefix");
        let (project, key) = self
            .projects
            .create_with_key(slug, name, prefix, &hash, FIRST_KEY_NAME)
            .await
            .map_err(slug_error)?;

        Ok((project, NewApiKey { key, plaintext }))
    }

    pub async fn get_project(&self, id: ProjectId) -> Result<Project, IdentityError> {
        self.projects
            .get(id)
            .await?
            .ok_or(IdentityError::ProjectNotFound)
    }

    pub async fn get_project_by_slug(&self, slug: &str) -> Result<Project, IdentityError> {
        self.projects
            .get_by_slug(slug)
            .await?
            .ok_or(IdentityError::ProjectNotFound)
    }

    pub async fn list_projects(&self) -> Result<Vec<Project>, IdentityError> {
        Ok(self.projects.list().await?)
    }

    pub async fn rename_project(
        &self,
        id: ProjectId,
        name: &str,
    ) -> Result<Project, IdentityError> {
        self.projects
            .rename(id, name)
            .await?
            .ok_or(IdentityError::ProjectNotFound)
    }

    /// Deletes the project's rows. External resources (such as SES identities) must be cleaned up first.
    pub async fn delete_project(&self, id: ProjectId) -> Result<(), IdentityError> {
        if !self.projects.delete(id).await? {
            return Err(IdentityError::ProjectNotFound);
        }

        Ok(())
    }
}

/// Maps violations of the slug constraints in the `identity.projects` table to their own errors.
fn slug_error(err: anyhow::Error) -> IdentityError {
    let constraint = err
        .downcast_ref::<sqlx::Error>()
        .and_then(|e| e.as_database_error())
        .and_then(|e| e.constraint());

    match constraint {
        Some("projects_slug_key") => IdentityError::SlugTaken,
        Some("projects_slug_check") => IdentityError::InvalidSlug,
        _ => err.into(),
    }
}

impl Identity<PostgresApiKeyRepo, PostgresProjectRepo> {
    pub fn postgres(pool: PgPool) -> Self {
        Self::new(
            PostgresApiKeyRepo::new(pool.clone()),
            PostgresProjectRepo::new(pool),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn create_project(pool: &PgPool) -> ProjectId {
        let id: Uuid = sqlx::query_scalar(
            "insert into identity.projects (slug, name) values ('demo', 'Demo') returning id",
        )
        .fetch_one(pool)
        .await
        .unwrap();

        ProjectId(id)
    }

    #[sqlx::test(migrations = "../db/migrations")]
    async fn authenticates_until_revoked(pool: PgPool) {
        let project = create_project(&pool).await;
        let identity = Identity::postgres(pool);
        let new_key = identity.create_api_key(project, "test").await.unwrap();
        let id = ApiKeyId(new_key.key.id);

        let authed = identity
            .authenticate_api_key(&new_key.plaintext)
            .await
            .unwrap();
        assert_eq!(authed, project);

        let revoked = identity.revoke_api_key(project, id).await.unwrap();
        assert!(revoked.revoked_at.is_some());
        assert!(matches!(
            identity.authenticate_api_key(&new_key.plaintext).await,
            Err(IdentityError::Revoked)
        ));
    }

    #[sqlx::test(migrations = "../db/migrations")]
    async fn gets_lists_and_deletes_keys(pool: PgPool) {
        let project = create_project(&pool).await;
        let identity = Identity::postgres(pool);
        let first = identity.create_api_key(project, "first").await.unwrap();
        identity.create_api_key(project, "second").await.unwrap();
        let id = ApiKeyId(first.key.id);

        assert_eq!(
            identity.get_api_key(project, id).await.unwrap().name,
            "first"
        );
        let names: Vec<_> = identity
            .list_api_keys(project)
            .await
            .unwrap()
            .into_iter()
            .map(|key| key.name)
            .collect();
        assert_eq!(names, ["first", "second"]);

        identity.delete_api_key(project, id).await.unwrap();
        assert!(matches!(
            identity.get_api_key(project, id).await,
            Err(IdentityError::KeyNotFound)
        ));
        assert!(matches!(
            identity.delete_api_key(project, id).await,
            Err(IdentityError::KeyNotFound)
        ));
    }

    #[sqlx::test(migrations = "../db/migrations")]
    async fn keys_are_scoped_to_their_project(pool: PgPool) {
        let project = create_project(&pool).await;
        let other: Uuid = sqlx::query_scalar(
            "insert into identity.projects (slug, name) values ('other', 'Other') returning id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let other = ProjectId(other);
        let identity = Identity::postgres(pool);
        let id = ApiKeyId(
            identity
                .create_api_key(project, "test")
                .await
                .unwrap()
                .key
                .id,
        );

        assert!(matches!(
            identity.get_api_key(other, id).await,
            Err(IdentityError::KeyNotFound)
        ));
        assert!(matches!(
            identity.revoke_api_key(other, id).await,
            Err(IdentityError::KeyNotFound)
        ));
        assert!(matches!(
            identity.delete_api_key(other, id).await,
            Err(IdentityError::KeyNotFound)
        ));
        assert!(identity.list_api_keys(other).await.unwrap().is_empty());
    }

    #[sqlx::test(migrations = "../db/migrations")]
    async fn rejects_unknown_and_malformed_keys(pool: PgPool) {
        let identity = Identity::postgres(pool);

        for key in ["bdrk_unknownkey1234567890", "bdrk_", "bdrk_1234567é"] {
            assert!(matches!(
                identity.authenticate_api_key(key).await,
                Err(IdentityError::InvalidKey)
            ));
        }
    }

    #[sqlx::test(migrations = "../db/migrations")]
    async fn create_project_issues_a_working_key(pool: PgPool) {
        let identity = Identity::postgres(pool);
        let (project, new_key) = identity.create_project("demo", "Demo").await.unwrap();

        assert_eq!(new_key.key.name, FIRST_KEY_NAME);
        let authed = identity
            .authenticate_api_key(&new_key.plaintext)
            .await
            .unwrap();
        assert_eq!(authed, ProjectId(project.id));
    }

    #[sqlx::test(migrations = "../db/migrations")]
    async fn create_project_maps_slug_errors(pool: PgPool) {
        let identity = Identity::postgres(pool);
        identity.create_project("demo", "Demo").await.unwrap();

        assert!(matches!(
            identity.create_project("demo", "Again").await,
            Err(IdentityError::SlugTaken)
        ));
        for slug in ["Has Caps", "ab"] {
            assert!(matches!(
                identity.create_project(slug, "Bad").await,
                Err(IdentityError::InvalidSlug)
            ));
        }
    }

    #[sqlx::test(migrations = "../db/migrations")]
    async fn project_operations(pool: PgPool) {
        let identity = Identity::postgres(pool);
        let (project, _) = identity.create_project("demo", "Demo").await.unwrap();
        let id = ProjectId(project.id);

        assert_eq!(identity.get_project(id).await.unwrap().slug, "demo");
        assert_eq!(
            identity.get_project_by_slug("demo").await.unwrap().id,
            project.id
        );
        assert_eq!(identity.list_projects().await.unwrap().len(), 1);
        assert_eq!(
            identity.rename_project(id, "Renamed").await.unwrap().name,
            "Renamed"
        );

        identity.delete_project(id).await.unwrap();
        for result in [
            identity.get_project(id).await.map(|_| ()),
            identity.get_project_by_slug("demo").await.map(|_| ()),
            identity.rename_project(id, "Gone").await.map(|_| ()),
            identity.delete_project(id).await,
        ] {
            assert!(matches!(result, Err(IdentityError::ProjectNotFound)));
        }
    }
}

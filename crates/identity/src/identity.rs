use db::PgPool;
use uuid::Uuid;

use crate::{
    auth::{generate_api_key, hash_api_key},
    keys::{ApiKey, ApiKeyRepo, PostgresApiKeyRepo},
};

pub struct ProjectId(pub(crate) Uuid);
pub struct ApiKeyId(pub(crate) Uuid);

#[derive(Debug, thiserror::Error)]
pub enum IdentityError {
    #[error("API key is invalid")]
    InvalidKey,
    #[error("API key has been revoked")]
    Revoked,
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

/// A newly created key. `plaintext` is only available here and is never stored.
pub struct NewApiKey {
    pub key: ApiKey,
    pub plaintext: String,
}

#[derive(Clone)]
pub struct Identity<T: ApiKeyRepo> {
    db: T,
}

impl<T: ApiKeyRepo> Identity<T> {
    pub fn new(repo: T) -> Self {
        Self { db: repo }
    }

    pub async fn authenticate_api_key(&self, key: &str) -> Result<ProjectId, IdentityError> {
        let (prefix, hash) = hash_api_key(key).ok_or(IdentityError::InvalidKey)?;
        let key = self
            .db
            .lookup_key_from_hash_prefix(prefix, &hash)
            .await?
            .ok_or(IdentityError::InvalidKey)?;

        if key.revoked_at.is_some() {
            return Err(IdentityError::Revoked);
        }

        Ok(ProjectId(key.project_id))
    }

    pub async fn create_api_key(
        &self,
        project: ProjectId,
        name: &str,
    ) -> Result<NewApiKey, IdentityError> {
        let plaintext = generate_api_key();
        let (prefix, hash) = hash_api_key(&plaintext).expect("generated keys always have a prefix");
        let key = self.db.create(prefix, &hash, name, project).await?;

        Ok(NewApiKey { key, plaintext })
    }
}

impl Identity<PostgresApiKeyRepo> {
    pub fn postgres(pool: PgPool) -> Self {
        Self::new(PostgresApiKeyRepo::new(pool))
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
        let repo = PostgresApiKeyRepo::new(pool);
        let identity = Identity::new(repo.clone());
        let new_key = identity
            .create_api_key(ProjectId(project.0), "test")
            .await
            .unwrap();

        let authed = identity
            .authenticate_api_key(&new_key.plaintext)
            .await
            .unwrap();
        assert_eq!(authed.0, project.0);

        repo.revoke(project, ApiKeyId(new_key.key.id))
            .await
            .unwrap();
        assert!(matches!(
            identity.authenticate_api_key(&new_key.plaintext).await,
            Err(IdentityError::Revoked)
        ));
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
}

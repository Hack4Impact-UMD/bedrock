use anyhow::Result;
use db::PgPool;
use serde::Serialize;
use sqlx::PgExecutor;
use sqlx::prelude::FromRow;
use sqlx::types::chrono::DateTime;
use sqlx::types::chrono::Utc;
use uuid::Uuid;

use crate::identity::ProjectId;
use crate::keys::{ApiKey, insert_key};

#[derive(FromRow, Debug, Serialize)]
pub struct Project {
    pub id: Uuid,
    pub slug: String,
    pub name: String,
    pub created_at: DateTime<Utc>,
}

pub trait ProjectRepo {
    fn create(&self, slug: &str, name: &str) -> impl Future<Output = Result<Project>> + Send;
    /// Creates a project and its first API key in one transaction.
    fn create_with_key(
        &self,
        slug: &str,
        name: &str,
        key_prefix: &str,
        key_hash: &[u8],
        key_name: &str,
    ) -> impl Future<Output = Result<(Project, ApiKey)>> + Send;
    fn get(&self, id: ProjectId) -> impl Future<Output = Result<Option<Project>>> + Send;
    fn get_by_slug(&self, slug: &str) -> impl Future<Output = Result<Option<Project>>> + Send;
    fn list(&self) -> impl Future<Output = Result<Vec<Project>>> + Send;
    fn rename(
        &self,
        id: ProjectId,
        name: &str,
    ) -> impl Future<Output = Result<Option<Project>>> + Send;
    /// Returns whether a project was deleted. Its keys and courier rows are removed by cascade.
    fn delete(&self, id: ProjectId) -> impl Future<Output = Result<bool>> + Send;
}

async fn insert_project(db: impl PgExecutor<'_>, slug: &str, name: &str) -> Result<Project> {
    let project: Project = sqlx::query_as(
        r"
    insert into identity.projects (slug, name)
    values ($1, $2)
    returning *
    ",
    )
    .bind(slug)
    .bind(name)
    .fetch_one(db)
    .await?;

    Ok(project)
}

#[derive(Clone)]
pub struct PostgresProjectRepo {
    db: PgPool,
}

impl PostgresProjectRepo {
    pub fn new(db: PgPool) -> Self {
        Self { db }
    }
}

impl ProjectRepo for PostgresProjectRepo {
    async fn create(&self, slug: &str, name: &str) -> Result<Project> {
        insert_project(&self.db, slug, name).await
    }

    async fn create_with_key(
        &self,
        slug: &str,
        name: &str,
        key_prefix: &str,
        key_hash: &[u8],
        key_name: &str,
    ) -> Result<(Project, ApiKey)> {
        let mut tx = self.db.begin().await?;
        let project = insert_project(&mut *tx, slug, name).await?;
        let key = insert_key(
            &mut *tx,
            key_prefix,
            key_hash,
            key_name,
            ProjectId(project.id),
        )
        .await?;
        tx.commit().await?;

        Ok((project, key))
    }

    async fn get(&self, id: ProjectId) -> Result<Option<Project>> {
        let project: Option<Project> = sqlx::query_as(
            r"
        select * from identity.projects
        where id = $1
        ",
        )
        .bind(id.0)
        .fetch_optional(&self.db)
        .await?;

        Ok(project)
    }

    async fn get_by_slug(&self, slug: &str) -> Result<Option<Project>> {
        let project: Option<Project> = sqlx::query_as(
            r"
        select * from identity.projects
        where slug = $1
        ",
        )
        .bind(slug)
        .fetch_optional(&self.db)
        .await?;

        Ok(project)
    }

    async fn list(&self) -> Result<Vec<Project>> {
        let projects: Vec<Project> = sqlx::query_as(
            r"
        select * from identity.projects
        order by created_at
        ",
        )
        .fetch_all(&self.db)
        .await?;

        Ok(projects)
    }

    async fn rename(&self, id: ProjectId, name: &str) -> Result<Option<Project>> {
        let project: Option<Project> = sqlx::query_as(
            r"
        update identity.projects
        set name = $2
        where id = $1
        returning *
        ",
        )
        .bind(id.0)
        .bind(name)
        .fetch_optional(&self.db)
        .await?;

        Ok(project)
    }

    async fn delete(&self, id: ProjectId) -> Result<bool> {
        let result = sqlx::query(
            r"
        delete from identity.projects
        where id = $1
        ",
        )
        .bind(id.0)
        .execute(&self.db)
        .await?;

        Ok(result.rows_affected() > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::{ApiKeyRepo, PostgresApiKeyRepo};

    #[sqlx::test(migrations = "../db/migrations")]
    async fn crud(pool: PgPool) {
        let repo = PostgresProjectRepo::new(pool);
        let created = repo.create("demo", "Demo").await.unwrap();
        let id = ProjectId(created.id);

        assert_eq!(repo.get(id).await.unwrap().unwrap().slug, "demo");
        assert_eq!(
            repo.get_by_slug("demo").await.unwrap().unwrap().id,
            created.id
        );
        assert_eq!(repo.list().await.unwrap().len(), 1);

        let renamed = repo.rename(id, "Renamed").await.unwrap().unwrap();
        assert_eq!(renamed.name, "Renamed");

        assert!(repo.delete(id).await.unwrap());
        assert!(repo.get(id).await.unwrap().is_none());
        assert!(!repo.delete(id).await.unwrap());
        assert!(repo.rename(id, "Gone").await.unwrap().is_none());
    }

    #[sqlx::test(migrations = "../db/migrations")]
    async fn rejects_duplicate_and_invalid_slugs(pool: PgPool) {
        let repo = PostgresProjectRepo::new(pool);
        repo.create("demo", "Demo").await.unwrap();

        for slug in ["demo", "Has Caps", "ab"] {
            assert!(repo.create(slug, "Bad").await.is_err());
        }
    }

    #[sqlx::test(migrations = "../db/migrations")]
    async fn create_with_key_is_atomic(pool: PgPool) {
        let repo = PostgresProjectRepo::new(pool.clone());
        let keys = PostgresApiKeyRepo::new(pool);
        let (project, key) = repo
            .create_with_key("demo", "Demo", "bdrk_12345678", b"hash", "default")
            .await
            .unwrap();
        assert_eq!(key.project_id, project.id);
        assert_eq!(keys.list(ProjectId(project.id)).await.unwrap().len(), 1);

        // The key insert fails on the duplicate prefix, so the project insert is rolled back.
        assert!(
            repo.create_with_key("other", "Other", "bdrk_12345678", b"hash", "default")
                .await
                .is_err()
        );
        assert!(repo.get_by_slug("other").await.unwrap().is_none());
    }

    #[sqlx::test(migrations = "../db/migrations")]
    async fn delete_cascades_to_keys(pool: PgPool) {
        let projects = PostgresProjectRepo::new(pool.clone());
        let keys = PostgresApiKeyRepo::new(pool);
        let id = ProjectId(projects.create("demo", "Demo").await.unwrap().id);
        keys.create("bdrk_12345678", b"hash", "test", id)
            .await
            .unwrap();

        projects.delete(id).await.unwrap();
        assert!(keys.list(id).await.unwrap().is_empty());
    }
}

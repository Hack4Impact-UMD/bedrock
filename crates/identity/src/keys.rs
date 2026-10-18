use anyhow::Result;
use db::PgPool;
use sqlx::PgExecutor;
use sqlx::prelude::FromRow;
use sqlx::types::chrono::DateTime;
use sqlx::types::chrono::Utc;
use uuid::Uuid;

use crate::identity::{ApiKeyId, ProjectId};

#[derive(FromRow, Debug)]
pub struct ApiKey {
    pub id: Uuid,
    pub project_id: Uuid,
    pub name: String,
    pub prefix: String,
    pub hash: Vec<u8>,
    pub created_at: DateTime<Utc>,
    pub last_used_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
}

pub trait ApiKeyRepo {
    fn lookup_key_from_hash_prefix(
        &self,
        prefix: &str,
        hash: &[u8],
    ) -> impl Future<Output = Result<Option<ApiKey>>> + Send;
    fn create(
        &self,
        prefix: &str,
        hash: &[u8],
        name: &str,
        project: ProjectId,
    ) -> impl Future<Output = Result<ApiKey>> + Send;
    fn get(
        &self,
        project: ProjectId,
        id: ApiKeyId,
    ) -> impl Future<Output = Result<Option<ApiKey>>> + Send;
    fn list(&self, project: ProjectId) -> impl Future<Output = Result<Vec<ApiKey>>> + Send;
    fn mark_used(&self, id: ApiKeyId) -> impl Future<Output = Result<()>> + Send;
    fn revoke(
        &self,
        project: ProjectId,
        id: ApiKeyId,
    ) -> impl Future<Output = Result<Option<ApiKey>>> + Send;
    /// Returns whether a key was deleted.
    fn delete(&self, project: ProjectId, id: ApiKeyId)
    -> impl Future<Output = Result<bool>> + Send;
}

// Takes any executor so it can also run inside another repo's transaction.
pub(crate) async fn insert_key(
    db: impl PgExecutor<'_>,
    prefix: &str,
    hash: &[u8],
    name: &str,
    project: ProjectId,
) -> Result<ApiKey> {
    let key: ApiKey = sqlx::query_as(
        r"
    insert into identity.api_keys (project_id, name, prefix, hash)
    values ($1, $2, $3, $4)
    returning *
    ",
    )
    .bind(project.0)
    .bind(name)
    .bind(prefix)
    .bind(hash)
    .persistent(false)
    .fetch_one(db)
    .await?;

    Ok(key)
}

#[derive(Clone)]
pub struct PostgresApiKeyRepo {
    db: PgPool,
}

impl PostgresApiKeyRepo {
    pub fn new(db: PgPool) -> Self {
        Self { db }
    }
}

impl ApiKeyRepo for PostgresApiKeyRepo {
    async fn lookup_key_from_hash_prefix(
        &self,
        prefix: &str,
        hash: &[u8],
    ) -> Result<Option<ApiKey>> {
        let key: Option<ApiKey> = sqlx::query_as(
            r"
        select * from identity.api_keys 
        where hash = $1 and prefix = $2
        ",
        )
        .bind(hash)
        .bind(prefix)
        .persistent(false)
        .fetch_optional(&self.db)
        .await?;

        Ok(key)
    }

    async fn create(
        &self,
        prefix: &str,
        hash: &[u8],
        name: &str,
        project: ProjectId,
    ) -> Result<ApiKey> {
        insert_key(&self.db, prefix, hash, name, project).await
    }

    async fn get(&self, project: ProjectId, id: ApiKeyId) -> Result<Option<ApiKey>> {
        let key: Option<ApiKey> = sqlx::query_as(
            r"
        select * from identity.api_keys
        where project_id = $1 and id = $2
        ",
        )
        .bind(project.0)
        .bind(id.0)
        .persistent(false)
        .fetch_optional(&self.db)
        .await?;

        Ok(key)
    }

    async fn list(&self, project: ProjectId) -> Result<Vec<ApiKey>> {
        let keys: Vec<ApiKey> = sqlx::query_as(
            r"
        select * from identity.api_keys
        where project_id = $1
        order by created_at
        ",
        )
        .bind(project.0)
        .persistent(false)
        .fetch_all(&self.db)
        .await?;

        Ok(keys)
    }

    async fn mark_used(&self, id: ApiKeyId) -> Result<()> {
        sqlx::query(
            r"
        update identity.api_keys
        set last_used_at = now()
        where id = $1
        ",
        )
        .bind(id.0)
        .persistent(false)
        .execute(&self.db)
        .await?;

        Ok(())
    }

    // Revoking an already revoked key keeps its original revocation time.
    async fn revoke(&self, project: ProjectId, id: ApiKeyId) -> Result<Option<ApiKey>> {
        let key: Option<ApiKey> = sqlx::query_as(
            r"
        update identity.api_keys
        set revoked_at = coalesce(revoked_at, now())
        where project_id = $1 and id = $2
        returning *
        ",
        )
        .bind(project.0)
        .bind(id.0)
        .persistent(false)
        .fetch_optional(&self.db)
        .await?;

        Ok(key)
    }

    async fn delete(&self, project: ProjectId, id: ApiKeyId) -> Result<bool> {
        let result = sqlx::query(
            r"
        delete from identity.api_keys
        where project_id = $1 and id = $2
        ",
        )
        .bind(project.0)
        .bind(id.0)
        .persistent(false)
        .execute(&self.db)
        .await?;

        Ok(result.rows_affected() > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::projects::{PostgresProjectRepo, ProjectRepo};

    async fn create_project(pool: &PgPool, slug: &str) -> ProjectId {
        let project = PostgresProjectRepo::new(pool.clone())
            .create(slug, "Demo")
            .await
            .unwrap();

        ProjectId(project.id)
    }

    #[sqlx::test(migrations = "../db/migrations")]
    async fn crud(pool: PgPool) {
        let project = create_project(&pool, "demo").await;
        let repo = PostgresApiKeyRepo::new(pool);
        let created = repo
            .create("bdrk_12345678", b"hash", "test", project)
            .await
            .unwrap();
        let id = ApiKeyId(created.id);

        assert_eq!(repo.get(project, id).await.unwrap().unwrap().name, "test");
        assert_eq!(repo.list(project).await.unwrap().len(), 1);
        assert!(created.last_used_at.is_none());

        repo.mark_used(id).await.unwrap();
        let used = repo.get(project, id).await.unwrap().unwrap();
        assert!(used.last_used_at.is_some());

        assert!(repo.delete(project, id).await.unwrap());
        assert!(repo.get(project, id).await.unwrap().is_none());
        assert!(!repo.delete(project, id).await.unwrap());
        assert!(repo.revoke(project, id).await.unwrap().is_none());
    }

    #[sqlx::test(migrations = "../db/migrations")]
    async fn lookup_requires_matching_prefix_and_hash(pool: PgPool) {
        let project = create_project(&pool, "demo").await;
        let repo = PostgresApiKeyRepo::new(pool);
        let created = repo
            .create("bdrk_12345678", b"hash", "test", project)
            .await
            .unwrap();

        let found = repo
            .lookup_key_from_hash_prefix("bdrk_12345678", b"hash")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(found.id, created.id);

        for (prefix, hash) in [
            ("bdrk_12345678", &b"other"[..]),
            ("bdrk_87654321", &b"hash"[..]),
        ] {
            assert!(
                repo.lookup_key_from_hash_prefix(prefix, hash)
                    .await
                    .unwrap()
                    .is_none()
            );
        }
    }

    #[sqlx::test(migrations = "../db/migrations")]
    async fn revoke_keeps_original_time(pool: PgPool) {
        let project = create_project(&pool, "demo").await;
        let repo = PostgresApiKeyRepo::new(pool);
        let id = ApiKeyId(
            repo.create("bdrk_12345678", b"hash", "test", project)
                .await
                .unwrap()
                .id,
        );

        let first = repo.revoke(project, id).await.unwrap().unwrap();
        let second = repo.revoke(project, id).await.unwrap().unwrap();
        assert!(first.revoked_at.is_some());
        assert_eq!(first.revoked_at, second.revoked_at);
    }

    #[sqlx::test(migrations = "../db/migrations")]
    async fn keys_are_scoped_to_their_project(pool: PgPool) {
        let project = create_project(&pool, "demo").await;
        let other = create_project(&pool, "other").await;
        let repo = PostgresApiKeyRepo::new(pool);
        let id = ApiKeyId(
            repo.create("bdrk_12345678", b"hash", "test", project)
                .await
                .unwrap()
                .id,
        );

        assert!(repo.get(other, id).await.unwrap().is_none());
        assert!(repo.list(other).await.unwrap().is_empty());
        assert!(repo.revoke(other, id).await.unwrap().is_none());
        assert!(!repo.delete(other, id).await.unwrap());
        assert!(repo.get(project, id).await.unwrap().is_some());
    }

    #[sqlx::test(migrations = "../db/migrations")]
    async fn rejects_duplicate_prefix(pool: PgPool) {
        let project = create_project(&pool, "demo").await;
        let repo = PostgresApiKeyRepo::new(pool);
        repo.create("bdrk_12345678", b"hash", "first", project)
            .await
            .unwrap();

        assert!(
            repo.create("bdrk_12345678", b"other", "second", project)
                .await
                .is_err()
        );
    }
}

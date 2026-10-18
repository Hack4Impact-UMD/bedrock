use anyhow::Result;
use db::PgPool;
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
    pub hash: String,
    pub created_at: DateTime<Utc>,
    pub last_used_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
}

pub trait ApiKeyRepo {
    async fn lookup_key_from_hash_prefix(&self, prefix: &str, hash: &str) -> Result<ApiKey>;
    async fn create(
        &self,
        prefix: &str,
        hash: &str,
        name: &str,
        project: ProjectId,
    ) -> Result<ApiKey>;
    async fn get(&self, project: ProjectId, id: ApiKeyId) -> Result<ApiKey>;
    async fn list(&self, project: ProjectId) -> Result<Vec<ApiKey>>;
    async fn mark_used(&self, id: ApiKeyId) -> Result<()>;
    async fn revoke(&self, project: ProjectId, id: ApiKeyId) -> Result<ApiKey>;
    async fn delete(&self, project: ProjectId, id: ApiKeyId) -> Result<()>;
}

pub struct PostgresApiKeyRepo<'a> {
    db: &'a PgPool,
}

impl<'a> PostgresApiKeyRepo<'a> {
    pub fn new(db: &'a PgPool) -> Self {
        Self { db }
    }
}

impl<'a> ApiKeyRepo for PostgresApiKeyRepo<'a> {
    async fn lookup_key_from_hash_prefix(&self, prefix: &str, hash: &str) -> Result<ApiKey> {
        let key: ApiKey = sqlx::query_as(
            r"
        select * from identity.api_keys 
        where hash = $1 and prefix = $2
        ",
        )
        .bind(hash)
        .bind(prefix)
        .persistent(false)
        .fetch_one(self.db)
        .await?;

        Ok(key)
    }

    async fn create(
        &self,
        prefix: &str,
        hash: &str,
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
        .fetch_one(self.db)
        .await?;

        Ok(key)
    }

    async fn get(&self, project: ProjectId, id: ApiKeyId) -> Result<ApiKey> {
        let key: ApiKey = sqlx::query_as(
            r"
        select * from identity.api_keys
        where project_id = $1 and id = $2
        ",
        )
        .bind(project.0)
        .bind(id.0)
        .persistent(false)
        .fetch_one(self.db)
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
        .fetch_all(self.db)
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
        .execute(self.db)
        .await?;

        Ok(())
    }

    // Revoking an already revoked key keeps its original revocation time.
    async fn revoke(&self, project: ProjectId, id: ApiKeyId) -> Result<ApiKey> {
        let key: ApiKey = sqlx::query_as(
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
        .fetch_one(self.db)
        .await?;

        Ok(key)
    }

    async fn delete(&self, project: ProjectId, id: ApiKeyId) -> Result<()> {
        let result = sqlx::query(
            r"
        delete from identity.api_keys
        where project_id = $1 and id = $2
        ",
        )
        .bind(project.0)
        .bind(id.0)
        .persistent(false)
        .execute(self.db)
        .await?;

        if result.rows_affected() == 0 {
            return Err(sqlx::Error::RowNotFound.into());
        }

        Ok(())
    }
}

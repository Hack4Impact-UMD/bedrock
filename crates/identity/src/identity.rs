use anyhow::Result;
use uuid::Uuid;

use crate::{auth::hash_api_key, keys::ApiKeyRepo};

pub struct ProjectId(pub(crate) Uuid);
pub struct ApiKeyId(pub(crate) Uuid);

pub struct Identity<'a, T: ApiKeyRepo> {
    db: &'a T,
}

impl<'a, T: ApiKeyRepo> Identity<'a, T> {
    pub async fn authenticate_api_key(&self, key: &str) -> Result<ProjectId> {
        let (prefix, hash) = hash_api_key(key)?;
        let key = self.db.lookup_key_from_hash_prefix(&prefix, &hash).await?;

        Ok(ProjectId(key.project_id))
    }
}

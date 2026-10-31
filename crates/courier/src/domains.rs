use anyhow::Result;
use db::PgPool;
use identity::ProjectId;
use serde::{Deserialize, Serialize};
use sqlx::prelude::FromRow;
use sqlx::types::Json;
use sqlx::types::chrono::DateTime;
use sqlx::types::chrono::Utc;
use uuid::Uuid;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DomainId(pub Uuid);

#[derive(sqlx::Type, Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[sqlx(type_name = "courier.domain_status", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum DomainStatus {
    Pending,
    Verified,
    Failed,
    TemporaryFailure,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DnsRecord {
    #[serde(rename = "type")]
    pub record_type: String,
    pub name: String,
    pub value: String,
}

#[derive(FromRow, Debug, Serialize)]
pub struct Domain {
    pub id: Uuid,
    pub project_id: Uuid,
    pub domain: String,
    pub status: DomainStatus,
    pub dns_records: Json<Vec<DnsRecord>>,
    pub created_at: DateTime<Utc>,
    pub last_checked_at: Option<DateTime<Utc>>,
    pub verified_at: Option<DateTime<Utc>>,
}

pub trait DomainRepo {
    fn create(
        &self,
        project: ProjectId,
        domain: &str,
        dns_records: &[DnsRecord],
    ) -> impl Future<Output = Result<Domain>> + Send;
    fn get(
        &self,
        project: ProjectId,
        id: DomainId,
    ) -> impl Future<Output = Result<Option<Domain>>> + Send;
    /// Case-insensitive lookup, matching the uniqueness index.
    fn get_by_name(
        &self,
        project: ProjectId,
        domain: &str,
    ) -> impl Future<Output = Result<Option<Domain>>> + Send;
    fn list(&self, project: ProjectId) -> impl Future<Output = Result<Vec<Domain>>> + Send;
    /// Records the result of a verification check.
    fn record_check(
        &self,
        id: DomainId,
        status: DomainStatus,
    ) -> impl Future<Output = Result<Option<Domain>>> + Send;
    /// Returns whether a domain was deleted.
    fn delete(&self, project: ProjectId, id: DomainId)
    -> impl Future<Output = Result<bool>> + Send;
}

#[derive(Clone)]
pub struct PostgresDomainRepo {
    db: PgPool,
}

impl PostgresDomainRepo {
    pub fn new(db: PgPool) -> Self {
        Self { db }
    }
}

impl DomainRepo for PostgresDomainRepo {
    async fn create(
        &self,
        project: ProjectId,
        domain: &str,
        dns_records: &[DnsRecord],
    ) -> Result<Domain> {
        let domain: Domain = sqlx::query_as(
            r"
        insert into courier.domains (project_id, domain, dns_records)
        values ($1, $2, $3)
        returning *
        ",
        )
        .bind(project.0)
        .bind(domain)
        .bind(Json(dns_records))
        .fetch_one(&self.db)
        .await?;

        Ok(domain)
    }

    async fn get(&self, project: ProjectId, id: DomainId) -> Result<Option<Domain>> {
        let domain: Option<Domain> = sqlx::query_as(
            r"
        select * from courier.domains
        where project_id = $1 and id = $2
        ",
        )
        .bind(project.0)
        .bind(id.0)
        .fetch_optional(&self.db)
        .await?;

        Ok(domain)
    }

    async fn get_by_name(&self, project: ProjectId, domain: &str) -> Result<Option<Domain>> {
        let domain: Option<Domain> = sqlx::query_as(
            r"
        select * from courier.domains
        where project_id = $1 and lower(domain) = lower($2)
        ",
        )
        .bind(project.0)
        .bind(domain)
        .fetch_optional(&self.db)
        .await?;

        Ok(domain)
    }

    async fn list(&self, project: ProjectId) -> Result<Vec<Domain>> {
        let domains: Vec<Domain> = sqlx::query_as(
            r"
        select * from courier.domains
        where project_id = $1
        order by created_at
        ",
        )
        .bind(project.0)
        .fetch_all(&self.db)
        .await?;

        Ok(domains)
    }

    async fn record_check(&self, id: DomainId, status: DomainStatus) -> Result<Option<Domain>> {
        let domain: Option<Domain> = sqlx::query_as(
            r"
        update courier.domains
        set status = $2,
            last_checked_at = now(),
            verified_at = case when $2 = 'verified' then coalesce(verified_at, now()) else verified_at end
        where id = $1
        returning *
        ",
        )
        .bind(id.0)
        .bind(status)
        .fetch_optional(&self.db)
        .await?;

        Ok(domain)
    }

    async fn delete(&self, project: ProjectId, id: DomainId) -> Result<bool> {
        let result = sqlx::query(
            r"
        delete from courier.domains
        where project_id = $1 and id = $2
        ",
        )
        .bind(project.0)
        .bind(id.0)
        .execute(&self.db)
        .await?;

        Ok(result.rows_affected() > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn create_project(pool: &PgPool, slug: &str) -> ProjectId {
        let (id,): (Uuid,) = sqlx::query_as(
            "insert into identity.projects (slug, name) values ($1, 'Demo') returning id",
        )
        .bind(slug)
        .fetch_one(pool)
        .await
        .unwrap();

        ProjectId(id)
    }

    fn records() -> Vec<DnsRecord> {
        vec![DnsRecord {
            record_type: "CNAME".into(),
            name: "abc._domainkey.example.com".into(),
            value: "abc.dkim.amazonses.com".into(),
        }]
    }

    #[sqlx::test(migrations = "../db/migrations")]
    async fn crud(pool: PgPool) {
        let project = create_project(&pool, "demo").await;
        let repo = PostgresDomainRepo::new(pool);
        let created = repo
            .create(project, "example.com", &records())
            .await
            .unwrap();
        let id = DomainId(created.id);

        assert_eq!(created.status, DomainStatus::Pending);
        assert_eq!(created.dns_records.0, records());
        assert!(created.last_checked_at.is_none());
        assert!(created.verified_at.is_none());

        let fetched = repo.get(project, id).await.unwrap().unwrap();
        assert_eq!(fetched.domain, "example.com");
        assert_eq!(fetched.dns_records.0, records());
        assert_eq!(repo.list(project).await.unwrap().len(), 1);

        assert!(repo.delete(project, id).await.unwrap());
        assert!(repo.get(project, id).await.unwrap().is_none());
        assert!(!repo.delete(project, id).await.unwrap());
        assert!(
            repo.record_check(id, DomainStatus::Verified)
                .await
                .unwrap()
                .is_none()
        );
    }

    #[sqlx::test(migrations = "../db/migrations")]
    async fn get_by_name_is_case_insensitive(pool: PgPool) {
        let project = create_project(&pool, "demo").await;
        let repo = PostgresDomainRepo::new(pool);
        let created = repo
            .create(project, "Example.com", &records())
            .await
            .unwrap();

        let found = repo
            .get_by_name(project, "example.COM")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(found.id, created.id);
        assert!(
            repo.get_by_name(project, "other.com")
                .await
                .unwrap()
                .is_none()
        );
    }

    #[sqlx::test(migrations = "../db/migrations")]
    async fn record_check_tracks_verification(pool: PgPool) {
        let project = create_project(&pool, "demo").await;
        let repo = PostgresDomainRepo::new(pool);
        let id = DomainId(
            repo.create(project, "example.com", &records())
                .await
                .unwrap()
                .id,
        );

        let pending = repo
            .record_check(id, DomainStatus::Pending)
            .await
            .unwrap()
            .unwrap();
        assert!(pending.last_checked_at.is_some());
        assert!(pending.verified_at.is_none());

        let verified = repo
            .record_check(id, DomainStatus::Verified)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(verified.status, DomainStatus::Verified);
        assert!(verified.verified_at.is_some());

        let failed = repo
            .record_check(id, DomainStatus::Failed)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(failed.status, DomainStatus::Failed);
        assert_eq!(failed.verified_at, verified.verified_at);

        let reverified = repo
            .record_check(id, DomainStatus::Verified)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(reverified.verified_at, verified.verified_at);
    }

    #[sqlx::test(migrations = "../db/migrations")]
    async fn domains_are_scoped_to_their_project(pool: PgPool) {
        let project = create_project(&pool, "demo").await;
        let other = create_project(&pool, "other").await;
        let repo = PostgresDomainRepo::new(pool);
        let id = DomainId(
            repo.create(project, "example.com", &records())
                .await
                .unwrap()
                .id,
        );

        assert!(repo.get(other, id).await.unwrap().is_none());
        assert!(
            repo.get_by_name(other, "example.com")
                .await
                .unwrap()
                .is_none()
        );
        assert!(repo.list(other).await.unwrap().is_empty());
        assert!(!repo.delete(other, id).await.unwrap());
        assert!(repo.get(project, id).await.unwrap().is_some());
    }

    #[sqlx::test(migrations = "../db/migrations")]
    async fn rejects_duplicate_domain_across_projects(pool: PgPool) {
        let project = create_project(&pool, "demo").await;
        let other = create_project(&pool, "other").await;
        let repo = PostgresDomainRepo::new(pool);
        repo.create(project, "example.com", &records())
            .await
            .unwrap();

        assert!(repo.create(other, "EXAMPLE.com", &records()).await.is_err());
    }
}

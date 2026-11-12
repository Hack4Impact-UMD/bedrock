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

#[derive(sqlx::Type, Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[sqlx(type_name = "courier.domain_scope", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum DomainScope {
    /// Owned by one project, which alone can send from it.
    Project,
    /// Owned by no project; any project can send from it.
    Shared,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DomainOwner {
    Project(ProjectId),
    Shared,
}

impl DomainOwner {
    fn scope(self) -> DomainScope {
        match self {
            Self::Project(_) => DomainScope::Project,
            Self::Shared => DomainScope::Shared,
        }
    }

    fn project_id(self) -> Option<Uuid> {
        match self {
            Self::Project(project) => Some(project.0),
            Self::Shared => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DomainCheck {
    // verification status
    pub status: DomainStatus,
    pub mail_from_status: DomainStatus,
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
    /// None exactly when `scope` is shared.
    pub project_id: Option<Uuid>,
    pub scope: DomainScope,
    pub domain: String,
    pub status: DomainStatus,
    pub mail_from_status: DomainStatus,
    pub dns_records: Json<Vec<DnsRecord>>,
    pub created_at: DateTime<Utc>,
    pub last_checked_at: Option<DateTime<Utc>>,
    pub verified_at: Option<DateTime<Utc>>,
}

impl Domain {
    pub fn owner(&self) -> DomainOwner {
        match (self.scope, self.project_id) {
            (DomainScope::Project, Some(id)) => DomainOwner::Project(ProjectId(id)),
            _ => DomainOwner::Shared,
        }
    }
}

pub trait DomainRepo {
    fn create(
        &self,
        owner: DomainOwner,
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
    /// The project's own domains followed by shared ones.
    fn list(&self, project: ProjectId) -> impl Future<Output = Result<Vec<Domain>>> + Send;
    /// Records the result of a verification check.
    fn record_check(
        &self,
        id: DomainId,
        check: DomainCheck,
    ) -> impl Future<Output = Result<Option<Domain>>> + Send;
    /// Returns whether a domain was deleted.
    fn delete(&self, owner: DomainOwner, id: DomainId)
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
        owner: DomainOwner,
        domain: &str,
        dns_records: &[DnsRecord],
    ) -> Result<Domain> {
        let domain: Domain = sqlx::query_as(
            r"
        insert into courier.domains (project_id, scope, domain, dns_records)
        values ($1, $2, $3, $4)
        returning *
        ",
        )
        .bind(owner.project_id())
        .bind(owner.scope())
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
        where id = $2 and (project_id = $1 or scope = 'shared')
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
        where lower(domain) = lower($2) and (project_id = $1 or scope = 'shared')
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
        where project_id = $1 or scope = 'shared'
        order by scope, created_at
        ",
        )
        .bind(project.0)
        .fetch_all(&self.db)
        .await?;

        Ok(domains)
    }

    async fn record_check(&self, id: DomainId, check: DomainCheck) -> Result<Option<Domain>> {
        let domain: Option<Domain> = sqlx::query_as(
            r"
        update courier.domains
        set status = $2,
            mail_from_status = $3,
            last_checked_at = now(),
            verified_at = case when $2 = 'verified' then coalesce(verified_at, now()) else verified_at end
        where id = $1
        returning *
        ",
        )
        .bind(id.0)
        .bind(check.status)
        .bind(check.mail_from_status)
        .fetch_optional(&self.db)
        .await?;

        Ok(domain)
    }

    async fn delete(&self, owner: DomainOwner, id: DomainId) -> Result<bool> {
        let result = sqlx::query(
            r"
        delete from courier.domains
        where id = $1 and scope = $2 and project_id is not distinct from $3
        ",
        )
        .bind(id.0)
        .bind(owner.scope())
        .bind(owner.project_id())
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

    fn dkim(status: DomainStatus) -> DomainCheck {
        DomainCheck {
            status,
            mail_from_status: DomainStatus::Pending,
        }
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
            .create(DomainOwner::Project(project), "example.com", &records())
            .await
            .unwrap();
        let id = DomainId(created.id);

        assert_eq!(created.status, DomainStatus::Pending);
        assert_eq!(created.mail_from_status, DomainStatus::Pending);
        assert_eq!(created.dns_records.0, records());
        assert!(created.last_checked_at.is_none());
        assert!(created.verified_at.is_none());

        let fetched = repo.get(project, id).await.unwrap().unwrap();
        assert_eq!(fetched.domain, "example.com");
        assert_eq!(fetched.dns_records.0, records());
        assert_eq!(repo.list(project).await.unwrap().len(), 1);

        assert!(
            repo.delete(DomainOwner::Project(project), id)
                .await
                .unwrap()
        );
        assert!(repo.get(project, id).await.unwrap().is_none());
        assert!(
            !repo
                .delete(DomainOwner::Project(project), id)
                .await
                .unwrap()
        );
        assert!(
            repo.record_check(id, dkim(DomainStatus::Verified))
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
            .create(DomainOwner::Project(project), "Example.com", &records())
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
            repo.create(DomainOwner::Project(project), "example.com", &records())
                .await
                .unwrap()
                .id,
        );

        let pending = repo
            .record_check(id, dkim(DomainStatus::Pending))
            .await
            .unwrap()
            .unwrap();
        assert!(pending.last_checked_at.is_some());
        assert!(pending.verified_at.is_none());

        let verified = repo
            .record_check(id, dkim(DomainStatus::Verified))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(verified.status, DomainStatus::Verified);
        assert!(verified.verified_at.is_some());

        let failed = repo
            .record_check(id, dkim(DomainStatus::Failed))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(failed.status, DomainStatus::Failed);
        assert_eq!(failed.verified_at, verified.verified_at);

        let reverified = repo
            .record_check(id, dkim(DomainStatus::Verified))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(reverified.verified_at, verified.verified_at);

        let mail_from = repo
            .record_check(
                id,
                DomainCheck {
                    status: DomainStatus::Verified,
                    mail_from_status: DomainStatus::Verified,
                },
            )
            .await
            .unwrap()
            .unwrap();
        assert_eq!(mail_from.mail_from_status, DomainStatus::Verified);
        assert_eq!(mail_from.verified_at, verified.verified_at);
    }

    #[sqlx::test(migrations = "../db/migrations")]
    async fn domains_are_scoped_to_their_project(pool: PgPool) {
        let project = create_project(&pool, "demo").await;
        let other = create_project(&pool, "other").await;
        let repo = PostgresDomainRepo::new(pool);
        let id = DomainId(
            repo.create(DomainOwner::Project(project), "example.com", &records())
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
        assert!(!repo.delete(DomainOwner::Project(other), id).await.unwrap());
        assert!(repo.get(project, id).await.unwrap().is_some());
    }

    #[sqlx::test(migrations = "../db/migrations")]
    async fn rejects_duplicate_domain_across_projects(pool: PgPool) {
        let project = create_project(&pool, "demo").await;
        let other = create_project(&pool, "other").await;
        let repo = PostgresDomainRepo::new(pool);
        repo.create(DomainOwner::Project(project), "example.com", &records())
            .await
            .unwrap();

        assert!(
            repo.create(DomainOwner::Project(other), "EXAMPLE.com", &records())
                .await
                .is_err()
        );
    }

    #[sqlx::test(migrations = "../db/migrations")]
    async fn shared_domains_are_visible_to_every_project(pool: PgPool) {
        let project = create_project(&pool, "demo").await;
        let other = create_project(&pool, "other").await;
        let repo = PostgresDomainRepo::new(pool);
        let own = repo
            .create(DomainOwner::Project(project), "own.com", &records())
            .await
            .unwrap();
        let shared = repo
            .create(DomainOwner::Shared, "shared.com", &records())
            .await
            .unwrap();
        let id = DomainId(shared.id);

        assert_eq!(shared.scope, DomainScope::Shared);
        assert!(shared.project_id.is_none());
        assert_eq!(shared.owner(), DomainOwner::Shared);
        assert_eq!(own.owner(), DomainOwner::Project(project));

        for p in [project, other] {
            assert!(repo.get(p, id).await.unwrap().is_some());
            let found = repo.get_by_name(p, "SHARED.com").await.unwrap().unwrap();
            assert_eq!(found.id, shared.id);
        }

        let listed: Vec<Uuid> = repo
            .list(project)
            .await
            .unwrap()
            .iter()
            .map(|d| d.id)
            .collect();
        assert_eq!(listed, vec![own.id, shared.id]);
        let listed: Vec<Uuid> = repo
            .list(other)
            .await
            .unwrap()
            .iter()
            .map(|d| d.id)
            .collect();
        assert_eq!(listed, vec![shared.id]);
    }

    #[sqlx::test(migrations = "../db/migrations")]
    async fn delete_requires_the_owner(pool: PgPool) {
        let project = create_project(&pool, "demo").await;
        let repo = PostgresDomainRepo::new(pool);
        let own = DomainId(
            repo.create(DomainOwner::Project(project), "own.com", &records())
                .await
                .unwrap()
                .id,
        );
        let shared = DomainId(
            repo.create(DomainOwner::Shared, "shared.com", &records())
                .await
                .unwrap()
                .id,
        );

        assert!(
            !repo
                .delete(DomainOwner::Project(project), shared)
                .await
                .unwrap()
        );
        assert!(!repo.delete(DomainOwner::Shared, own).await.unwrap());
        assert!(repo.get(project, shared).await.unwrap().is_some());
        assert!(repo.get(project, own).await.unwrap().is_some());

        assert!(repo.delete(DomainOwner::Shared, shared).await.unwrap());
        assert!(
            repo.delete(DomainOwner::Project(project), own)
                .await
                .unwrap()
        );
        assert!(repo.get(project, shared).await.unwrap().is_none());
        assert!(repo.get(project, own).await.unwrap().is_none());
    }

    #[sqlx::test(migrations = "../db/migrations")]
    async fn rejects_duplicate_between_shared_and_project(pool: PgPool) {
        let project = create_project(&pool, "demo").await;
        let repo = PostgresDomainRepo::new(pool);
        repo.create(DomainOwner::Shared, "example.com", &records())
            .await
            .unwrap();

        assert!(
            repo.create(DomainOwner::Project(project), "Example.com", &records())
                .await
                .is_err()
        );
        assert!(
            repo.create(DomainOwner::Shared, "EXAMPLE.com", &records())
                .await
                .is_err()
        );
    }

    #[sqlx::test(migrations = "../db/migrations")]
    async fn scope_must_match_project(pool: PgPool) {
        let project = create_project(&pool, "demo").await;
        let insert = |scope: DomainScope, project_id: Option<Uuid>| {
            sqlx::query(
                r"
            insert into courier.domains (project_id, scope, domain, dns_records)
            values ($1, $2, 'example.com', '[]')
            ",
            )
            .bind(project_id)
            .bind(scope)
            .execute(&pool)
        };

        assert!(insert(DomainScope::Project, None).await.is_err());
        assert!(insert(DomainScope::Shared, Some(project.0)).await.is_err());
    }
}

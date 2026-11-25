use anyhow::Result;
use db::PgPool;
use identity::ProjectId;
use serde::{Deserialize, Serialize};
use sqlx::prelude::FromRow;
use sqlx::types::chrono::DateTime;
use sqlx::types::chrono::Utc;
use uuid::Uuid;

use crate::domains::DomainId;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EmailId(pub Uuid);

#[derive(Default, sqlx::Type, Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[sqlx(type_name = "courier.send_mode", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum SendMode {
    #[default]
    Live,
    Test,
}

#[derive(sqlx::Type, Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[sqlx(type_name = "courier.email_status", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum EmailStatus {
    Sent,
    /// The provider definitely did not accept the message.
    Failed,
    /// The send failed in a way that leaves it unclear whether the provider accepted the message,
    /// so it may still be delivered. Callers must not treat it as safe to retry.
    Unknown,
}

#[derive(FromRow, Debug, Serialize)]
pub struct Email {
    pub id: Uuid,
    pub project_id: Uuid,
    pub domain_id: Option<Uuid>,
    pub mode: SendMode,
    pub from_address: String,
    pub to_addresses: Vec<String>,
    pub cc_addresses: Vec<String>,
    pub bcc_addresses: Vec<String>,
    pub subject: String,
    /// The HTML part, if the email had one.
    pub body: Option<String>,
    pub status: EmailStatus,
    pub provider_message_id: Option<String>,
    pub error: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// used to create email row
#[derive(Debug)]
pub struct NewEmail<'a> {
    pub domain: DomainId,
    pub status: EmailStatus,
    pub mode: SendMode,
    pub from_address: &'a str,
    pub to_addresses: &'a [String],
    pub cc_addresses: &'a [String],
    pub bcc_addresses: &'a [String],
    pub subject: &'a str,
    pub body: &'a str,
    pub provider_message_id: Option<&'a str>,
    pub error: Option<&'a str>,
}

/// What the provider said about a send.
#[derive(Debug)]
pub struct SendOutcome<'a> {
    pub status: EmailStatus,
    pub provider_message_id: Option<&'a str>,
    pub error: Option<&'a str>,
}

pub trait EmailRepo {
    fn create(
        &self,
        project: ProjectId,
        email: &NewEmail<'_>,
    ) -> impl Future<Output = Result<Email>> + Send;
    /// Records the provider's answer for an email created before it was sent.
    fn record_outcome(
        &self,
        id: EmailId,
        outcome: &SendOutcome<'_>,
    ) -> impl Future<Output = Result<Option<Email>>> + Send;
    fn get(&self, id: EmailId) -> impl Future<Output = Result<Option<Email>>> + Send;
    /// Newest first. Pass the last email of a page as `before` to get the next page.
    fn list(
        &self,
        project: ProjectId,
        limit: i64,
        before: Option<EmailId>,
    ) -> impl Future<Output = Result<Vec<Email>>> + Send;
    fn list_from_domain(
        &self,
        domain: DomainId,
        limit: i64,
        before: Option<EmailId>,
    ) -> impl Future<Output = Result<Vec<Email>>> + Send;
}

#[derive(Clone)]
pub struct PostgresEmailRepo {
    db: PgPool,
}

impl PostgresEmailRepo {
    pub fn new(db: PgPool) -> Self {
        Self { db }
    }
}

impl EmailRepo for PostgresEmailRepo {
    async fn create(&self, project: ProjectId, email: &NewEmail<'_>) -> Result<Email> {
        let email: Email = sqlx::query_as(
            r"
        insert into courier.emails (
            project_id, domain_id, mode, from_address, to_addresses, cc_addresses, bcc_addresses,
            subject, body, status, provider_message_id, error
        )
        values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
        returning *
        ",
        )
        .bind(project.0)
        .bind(email.domain.0)
        .bind(email.mode)
        .bind(email.from_address)
        .bind(email.to_addresses)
        .bind(email.cc_addresses)
        .bind(email.bcc_addresses)
        .bind(email.subject)
        .bind(email.body)
        .bind(email.status)
        .bind(email.provider_message_id)
        .bind(email.error)
        .fetch_one(&self.db)
        .await?;

        Ok(email)
    }

    async fn record_outcome(
        &self,
        id: EmailId,
        outcome: &SendOutcome<'_>,
    ) -> Result<Option<Email>> {
        let email: Option<Email> = sqlx::query_as(
            r"
        update courier.emails
        set status = $2, provider_message_id = $3, error = $4
        where id = $1
        returning *
        ",
        )
        .bind(id.0)
        .bind(outcome.status)
        .bind(outcome.provider_message_id)
        .bind(outcome.error)
        .fetch_optional(&self.db)
        .await?;

        Ok(email)
    }

    async fn get(&self, id: EmailId) -> Result<Option<Email>> {
        let email: Option<Email> = sqlx::query_as(
            r"
        select * from courier.emails
        where id = $1
        ",
        )
        .bind(id.0)
        .fetch_optional(&self.db)
        .await?;

        Ok(email)
    }

    async fn list(
        &self,
        project: ProjectId,
        limit: i64,
        before: Option<EmailId>,
    ) -> Result<Vec<Email>> {
        let emails: Vec<Email> = sqlx::query_as(
            r"
        select * from courier.emails
        where project_id = $1
          and (
            $3::uuid is null
            or (created_at, id) < (
                select created_at, id from courier.emails
                where project_id = $1 and id = $3
            )
          )
        order by created_at desc, id desc
        limit $2
        ",
        )
        .bind(project.0)
        .bind(limit)
        .bind(before.map(|e| e.0))
        .fetch_all(&self.db)
        .await?;

        Ok(emails)
    }

    async fn list_from_domain(
        &self,
        domain_id: DomainId,
        limit: i64,
        before: Option<EmailId>,
    ) -> Result<Vec<Email>> {
        let emails: Vec<Email> = sqlx::query_as(
            r"
        select * from courier.emails
        where domain_id = $1
          and (
            $3::uuid is null
            or (created_at, id) < (
                select created_at, id from courier.emails
                where domain_id = $1 and id = $3
            )
          )
        order by created_at desc, id desc
        limit $2
        ",
        )
        .bind(domain_id.0)
        .bind(limit)
        .bind(before.map(|e| e.0))
        .fetch_all(&self.db)
        .await?;

        Ok(emails)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domains::{DomainOwner, DomainRepo, PostgresDomainRepo};

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

    async fn create_domain(pool: &PgPool, owner: DomainOwner, name: &str) -> DomainId {
        let domain = PostgresDomainRepo::new(pool.clone())
            .create(owner, name, &[])
            .await
            .unwrap();

        DomainId(domain.id)
    }

    fn email<'a>(domain: DomainId, to: &'a [String]) -> NewEmail<'a> {
        NewEmail {
            domain,
            status: EmailStatus::Sent,
            mode: SendMode::Live,
            from_address: "hi@example.com",
            to_addresses: to,
            cc_addresses: &[],
            bcc_addresses: &[],
            subject: "Hello",
            body: "Body",
            provider_message_id: Some("ses-123"),
            error: None,
        }
    }

    #[sqlx::test(migrations = "../db/migrations")]
    async fn create_and_get(pool: PgPool) {
        let project = create_project(&pool, "demo").await;
        let domain = create_domain(&pool, DomainOwner::Project(project), "example.com").await;
        let repo = PostgresEmailRepo::new(pool);
        let to = vec!["a@test.com".to_string(), "b@test.com".to_string()];
        let cc = vec!["c@test.com".to_string()];

        let created = repo
            .create(
                project,
                &NewEmail {
                    cc_addresses: &cc,
                    ..email(domain, &to)
                },
            )
            .await
            .unwrap();

        let fetched = repo.get(EmailId(created.id)).await.unwrap().unwrap();
        assert_eq!(fetched.domain_id, Some(domain.0));
        assert_eq!(fetched.mode, SendMode::Live);
        assert_eq!(fetched.status, EmailStatus::Sent);
        assert_eq!(fetched.to_addresses, to);
        assert_eq!(fetched.cc_addresses, cc);
        assert!(fetched.bcc_addresses.is_empty());
        assert_eq!(fetched.body.as_deref(), Some("Body"));
        assert_eq!(fetched.provider_message_id.as_deref(), Some("ses-123"));
        assert!(fetched.error.is_none());
    }

    #[sqlx::test(migrations = "../db/migrations")]
    async fn stores_failed_and_unknown_status(pool: PgPool) {
        let project = create_project(&pool, "demo").await;
        let domain = create_domain(&pool, DomainOwner::Project(project), "example.com").await;
        let repo = PostgresEmailRepo::new(pool);
        let to = vec!["a@test.com".to_string()];

        for status in [EmailStatus::Failed, EmailStatus::Unknown] {
            let created = repo
                .create(
                    project,
                    &NewEmail {
                        status,
                        provider_message_id: None,
                        error: Some("boom"),
                        ..email(domain, &to)
                    },
                )
                .await
                .unwrap();

            assert_eq!(created.status, status);
            assert_eq!(created.error.as_deref(), Some("boom"));
        }
    }

    #[sqlx::test(migrations = "../db/migrations")]
    async fn list_pages_newest_first(pool: PgPool) {
        let project = create_project(&pool, "demo").await;
        let domain = create_domain(&pool, DomainOwner::Project(project), "example.com").await;
        let repo = PostgresEmailRepo::new(pool);
        let to = vec!["a@test.com".to_string()];

        for _ in 0..5 {
            repo.create(project, &email(domain, &to)).await.unwrap();
        }

        let all: Vec<Uuid> = repo
            .list(project, 10, None)
            .await
            .unwrap()
            .iter()
            .map(|e| e.id)
            .collect();
        assert_eq!(all.len(), 5);

        let first = repo.list(project, 2, None).await.unwrap();
        let second = repo
            .list(project, 2, Some(EmailId(first[1].id)))
            .await
            .unwrap();
        let third = repo
            .list(project, 2, Some(EmailId(second[1].id)))
            .await
            .unwrap();
        let paged: Vec<Uuid> = first
            .iter()
            .chain(&second)
            .chain(&third)
            .map(|e| e.id)
            .collect();
        assert_eq!(paged, all);
        assert_eq!(third.len(), 1);
    }

    #[sqlx::test(migrations = "../db/migrations")]
    async fn list_from_domain_pages_newest_first(pool: PgPool) {
        let project = create_project(&pool, "demo").await;
        let domain = create_domain(&pool, DomainOwner::Project(project), "example.com").await;
        let other = create_domain(&pool, DomainOwner::Project(project), "other.com").await;
        let repo = PostgresEmailRepo::new(pool);
        let to = vec!["a@test.com".to_string()];

        for _ in 0..3 {
            repo.create(project, &email(domain, &to)).await.unwrap();
        }
        let other_id = EmailId(repo.create(project, &email(other, &to)).await.unwrap().id);

        let all: Vec<Uuid> = repo
            .list_from_domain(domain, 10, None)
            .await
            .unwrap()
            .iter()
            .map(|e| e.id)
            .collect();
        assert_eq!(all.len(), 3);

        let first = repo.list_from_domain(domain, 2, None).await.unwrap();
        let second = repo
            .list_from_domain(domain, 2, Some(EmailId(first[1].id)))
            .await
            .unwrap();
        let paged: Vec<Uuid> = first.iter().chain(&second).map(|e| e.id).collect();
        assert_eq!(paged, all);

        // A cursor from another domain matches nothing.
        assert!(
            repo.list_from_domain(domain, 10, Some(other_id))
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[sqlx::test(migrations = "../db/migrations")]
    async fn emails_are_scoped_to_their_project(pool: PgPool) {
        let project = create_project(&pool, "demo").await;
        let other = create_project(&pool, "other").await;
        let domain = create_domain(&pool, DomainOwner::Project(project), "example.com").await;
        let repo = PostgresEmailRepo::new(pool);
        let to = vec!["a@test.com".to_string()];
        let id = EmailId(repo.create(project, &email(domain, &to)).await.unwrap().id);

        assert!(repo.list(other, 10, None).await.unwrap().is_empty());
        assert!(repo.list(other, 10, Some(id)).await.unwrap().is_empty());
    }

    #[sqlx::test(migrations = "../db/migrations")]
    async fn deleting_domain_keeps_email(pool: PgPool) {
        let project = create_project(&pool, "demo").await;
        let domain = create_domain(&pool, DomainOwner::Project(project), "example.com").await;
        let repo = PostgresEmailRepo::new(pool.clone());
        let to = vec!["a@test.com".to_string()];
        let id = EmailId(repo.create(project, &email(domain, &to)).await.unwrap().id);

        assert!(
            PostgresDomainRepo::new(pool.clone())
                .delete(DomainOwner::Project(project), domain)
                .await
                .unwrap()
        );

        let email = repo.get(id).await.unwrap().unwrap();
        assert_eq!(email.project_id, project.0);
        assert!(email.domain_id.is_none());
    }

    #[sqlx::test(migrations = "../db/migrations")]
    async fn any_project_can_send_from_a_shared_domain(pool: PgPool) {
        let project = create_project(&pool, "demo").await;
        let other = create_project(&pool, "other").await;
        let domain = create_domain(&pool, DomainOwner::Shared, "example.com").await;
        let repo = PostgresEmailRepo::new(pool.clone());
        let to = vec!["a@test.com".to_string()];

        let mut ids = Vec::new();
        for p in [project, other] {
            let created = repo.create(p, &email(domain, &to)).await.unwrap();
            assert_eq!(created.domain_id, Some(domain.0));
            ids.push(EmailId(created.id));
        }

        assert!(
            PostgresDomainRepo::new(pool)
                .delete(DomainOwner::Shared, domain)
                .await
                .unwrap()
        );
        for id in ids {
            let email = repo.get(id).await.unwrap().unwrap();
            assert!(email.domain_id.is_none());
        }
    }
}

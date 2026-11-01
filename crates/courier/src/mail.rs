use anyhow::Result;
use db::PgPool;
use identity::{ApiKeyId, ProjectId};
use serde::{Deserialize, Serialize};
use sqlx::prelude::FromRow;
use sqlx::types::chrono::DateTime;
use sqlx::types::chrono::Utc;
use uuid::Uuid;

use crate::domains::DomainId;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EmailId(pub Uuid);

#[derive(sqlx::Type, Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[sqlx(type_name = "courier.send_mode", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum SendMode {
    Live,
    Test,
}

#[derive(sqlx::Type, Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[sqlx(type_name = "courier.email_status", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum EmailStatus {
    Sent,
    Failed,
}

#[derive(FromRow, Debug, Serialize)]
pub struct Email {
    pub id: Uuid,
    pub project_id: Uuid,
    pub api_key_id: Option<Uuid>,
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

/// The outcome of a send attempt, recorded once it has been made.
#[derive(Debug)]
pub struct NewEmail<'a> {
    pub api_key: Option<ApiKeyId>,
    pub domain: Option<DomainId>,
    pub mode: SendMode,
    pub from_address: &'a str,
    pub to_addresses: &'a [String],
    pub cc_addresses: &'a [String],
    pub bcc_addresses: &'a [String],
    pub subject: &'a str,
    pub body: Option<&'a str>,
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
    fn get(
        &self,
        project: ProjectId,
        id: EmailId,
    ) -> impl Future<Output = Result<Option<Email>>> + Send;
    /// Newest first. Pass the last email of a page as `before` to get the next page.
    fn list(
        &self,
        project: ProjectId,
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
            project_id, api_key_id, domain_id, mode, from_address, to_addresses, cc_addresses,
            bcc_addresses, subject, body, status, provider_message_id, error
        )
        values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)
        returning *
        ",
        )
        .bind(project.0)
        .bind(email.api_key.map(|k| k.0))
        .bind(email.domain.map(|d| d.0))
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

    async fn get(&self, project: ProjectId, id: EmailId) -> Result<Option<Email>> {
        let email: Option<Email> = sqlx::query_as(
            r"
        select * from courier.emails
        where project_id = $1 and id = $2
        ",
        )
        .bind(project.0)
        .bind(id.0)
        .fetch_optional(&self.db)
        .await?;

        Ok(email)
    }

    // Keyset pagination over the (project_id, created_at desc, id) index. A `before` id that
    // doesn't belong to the project matches nothing, so the page is empty.
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domains::{DomainRepo, PostgresDomainRepo};

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

    async fn create_key(pool: &PgPool, project: ProjectId, prefix: &str) -> ApiKeyId {
        let (id,): (Uuid,) = sqlx::query_as(
            r"
        insert into identity.api_keys (project_id, name, prefix, hash)
        values ($1, 'test', $2, $2::bytea)
        returning id
        ",
        )
        .bind(project.0)
        .bind(prefix)
        .fetch_one(pool)
        .await
        .unwrap();

        ApiKeyId(id)
    }

    async fn create_domain(pool: &PgPool, project: ProjectId, name: &str) -> DomainId {
        let domain = PostgresDomainRepo::new(pool.clone())
            .create(project, name, &[])
            .await
            .unwrap();

        DomainId(domain.id)
    }

    fn email<'a>(to: &'a [String]) -> NewEmail<'a> {
        NewEmail {
            api_key: None,
            domain: None,
            mode: SendMode::Live,
            from_address: "hi@example.com",
            to_addresses: to,
            cc_addresses: &[],
            bcc_addresses: &[],
            subject: "Hello",
            body: Some("Body"),
            status: EmailStatus::Sent,
            provider_message_id: Some("ses-123"),
            error: None,
        }
    }

    #[sqlx::test(migrations = "../db/migrations")]
    async fn create_and_get(pool: PgPool) {
        let project = create_project(&pool, "demo").await;
        let key = create_key(&pool, project, "bdrk_12345678").await;
        let domain = create_domain(&pool, project, "example.com").await;
        let repo = PostgresEmailRepo::new(pool);
        let to = vec!["a@test.com".to_string(), "b@test.com".to_string()];
        let cc = vec!["c@test.com".to_string()];

        let created = repo
            .create(
                project,
                &NewEmail {
                    api_key: Some(key),
                    domain: Some(domain),
                    cc_addresses: &cc,
                    ..email(&to)
                },
            )
            .await
            .unwrap();

        let fetched = repo
            .get(project, EmailId(created.id))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fetched.api_key_id, Some(key.0));
        assert_eq!(fetched.domain_id, Some(domain.0));
        assert_eq!(fetched.mode, SendMode::Live);
        assert_eq!(fetched.status, EmailStatus::Sent);
        assert_eq!(fetched.to_addresses, to);
        assert_eq!(fetched.cc_addresses, cc);
        assert!(fetched.bcc_addresses.is_empty());
        assert_eq!(fetched.body.as_deref(), Some("Body"));
        assert_eq!(fetched.provider_message_id.as_deref(), Some("ses-123"));
    }

    #[sqlx::test(migrations = "../db/migrations")]
    async fn list_pages_newest_first(pool: PgPool) {
        let project = create_project(&pool, "demo").await;
        let repo = PostgresEmailRepo::new(pool);
        let to = vec!["a@test.com".to_string()];

        let mut ids = Vec::new();
        for _ in 0..5 {
            ids.push(repo.create(project, &email(&to)).await.unwrap().id);
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
    async fn emails_are_scoped_to_their_project(pool: PgPool) {
        let project = create_project(&pool, "demo").await;
        let other = create_project(&pool, "other").await;
        let repo = PostgresEmailRepo::new(pool);
        let to = vec!["a@test.com".to_string()];
        let id = EmailId(repo.create(project, &email(&to)).await.unwrap().id);

        assert!(repo.get(other, id).await.unwrap().is_none());
        assert!(repo.list(other, 10, None).await.unwrap().is_empty());
        assert!(repo.list(other, 10, Some(id)).await.unwrap().is_empty());
        assert!(repo.get(project, id).await.unwrap().is_some());
    }

    #[sqlx::test(migrations = "../db/migrations")]
    async fn rejects_key_or_domain_from_another_project(pool: PgPool) {
        let project = create_project(&pool, "demo").await;
        let other = create_project(&pool, "other").await;
        let key = create_key(&pool, other, "bdrk_12345678").await;
        let domain = create_domain(&pool, other, "example.com").await;
        let repo = PostgresEmailRepo::new(pool);
        let to = vec!["a@test.com".to_string()];

        let with_key = NewEmail {
            api_key: Some(key),
            ..email(&to)
        };
        assert!(repo.create(project, &with_key).await.is_err());

        let with_domain = NewEmail {
            domain: Some(domain),
            ..email(&to)
        };
        assert!(repo.create(project, &with_domain).await.is_err());
    }

    #[sqlx::test(migrations = "../db/migrations")]
    async fn deleting_key_or_domain_keeps_email(pool: PgPool) {
        let project = create_project(&pool, "demo").await;
        let key = create_key(&pool, project, "bdrk_12345678").await;
        let domain = create_domain(&pool, project, "example.com").await;
        let repo = PostgresEmailRepo::new(pool.clone());
        let to = vec!["a@test.com".to_string()];
        let id = EmailId(
            repo.create(
                project,
                &NewEmail {
                    api_key: Some(key),
                    domain: Some(domain),
                    ..email(&to)
                },
            )
            .await
            .unwrap()
            .id,
        );

        sqlx::query("delete from identity.api_keys where id = $1")
            .bind(key.0)
            .execute(&pool)
            .await
            .unwrap();
        assert!(
            PostgresDomainRepo::new(pool.clone())
                .delete(project, domain)
                .await
                .unwrap()
        );

        let email = repo.get(project, id).await.unwrap().unwrap();
        assert_eq!(email.project_id, project.0);
        assert!(email.api_key_id.is_none());
        assert!(email.domain_id.is_none());
    }
}

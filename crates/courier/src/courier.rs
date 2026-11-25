use std::str::FromStr;

use db::PgPool;
use email_address::EmailAddress;
use identity::ProjectId;
use idna::uts46::{AsciiDenyList, DnsLength, Hyphens, Uts46};

use crate::{
    DomainId,
    courier::CourierError::NotFound,
    domains::{Domain, DomainCheck, DomainOwner, DomainRepo, PostgresDomainRepo},
    mail::{
        Email, EmailId, EmailRepo, EmailStatus, NewEmail, PostgresEmailRepo, SendMode, SendOutcome,
    },
    mail_provider::{MailProvider, MessageId, OutgoingEmail, ProviderError, SesMailProvider},
};

#[derive(Clone)]
pub struct Courier<D: DomainRepo, M: EmailRepo> {
    domains: D,
    email: M,
    ses_provider: SesMailProvider,
}

#[derive(Debug, thiserror::Error)]
pub enum CourierError {
    #[error("domain not found")]
    NotFound,
    #[error("domain taken")]
    Taken,
    #[error("not a valid domain name: {0}")]
    InvalidDomain(String),
    #[error(transparent)]
    InvalidAddress(#[from] email_address::Error),
    #[error(transparent)]
    Provider(#[from] ProviderError),
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

fn test_message() -> MessageId {
    MessageId(uuid::Uuid::new_v4().to_string())
}

impl Courier<PostgresDomainRepo, PostgresEmailRepo> {
    pub fn postgres(pool: PgPool, aws_client: aws_sdk_sesv2::Client) -> Self {
        Self {
            domains: PostgresDomainRepo::new(pool.clone()),
            email: PostgresEmailRepo::new(pool),
            ses_provider: SesMailProvider::new(aws_client),
        }
    }
}

impl<D: DomainRepo, M: EmailRepo> Courier<D, M> {
    pub async fn create_sender_identity(
        &self,
        owner: DomainOwner,
        domain: &str,
    ) -> Result<Domain, CourierError> {
        let domain = normalize_domain(domain)?;
        let records = self.ses_provider.create_identity(&domain).await?;
        match self.domains.create(owner, &domain, &records).await {
            Ok(domain) => Ok(domain),
            Err(err) => {
                self.ses_provider.delete_identity(&domain).await?;
                Err(domain_error(err))
            }
        }
    }

    pub async fn get_sender_identity(&self, domain: &str) -> Result<Domain, CourierError> {
        self.domains
            .get_by_name(domain)
            .await?
            .ok_or(CourierError::NotFound)
    }

    pub async fn get_sender_identities_for_project(
        &self,
        project: ProjectId,
    ) -> Result<Vec<Domain>, CourierError> {
        Ok(self.domains.list(project).await?)
    }

    pub async fn check_domain_status(&self, domain: &str) -> Result<DomainCheck, CourierError> {
        let d = self.get_sender_identity(domain).await?;
        let status = self.ses_provider.check_identity(domain).await?;

        self.domains.record_check(d.into(), status).await?;

        Ok(status)
    }

    pub async fn delete_sender_identity(
        &self,
        owner: DomainOwner,
        domain: &str,
    ) -> Result<bool, CourierError> {
        let resolved = self.get_sender_identity(domain).await?;
        if resolved.owner() != owner {
            return Ok(false);
        }

        match self.ses_provider.delete_identity(&resolved.domain).await {
            Ok(()) | Err(ProviderError::NotFound) => {
                Ok(self.domains.delete(owner, resolved.into()).await?)
            }
            Err(err) => Err(CourierError::Provider(err)),
        }
    }

    pub async fn get_email(&self, id: EmailId) -> Result<Email, CourierError> {
        self.email.get(id).await?.ok_or(CourierError::NotFound)
    }

    pub async fn list_email(
        &self,
        project_id: ProjectId,
        limit: i64,
        before: Option<EmailId>,
    ) -> Result<Vec<Email>, CourierError> {
        Ok(self.email.list(project_id, limit, before).await?)
    }

    pub async fn list_email_from_domain(
        &self,
        from_domain: &str,
        limit: i64,
        before: Option<EmailId>,
    ) -> Result<Vec<Email>, CourierError> {
        let domain = self
            .domains
            .get_by_name(from_domain)
            .await?
            .ok_or(NotFound)?;

        Ok(self
            .email
            .list_from_domain(domain.into(), limit, before)
            .await?)
    }

    pub async fn list_email_from_domain_id(
        &self,
        domain_id: DomainId,
        limit: i64,
        before: Option<EmailId>,
    ) -> Result<Vec<Email>, CourierError> {
        Ok(self
            .email
            .list_from_domain(domain_id, limit, before)
            .await?)
    }

    pub async fn send_email(
        &self,
        project_id: ProjectId,
        mail: &OutgoingEmail<'_>,
        mode: SendMode,
    ) -> Result<Email, CourierError> {
        let addr = EmailAddress::from_str(mail.from)?;
        let domain = self
            .domains
            .get_by_name(addr.domain())
            .await?
            .ok_or(CourierError::NotFound)?;

        let owner = domain.owner();
        if owner != DomainOwner::Shared && owner != DomainOwner::Project(project_id) {
            return Err(CourierError::NotFound);
        }

        let new = NewEmail {
            domain: domain.into(),
            status: EmailStatus::Unknown,
            mode,
            from_address: mail.from,
            to_addresses: mail.to,
            cc_addresses: mail.cc,
            bcc_addresses: mail.bcc,
            subject: mail.subject,
            body: match mail.body {
                crate::mail_provider::EmailBody::Text(t) => t,
                crate::mail_provider::EmailBody::Html(h) => h,
                crate::mail_provider::EmailBody::Both { html, text: _ } => html,
            },
            provider_message_id: None,
            error: None,
        };
        // record before send in case it fails
        let pending = self.email.create(project_id, &new).await?;

        let mail_id = match mode {
            SendMode::Live => self.ses_provider.send_email(mail).await,
            SendMode::Test => Ok(test_message()),
        };

        let (status, id, error) = match mail_id {
            Ok(id) => (EmailStatus::Sent, Some(id.0), None),
            Err(e @ ProviderError::Uncertain(_)) => {
                (EmailStatus::Unknown, None, Some(e.to_string()))
            }
            Err(e) => (EmailStatus::Failed, None, Some(e.to_string())),
        };
        let outcome = SendOutcome {
            status,
            provider_message_id: id.as_deref(),
            error: error.as_deref(),
        };

        match self
            .email
            .record_outcome(EmailId(pending.id), &outcome)
            .await
        {
            Ok(Some(email)) => Ok(email),
            Ok(None) | Err(_) => Ok(pending),
        }
    }
}

fn normalize_domain(domain: &str) -> Result<String, CourierError> {
    let invalid = || CourierError::InvalidDomain(domain.to_string());
    let trimmed = domain.trim().trim_end_matches('.');

    let ascii = Uts46::new()
        .to_ascii(
            trimmed.as_bytes(),
            AsciiDenyList::STD3,
            Hyphens::CheckFirstLast,
            DnsLength::Verify,
        )
        .map_err(|_| invalid())?;

    match ascii.rsplit_once('.') {
        Some((_, tld)) if !tld.bytes().all(|b| b.is_ascii_digit()) => Ok(ascii.into_owned()),
        _ => Err(invalid()),
    }
}

fn domain_error(err: anyhow::Error) -> CourierError {
    let constraint = err
        .downcast_ref::<sqlx::Error>()
        .and_then(|e| e.as_database_error())
        .and_then(|e| e.constraint());

    match constraint {
        Some("domains_domain_key") => CourierError::Taken,
        _ => err.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{domains::DomainStatus, mail_provider::EmailBody};
    use uuid::Uuid;

    type PgCourier = Courier<PostgresDomainRepo, PostgresEmailRepo>;

    async fn courier(pool: PgPool) -> Option<PgCourier> {
        let client = crate::test_support::ses_client().await?;
        Some(Courier::postgres(pool, client))
    }

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

    fn domain() -> String {
        format!("{}.example.com", Uuid::new_v4())
    }

    fn mail<'a>(from: &'a str, to: &'a [String]) -> OutgoingEmail<'a> {
        OutgoingEmail {
            from,
            to,
            cc: &[],
            bcc: &[],
            subject: "hello",
            body: EmailBody::Both {
                html: "<p>hi</p>",
                text: "hi",
            },
        }
    }

    #[sqlx::test(migrations = "../db/migrations")]
    async fn create_sender_identity_registers_with_ses_and_stores_it(pool: PgPool) {
        let project = create_project(&pool, "demo").await;
        let Some(courier) = courier(pool).await else {
            return;
        };
        let name = domain();

        let created = courier
            .create_sender_identity(DomainOwner::Project(project), &name)
            .await
            .unwrap();

        assert_eq!(created.domain, name);
        assert_eq!(created.owner(), DomainOwner::Project(project));
        assert_eq!(created.status, DomainStatus::Pending);
        assert!(!created.dns_records.is_empty());

        let fetched = courier.get_sender_identity(&name).await.unwrap();
        assert_eq!(fetched.id, created.id);

        let listed = courier
            .get_sender_identities_for_project(project)
            .await
            .unwrap();
        assert!(listed.iter().any(|d| d.id == created.id));

        let check = courier.ses_provider.check_identity(&name).await.unwrap();
        assert_eq!(check.status, DomainStatus::Pending);
    }

    #[sqlx::test(migrations = "../db/migrations")]
    async fn create_existing_sender_identity_fails(pool: PgPool) {
        let project = create_project(&pool, "demo").await;
        let Some(courier) = courier(pool).await else {
            return;
        };
        let name = domain();
        courier
            .create_sender_identity(DomainOwner::Project(project), &name)
            .await
            .unwrap();

        let result = courier
            .create_sender_identity(DomainOwner::Project(project), &name)
            .await;

        assert!(result.is_err());
    }

    #[sqlx::test(migrations = "../db/migrations")]
    async fn get_unknown_sender_identity_is_not_found(pool: PgPool) {
        let Some(courier) = courier(pool).await else {
            return;
        };

        let err = courier.get_sender_identity(&domain()).await.unwrap_err();

        assert!(matches!(err, CourierError::NotFound), "{err:?}");
    }

    #[sqlx::test(migrations = "../db/migrations")]
    async fn check_domain_status_records_the_check(pool: PgPool) {
        let project = create_project(&pool, "demo").await;
        let Some(courier) = courier(pool).await else {
            return;
        };
        let name = domain();
        let created = courier
            .create_sender_identity(DomainOwner::Project(project), &name)
            .await
            .unwrap();
        assert!(created.last_checked_at.is_none());

        let check = courier.check_domain_status(&name).await.unwrap();

        assert_eq!(check.status, DomainStatus::Pending);
        assert_eq!(check.mail_from_status, DomainStatus::Pending);
        let fetched = courier.get_sender_identity(&name).await.unwrap();
        assert!(fetched.last_checked_at.is_some());
        assert!(fetched.verified_at.is_none());
    }

    #[sqlx::test(migrations = "../db/migrations")]
    async fn delete_sender_identity_removes_it_from_ses_and_db(pool: PgPool) {
        let project = create_project(&pool, "demo").await;
        let Some(courier) = courier(pool).await else {
            return;
        };
        let name = domain();
        courier
            .create_sender_identity(DomainOwner::Project(project), &name)
            .await
            .unwrap();

        let deleted = courier
            .delete_sender_identity(DomainOwner::Project(project), &name)
            .await
            .unwrap();

        assert!(deleted);
        let err = courier.get_sender_identity(&name).await.unwrap_err();
        assert!(matches!(err, CourierError::NotFound), "{err:?}");
        let err = courier
            .ses_provider
            .check_identity(&name)
            .await
            .unwrap_err();
        assert!(matches!(err, ProviderError::NotFound), "{err:?}");
    }

    #[sqlx::test(migrations = "../db/migrations")]
    async fn delete_sender_identity_by_another_owner_leaves_it(pool: PgPool) {
        let owner = create_project(&pool, "owner").await;
        let other = create_project(&pool, "other").await;
        let Some(courier) = courier(pool).await else {
            return;
        };
        let name = domain();
        courier
            .create_sender_identity(DomainOwner::Project(owner), &name)
            .await
            .unwrap();

        let deleted = courier
            .delete_sender_identity(DomainOwner::Project(other), &name)
            .await
            .unwrap();

        assert!(!deleted);
        courier.get_sender_identity(&name).await.unwrap();
        courier.ses_provider.check_identity(&name).await.unwrap();
    }

    #[sqlx::test(migrations = "../db/migrations")]
    async fn create_sender_identity_rejects_non_domains(pool: PgPool) {
        let project = create_project(&pool, "demo").await;
        let Some(courier) = courier(pool).await else {
            return;
        };

        let err = courier
            .create_sender_identity(DomainOwner::Project(project), "someone@example.com")
            .await
            .unwrap_err();

        assert!(matches!(err, CourierError::InvalidDomain(_)), "{err:?}");
    }

    #[test]
    fn normalize_domain_trims_and_lowercases() {
        assert_eq!(normalize_domain(" Example.COM. ").unwrap(), "example.com");
        assert_eq!(normalize_domain("a-b.c1.io").unwrap(), "a-b.c1.io");
        assert_eq!(normalize_domain("München.de").unwrap(), "xn--mnchen-3ya.de");
    }

    #[test]
    fn normalize_domain_rejects_non_hostnames() {
        for bad in [
            "",
            "localhost",
            "someone@example.com",
            "not a domain",
            "-a.com",
            "a-.com",
            "a..com",
            "1.2.3.4",
            "exa_mple.com",
        ] {
            assert!(
                matches!(normalize_domain(bad), Err(CourierError::InvalidDomain(_))),
                "{bad:?}"
            );
        }
        assert!(normalize_domain(&format!("{}.com", "a".repeat(64))).is_err());
    }

    #[sqlx::test(migrations = "../db/migrations")]
    async fn send_test_email_records_it_without_ses(pool: PgPool) {
        let project = create_project(&pool, "demo").await;
        let Some(courier) = courier(pool).await else {
            return;
        };
        let name = domain();
        let created = courier
            .create_sender_identity(DomainOwner::Project(project), &name)
            .await
            .unwrap();
        let from = format!("noreply@{name}");
        let to = [String::from("someone@example.org")];

        // The identity is unverified, so this only succeeds if SES is skipped.
        let sent = courier
            .send_email(project, &mail(&from, &to), SendMode::Test)
            .await
            .unwrap();

        assert_eq!(sent.status, EmailStatus::Sent);
        assert_eq!(sent.mode, SendMode::Test);
        assert_eq!(sent.project_id, project.0);
        assert_eq!(sent.domain_id, Some(created.id));
        assert_eq!(sent.from_address, from);
        assert_eq!(sent.to_addresses, to);
        assert!(sent.provider_message_id.is_some());
        assert!(sent.error.is_none());

        let fetched = courier.get_email(EmailId(sent.id)).await.unwrap();
        assert_eq!(fetched.id, sent.id);

        let listed = courier.list_email(project, 10, None).await.unwrap();
        assert_eq!(listed.iter().map(|e| e.id).collect::<Vec<_>>(), [sent.id]);

        let listed = courier
            .list_email_from_domain(&name, 10, None)
            .await
            .unwrap();
        assert_eq!(listed.iter().map(|e| e.id).collect::<Vec<_>>(), [sent.id]);
    }

    #[sqlx::test(migrations = "../db/migrations")]
    async fn send_live_email_from_unverified_domain_records_failure(pool: PgPool) {
        let project = create_project(&pool, "demo").await;
        let Some(courier) = courier(pool).await else {
            return;
        };
        let name = domain();
        courier
            .create_sender_identity(DomainOwner::Project(project), &name)
            .await
            .unwrap();
        let from = format!("noreply@{name}");
        let to = [String::from("someone@example.org")];

        let sent = courier
            .send_email(project, &mail(&from, &to), SendMode::Live)
            .await
            .unwrap();

        assert_eq!(sent.status, EmailStatus::Failed);
        assert_eq!(sent.mode, SendMode::Live);
        assert!(sent.provider_message_id.is_none());
        assert!(sent.error.is_some());
    }

    #[sqlx::test(migrations = "../db/migrations")]
    async fn send_email_from_unknown_domain_is_not_found(pool: PgPool) {
        let project = create_project(&pool, "demo").await;
        let Some(courier) = courier(pool).await else {
            return;
        };
        let from = format!("noreply@{}", domain());
        let to = [String::from("someone@example.org")];

        let err = courier
            .send_email(project, &mail(&from, &to), SendMode::Test)
            .await
            .unwrap_err();

        assert!(matches!(err, CourierError::NotFound), "{err:?}");
        assert!(
            courier
                .list_email(project, 10, None)
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[sqlx::test(migrations = "../db/migrations")]
    async fn send_email_from_invalid_address_is_rejected(pool: PgPool) {
        let project = create_project(&pool, "demo").await;
        let Some(courier) = courier(pool).await else {
            return;
        };
        let to = [String::from("someone@example.org")];

        let err = courier
            .send_email(project, &mail("not-an-address", &to), SendMode::Test)
            .await
            .unwrap_err();

        assert!(matches!(err, CourierError::InvalidAddress(_)), "{err:?}");
    }

    #[sqlx::test(migrations = "../db/migrations")]
    async fn get_unknown_email_is_not_found(pool: PgPool) {
        let Some(courier) = courier(pool).await else {
            return;
        };

        let err = courier
            .get_email(EmailId(Uuid::new_v4()))
            .await
            .unwrap_err();

        assert!(matches!(err, CourierError::NotFound), "{err:?}");
    }
}

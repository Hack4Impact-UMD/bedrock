use std::str::FromStr;

use email_address::EmailAddress;
use identity::ProjectId;

use crate::{
    courier::CourierError::NotFound,
    domains::{Domain, DomainCheck, DomainId, DomainOwner, DomainRepo},
    mail::{Email, EmailId, EmailRepo, EmailStatus, NewEmail, SendMode},
    mail_provider::{MailProvider, MessageId, OutgoingEmail, ProviderError, SesMailProvider},
};

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
    #[error(transparent)]
    InvalidAddress(#[from] email_address::Error),
    #[error(transparent)]
    Provider(#[from] ProviderError),
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

fn mock_message() -> MessageId {
    MessageId(uuid::Uuid::new_v4().to_string())
}

impl<D: DomainRepo, M: EmailRepo> Courier<D, M> {
    pub async fn create_sender_identity(
        &self,
        owner: DomainOwner,
        domain: &str,
    ) -> Result<Domain, CourierError> {
        let records = self.ses_provider.create_identity(domain).await?;
        Ok(self.domains.create(owner, domain, &records).await?)
    }

    pub async fn get_sender_identity(
        self,
        project_id: ProjectId,
        domain: &str,
    ) -> Result<Domain, CourierError> {
        Ok(self
            .domains
            .get_by_name(domain)
            .await?
            .ok_or(CourierError::NotFound)?)
    }

    pub async fn get_sender_identities_for_project(
        &self,
        project: ProjectId,
    ) -> Result<Vec<Domain>, CourierError> {
        Ok(self.domains.list(project).await?)
    }

    pub async fn check_domain_status(&self, domain: &str) -> Result<DomainCheck, CourierError> {
        Ok(self.ses_provider.check_identity(domain).await?)
    }

    pub async fn delete_sender_identity(
        &self,
        owner: DomainOwner,
        domain: &str,
    ) -> Result<bool, CourierError> {
        Ok(self.domains.delete_by_name(owner, domain).await?)
    }

    pub async fn get_email(&self, id: EmailId) -> Result<Email, CourierError> {
        Ok(self.email.get(id).await?.ok_or(CourierError::NotFound)?)
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

        let mail_id = match mode {
            SendMode::Live => self.ses_provider.send_email(mail).await,
            SendMode::Test => Ok(mock_message()),
        };

        let (id, error) = match mail_id {
            Ok(id) => (Some(id.0), None),
            Err(e) => (None, Some(e.to_string())),
        };

        let mail = NewEmail {
            domain: domain.into(),
            mode: mode,
            from_address: mail.from,
            to_addresses: mail.to,
            cc_addresses: mail.cc,
            bcc_addresses: mail.bcc,
            subject: mail.subject,
            body: match mail.body {
                crate::mail_provider::EmailBody::Text(t) => t,
                crate::mail_provider::EmailBody::Html(h) => h,
                crate::mail_provider::EmailBody::Both { html: _, text } => text,
            },
            provider_message_id: id.as_deref(),
            error: error.as_deref(),
        };

        Ok(self.email.create(project_id, &mail).await?)
    }
}

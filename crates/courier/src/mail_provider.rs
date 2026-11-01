use crate::domains::{DnsRecord, DomainCheck, DomainStatus};
use aws_sdk_sesv2::error::{DisplayErrorContext, ProvideErrorMetadata};
use aws_sdk_sesv2::operation::create_email_identity::CreateEmailIdentityError;
use aws_sdk_sesv2::operation::delete_email_identity::DeleteEmailIdentityError;
use aws_sdk_sesv2::operation::get_email_identity::GetEmailIdentityError;
use aws_sdk_sesv2::operation::send_email::SendEmailError;
use aws_sdk_sesv2::types::{
    BehaviorOnMxFailure, Body, Content, Destination, EmailContent, MailFromDomainStatus, Message,
    VerificationStatus,
};
use uuid::{self, Uuid};

const MAIL_FROM_SUBDOMAIN: &str = "bounce";

pub struct OutgoingEmail<'a> {
    pub from: &'a str,
    pub to: &'a [String],
    pub cc: &'a [String],
    pub bcc: &'a [String],
    pub subject: &'a str,
    pub body: EmailBody<'a>,
}

#[derive(Clone, Copy, Debug)]
pub enum EmailBody<'a> {
    Html(&'a str),
    Text(&'a str),
    Both { html: &'a str, text: &'a str },
}

impl<'a> EmailBody<'a> {
    pub fn html(&self) -> Option<&'a str> {
        match *self {
            Self::Html(html) | Self::Both { html, .. } => Some(html),
            Self::Text(_) => None,
        }
    }

    pub fn text(&self) -> Option<&'a str> {
        match *self {
            Self::Text(text) | Self::Both { text, .. } => Some(text),
            Self::Html(_) => None,
        }
    }
}

pub struct MessageId(pub String);

#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("rejected by provider: {0}")]
    Rejected(String),
    #[error("identity already exists")]
    AlreadyExists,
    #[error("identity not found")]
    NotFound,
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

pub trait MailProvider {
    fn send_email(
        &self,
        mail: &OutgoingEmail<'_>,
    ) -> impl Future<Output = Result<MessageId, ProviderError>> + Send;

    fn create_identity(
        &self,
        domain: &str,
    ) -> impl Future<Output = Result<Vec<DnsRecord>, ProviderError>> + Send;

    fn delete_identity(
        &self,
        domain: &str,
    ) -> impl Future<Output = Result<(), ProviderError>> + Send;

    fn check_identity(
        &self,
        domain: &str,
    ) -> impl Future<Output = Result<DomainCheck, ProviderError>> + Send;
}

#[derive(Default)]
pub struct MockMailProvider {}

impl MailProvider for MockMailProvider {
    async fn send_email(&self, _: &OutgoingEmail<'_>) -> Result<MessageId, ProviderError> {
        Ok(MessageId(Uuid::new_v4().to_string()))
    }

    async fn create_identity(&self, domain: &str) -> Result<Vec<DnsRecord>, ProviderError> {
        Ok(vec![DnsRecord {
            record_type: String::from("test"),
            name: String::from("test"),
            value: domain.to_string(),
        }])
    }

    async fn delete_identity(&self, _: &str) -> Result<(), ProviderError> {
        Ok(())
    }

    async fn check_identity(&self, _: &str) -> Result<DomainCheck, ProviderError> {
        Ok(DomainCheck {
            status: DomainStatus::Verified,
            mail_from_status: DomainStatus::Verified,
        })
    }
}

pub struct SesMailProvider {
    client: aws_sdk_sesv2::Client,
}

impl SesMailProvider {
    pub fn new(client: aws_sdk_sesv2::Client) -> Self {
        SesMailProvider { client }
    }
}

impl MailProvider for SesMailProvider {
    async fn send_email(&self, mail: &OutgoingEmail<'_>) -> Result<MessageId, ProviderError> {
        let content = |data: &str| {
            Content::builder()
                .data(data)
                .charset("UTF-8")
                .build()
                .map_err(anyhow::Error::from)
        };

        let mut body = Body::builder();
        if let Some(html) = mail.body.html() {
            body = body.html(content(html)?);
        }
        if let Some(text) = mail.body.text() {
            body = body.text(content(text)?);
        }

        let message = Message::builder()
            .subject(content(mail.subject)?)
            .body(body.build())
            .build();

        let result = self
            .client
            .send_email()
            .from_email_address(mail.from)
            .destination(
                Destination::builder()
                    .set_to_addresses(Some(mail.to.to_vec()))
                    .set_cc_addresses(Some(mail.cc.to_vec()))
                    .set_bcc_addresses(Some(mail.bcc.to_vec()))
                    .build(),
            )
            .content(EmailContent::builder().simple(message).build())
            .send()
            .await
            .map_err(|err| match err.as_service_error() {
                Some(
                    SendEmailError::MessageRejected(_)
                    | SendEmailError::MailFromDomainNotVerifiedException(_)
                    | SendEmailError::BadRequestException(_),
                ) => ProviderError::Rejected(err.message().unwrap_or("rejected").to_string()),
                _ => other(err),
            })?;

        let id = result
            .message_id
            .ok_or_else(|| anyhow::anyhow!("SES returned no message id"))?;

        Ok(MessageId(id))
    }

    async fn create_identity(&self, domain: &str) -> Result<Vec<DnsRecord>, ProviderError> {
        let result = self
            .client
            .create_email_identity()
            .email_identity(domain)
            .send()
            .await
            .map_err(|err| match err.as_service_error() {
                Some(CreateEmailIdentityError::AlreadyExistsException(_)) => {
                    ProviderError::AlreadyExists
                }
                Some(CreateEmailIdentityError::BadRequestException(_)) => {
                    ProviderError::Rejected(err.message().unwrap_or("rejected").to_string())
                }
                _ => other(err),
            })?;

        let dkim = result
            .dkim_attributes()
            .filter(|dkim| !dkim.tokens().is_empty())
            .ok_or_else(|| anyhow::anyhow!("SES returned no DKIM tokens for {domain}"))?;
        let zone = dkim.signing_hosted_zone().unwrap_or("dkim.amazonses.com");

        self.client
            .put_email_identity_mail_from_attributes()
            .email_identity(domain)
            .mail_from_domain(mail_from_domain(domain))
            .behavior_on_mx_failure(BehaviorOnMxFailure::UseDefaultValue)
            .send()
            .await
            .map_err(other)?;

        let region = self
            .client
            .config()
            .region()
            .ok_or_else(|| anyhow::anyhow!("SES client has no region configured"))?;

        Ok(dns_records(domain, dkim.tokens(), zone, region.as_ref()))
    }

    async fn delete_identity(&self, domain: &str) -> Result<(), ProviderError> {
        self.client
            .delete_email_identity()
            .email_identity(domain)
            .send()
            .await
            .map_err(|err| match err.as_service_error() {
                Some(DeleteEmailIdentityError::NotFoundException(_)) => ProviderError::NotFound,
                _ => other(err),
            })?;

        Ok(())
    }

    async fn check_identity(&self, domain: &str) -> Result<DomainCheck, ProviderError> {
        let result = self
            .client
            .get_email_identity()
            .email_identity(domain)
            .send()
            .await
            .map_err(|err| match err.as_service_error() {
                Some(GetEmailIdentityError::NotFoundException(_)) => ProviderError::NotFound,
                _ => other(err),
            })?;

        let status = match result.verification_status() {
            Some(VerificationStatus::Success) => DomainStatus::Verified,
            Some(VerificationStatus::Failed) => DomainStatus::Failed,
            Some(VerificationStatus::TemporaryFailure) => DomainStatus::TemporaryFailure,
            Some(VerificationStatus::Pending | VerificationStatus::NotStarted) | None => {
                DomainStatus::Pending
            }
            Some(unknown) => {
                return Err(anyhow::anyhow!(
                    "unknown SES verification status for {domain}: {}",
                    unknown.as_str()
                )
                .into());
            }
        };

        let mail_from_status = match result
            .mail_from_attributes()
            .map(|attrs| attrs.mail_from_domain_status())
        {
            Some(MailFromDomainStatus::Success) => DomainStatus::Verified,
            Some(MailFromDomainStatus::Failed) => DomainStatus::Failed,
            Some(MailFromDomainStatus::TemporaryFailure) => DomainStatus::TemporaryFailure,
            Some(MailFromDomainStatus::Pending) | None => DomainStatus::Pending,
            Some(unknown) => {
                return Err(anyhow::anyhow!(
                    "unknown SES MAIL FROM status for {domain}: {}",
                    unknown.as_str()
                )
                .into());
            }
        };

        Ok(DomainCheck {
            status,
            mail_from_status,
        })
    }
}

fn mail_from_domain(domain: &str) -> String {
    format!("{MAIL_FROM_SUBDOMAIN}.{domain}")
}

fn dns_records(
    domain: &str,
    dkim_tokens: &[String],
    dkim_zone: &str,
    region: &str,
) -> Vec<DnsRecord> {
    let record = |record_type: &str, name: String, value: String| DnsRecord {
        record_type: record_type.to_string(),
        name,
        value,
    };

    let mut records: Vec<DnsRecord> = dkim_tokens
        .iter()
        .map(|token| {
            record(
                "CNAME",
                format!("{token}._domainkey.{domain}"),
                format!("{token}.{dkim_zone}"),
            )
        })
        .collect();

    let mail_from = mail_from_domain(domain);
    records.extend([
        record(
            "MX",
            mail_from.clone(),
            format!("10 feedback-smtp.{region}.amazonses.com"),
        ),
        record(
            "TXT",
            mail_from,
            String::from("v=spf1 include:amazonses.com ~all"),
        ),
        record(
            "TXT",
            format!("_dmarc.{domain}"),
            String::from("v=DMARC1; p=none;"),
        ),
    ]);

    records
}

fn other(err: impl std::error::Error) -> ProviderError {
    ProviderError::Other(anyhow::anyhow!("{}", DisplayErrorContext(&err)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dns_records_cover_dkim_mail_from_and_dmarc() {
        let tokens = vec![String::from("abc"), String::from("def")];
        let records = dns_records("example.com", &tokens, "dkim.amazonses.com", "us-east-1");

        let actual: Vec<(&str, &str, &str)> = records
            .iter()
            .map(|r| (r.record_type.as_str(), r.name.as_str(), r.value.as_str()))
            .collect();
        assert_eq!(
            actual,
            [
                (
                    "CNAME",
                    "abc._domainkey.example.com",
                    "abc.dkim.amazonses.com"
                ),
                (
                    "CNAME",
                    "def._domainkey.example.com",
                    "def.dkim.amazonses.com"
                ),
                (
                    "MX",
                    "bounce.example.com",
                    "10 feedback-smtp.us-east-1.amazonses.com"
                ),
                (
                    "TXT",
                    "bounce.example.com",
                    "v=spf1 include:amazonses.com ~all"
                ),
                ("TXT", "_dmarc.example.com", "v=DMARC1; p=none;"),
            ]
        );
    }
}

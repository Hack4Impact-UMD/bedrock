use identity::ProjectId;

use crate::{
    domains::{Domain, DomainRepo},
    mail::{Email, EmailRepo, SendMode},
    mail_provider::{MockMailProvider, OutgoingEmail, ProviderError, SesMailProvider},
};

pub struct Courier<D: DomainRepo, M: EmailRepo> {
    domains: D,
    email: M,
    ses_provider: SesMailProvider,
    mock_provider: MockMailProvider,
}

#[derive(Debug, thiserror::Error)]
pub enum CourierError {
    #[error("domain not found")]
    NotFound,
    #[error("domain taken")]
    Taken,
    #[error(transparent)]
    Provider(#[from] ProviderError),
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

impl<D: DomainRepo, M: EmailRepo> Courier<D, M> {
    async fn create_sender_identity(domain: &str) -> Result<Domain, CourierError> {
        todo!()
    }

    async fn get_sender_identity(domain: &str) -> Result<Domain, CourierError> {
        todo!()
    }

    async fn get_sender_identities_for_project(
        project: &ProjectId,
    ) -> Result<Vec<Domain>, CourierError> {
        todo!()
    }

    async fn delete_sender_identity(domain: &str) -> Result<(), CourierError> {
        todo!()
    }

    async fn send_email(mail: &OutgoingEmail<'_>, mode: SendMode) -> Result<Email, CourierError> {
        todo!()
    }
}

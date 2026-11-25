use crate::courier::Courier;

mod courier;
mod domains;
mod mail;
mod mail_provider;

pub use courier::CourierError;
pub use domains::{Domain, DomainCheck, DomainId, DomainOwner, DomainStatus};
pub use mail::SendMode;
pub use mail::{Email, EmailId};
pub use mail_provider::EmailBody;
pub use mail_provider::OutgoingEmail;

pub type PgCourierService = Courier<domains::PostgresDomainRepo, mail::PostgresEmailRepo>;

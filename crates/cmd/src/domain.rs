use std::str::FromStr;

use clap::Subcommand;
use courier::PgCourierService;
use uuid::Uuid;

use crate::output::{Output, confirm};
use anyhow::Result;

#[derive(Clone, Debug)]
pub enum DomainOwner {
    Shared,
    Project(Uuid),
}

impl From<DomainOwner> for courier::DomainOwner {
    fn from(value: DomainOwner) -> Self {
        match value {
            DomainOwner::Project(project_id) => {
                courier::DomainOwner::Project(identity::ProjectId(project_id))
            }
            DomainOwner::Shared => courier::DomainOwner::Shared,
        }
    }
}

impl FromStr for DomainOwner {
    type Err = uuid::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s.trim() {
            "shared" => DomainOwner::Shared,
            id => DomainOwner::Project(Uuid::parse_str(id)?),
        })
    }
}

#[derive(Subcommand)]
pub enum DomainCmd {
    /// Create a new domain
    Create {
        name: String,
        #[arg(short, long, value_parser = DomainOwner::from_str)]
        owner: DomainOwner,
    },
    /// Delete a new domain
    Delete {
        name: String,
        #[arg(short, long, value_parser = DomainOwner::from_str)]
        owner: DomainOwner,
    },
    /// Check a domain's status
    Check { name: String },
    /// Get domain details
    Get { name: String },
}

pub async fn run(cmd: DomainCmd, courier: &PgCourierService, out: &Output) -> Result<()> {
    match cmd {
        DomainCmd::Create { name, owner } => {
            let domain = courier
                .create_sender_identity(owner.into(), name.as_str())
                .await?;
            out.domain(&domain)
        }
        DomainCmd::Delete { name, owner } => {
            if let Ok(true) = confirm(format!("Are you sure you want to delete {name}?").as_str()) {
                if courier
                    .delete_sender_identity(owner.into(), name.as_str())
                    .await?
                {
                    println!("deleted {}", name);
                } else {
                    println!("failed to delete {}", name);
                }
            }
            Ok(())
        }
        DomainCmd::Check { name } => {
            let resp = courier.check_domain_status(name.as_str()).await?;
            out.domain_check(&resp)
        }
        DomainCmd::Get { name } => {
            let domain = courier.get_sender_identity(name.as_str()).await?;
            out.domain(&domain)
        }
    }
}

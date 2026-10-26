use anyhow::{Result, bail};
use clap::Subcommand;
use identity::{ApiKeyId, PgIdentityService, ProjectId};
use uuid::Uuid;

use crate::{
    output::{Output, confirm},
    project::resolve_project,
};

#[derive(Subcommand)]
pub enum KeyCmd {
    /// Issue a new API key for a project.
    Create {
        /// Project slug or ID.
        project: String,
        name: String,
    },
    /// List a project's API keys.
    List {
        /// Project slug or ID.
        project: String,
    },
    /// Show an API key.
    Get {
        /// Project slug or ID.
        project: String,
        id: Uuid,
    },
    /// Revoke an API key. The key stays listed but can no longer authenticate.
    Revoke {
        /// Project slug or ID.
        project: String,
        id: Uuid,
    },
    /// Delete an API key. Emails sent with it keep their rows but lose the reference.
    Delete {
        /// Project slug or ID.
        project: String,
        id: Uuid,
        /// Skip the confirmation prompt.
        #[arg(long)]
        yes: bool,
    },
}

pub async fn run(cmd: KeyCmd, identity: &PgIdentityService, out: &Output) -> Result<()> {
    match cmd {
        KeyCmd::Create { project, name } => {
            let project = project_id(identity, &project).await?;
            out.new_key(&identity.create_api_key(project, &name).await?)
        }
        KeyCmd::List { project } => {
            let project = project_id(identity, &project).await?;
            out.keys(&identity.list_api_keys(project).await?)
        }
        KeyCmd::Get { project, id } => {
            let project = project_id(identity, &project).await?;
            out.key(&identity.get_api_key(project, ApiKeyId(id)).await?)
        }
        KeyCmd::Revoke { project, id } => {
            let project = project_id(identity, &project).await?;
            out.key(&identity.revoke_api_key(project, ApiKeyId(id)).await?)
        }
        KeyCmd::Delete { project, id, yes } => {
            let project = project_id(identity, &project).await?;
            let key = identity.get_api_key(project, ApiKeyId(id)).await?;
            let question = format!("Delete API key '{}' ({})?", key.name, key.prefix);
            if !yes && !confirm(&question)? {
                bail!("aborted");
            }

            identity.delete_api_key(project, ApiKeyId(id)).await?;
            out.done(&format!("Deleted API key '{}'.", key.name));
            Ok(())
        }
    }
}

async fn project_id(identity: &PgIdentityService, project: &str) -> Result<ProjectId> {
    Ok(ProjectId(resolve_project(identity, project).await?.id))
}

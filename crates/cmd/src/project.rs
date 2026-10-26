use anyhow::{Result, bail};
use clap::Subcommand;
use identity::{IdentityError, PgIdentityService, Project, ProjectId};
use uuid::Uuid;

use crate::output::{Output, confirm};

#[derive(Subcommand)]
pub enum ProjectCmd {
    /// Create a project and its first API key.
    Create { slug: String, name: String },
    /// List all projects.
    List,
    /// Show a project.
    Get {
        /// Project slug or ID.
        project: String,
    },
    /// Change a project's display name.
    Rename {
        /// Project slug or ID.
        project: String,
        name: String,
    },
    /// Delete a project along with its keys and courier rows.
    Delete {
        /// Project slug or ID.
        project: String,
        /// Skip the confirmation prompt.
        #[arg(long)]
        yes: bool,
    },
}

pub async fn run(cmd: ProjectCmd, identity: &PgIdentityService, out: &Output) -> Result<()> {
    match cmd {
        ProjectCmd::Create { slug, name } => {
            let (project, key) = identity.create_project(&slug, &name).await?;
            out.created_project(&project, &key)
        }
        ProjectCmd::List => out.projects(&identity.list_projects().await?),
        ProjectCmd::Get { project } => out.project(&resolve_project(identity, &project).await?),
        ProjectCmd::Rename { project, name } => {
            let project = resolve_project(identity, &project).await?;
            let renamed = identity
                .rename_project(ProjectId(project.id), &name)
                .await?;
            out.project(&renamed)
        }
        ProjectCmd::Delete { project, yes } => {
            let project = resolve_project(identity, &project).await?;
            let question = format!(
                "Delete project '{}' and all of its keys and courier rows?",
                project.slug
            );
            if !yes && !confirm(&question)? {
                bail!("aborted");
            }

            identity.delete_project(ProjectId(project.id)).await?;
            out.done(&format!("Deleted project '{}'.", project.slug));
            Ok(())
        }
    }
}

/// Looks a project up by ID if `project` parses as a UUID, otherwise by slug.
pub async fn resolve_project(
    identity: &PgIdentityService,
    project: &str,
) -> Result<Project, IdentityError> {
    match Uuid::parse_str(project) {
        Ok(id) => identity.get_project(ProjectId(id)).await,
        Err(_) => identity.get_project_by_slug(project).await,
    }
}

#[cfg(test)]
mod tests {
    use db::PgPool;

    use super::*;

    #[sqlx::test(migrations = "../db/migrations")]
    async fn resolves_by_slug_or_id(pool: PgPool) {
        let identity = PgIdentityService::postgres(pool);
        let (project, _) = identity.create_project("demo", "Demo").await.unwrap();

        for input in ["demo".to_owned(), project.id.to_string()] {
            assert_eq!(
                resolve_project(&identity, &input).await.unwrap().id,
                project.id
            );
        }
        for input in ["missing".to_owned(), Uuid::new_v4().to_string()] {
            assert!(matches!(
                resolve_project(&identity, &input).await,
                Err(IdentityError::ProjectNotFound)
            ));
        }
    }
}

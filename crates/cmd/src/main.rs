use anyhow::Context;
use aws_config::BehaviorVersion;
use clap::{Parser, Subcommand};
use courier::PgCourierService;
use identity::PgIdentityService;

use crate::{domain::DomainCmd, key::KeyCmd, output::Output, project::ProjectCmd};

mod domain;
mod key;
mod output;
mod project;

/// Operator CLI for bedrock.
#[derive(Parser)]
#[command(name = "bedrock", version)]
struct Cli {
    #[arg(long, env = "DATABASE_URL", hide_env_values = true)]
    database_url: String,
    /// Print results as JSON.
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Manage projects.
    #[command(subcommand)]
    Project(ProjectCmd),
    /// Manage a project's API keys.
    #[command(subcommand)]
    Key(KeyCmd),
    /// Manage domains.
    #[command(subcommand)]
    Domain(DomainCmd),
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    // Only the working directory's .env, without searching parents. Variables already set take precedence.
    if let Err(err) = dotenvy::from_path(".env")
        && !err.not_found()
    {
        return Err(err).context("failed to load .env");
    }

    let cli = Cli::parse();
    let pool = db::connect(&cli.database_url)
        .await
        .context("failed to connect to the database")?;

    let config = aws_config::load_defaults(BehaviorVersion::v2026_01_12()).await;
    let ses_client = aws_sdk_sesv2::client::Client::new(&config);

    let identity = PgIdentityService::postgres(pool.clone());
    let courier = PgCourierService::postgres(pool, ses_client);
    let out = Output::new(cli.json);

    match cli.command {
        Command::Project(cmd) => project::run(cmd, &identity, &out).await,
        Command::Key(cmd) => key::run(cmd, &identity, &out).await,
        Command::Domain(cmd) => domain::run(cmd, &courier, &out).await,
    }
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory;

    use super::*;

    #[test]
    fn cli_is_valid() {
        Cli::command().debug_assert();
    }
}

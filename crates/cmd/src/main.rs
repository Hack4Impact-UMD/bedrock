use anyhow::Context;
use clap::{Parser, Subcommand};
use identity::PgIdentityService;

use crate::{key::KeyCmd, output::Output, project::ProjectCmd};

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
    let identity = PgIdentityService::postgres(pool);
    let out = Output::new(cli.json);

    match cli.command {
        Command::Project(cmd) => project::run(cmd, &identity, &out).await,
        Command::Key(cmd) => key::run(cmd, &identity, &out).await,
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

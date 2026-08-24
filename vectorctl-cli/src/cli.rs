use clap::{Parser, Subcommand};
use std::io::IsTerminal;
use std::path::PathBuf;
use thiserror::Error;

use crate::commands::{MigrateSubcommands, run_migrate_command};

#[derive(Debug, Error)]
pub enum CliError {
    #[error(transparent)]
    Qdrant(#[from] qdrant_client::QdrantError),
    #[error(transparent)]
    Migrate(#[from] crate::commands::MigrateError),
    #[error("custom: {0}")]
    Custom(String),
}

#[derive(Subcommand, PartialEq, Eq, Debug)]
pub enum Commands {
    #[command(about = "Migration related commands")]
    Migrate {
        #[arg(
            global = true,
            short = 'd',
            long,
            env = "MIGRATION_DIR",
            default_value = "./migration"
        )]
        migration_dir: PathBuf,
        #[arg(
            global = true,
            short = 'u',
            long,
            help = "database url",
            env = "DATABASE_URL",
            default_value = "http://localhost:6334"
        )]
        database_url: url::Url,
        #[arg(
            global = true,
            short = 'k',
            long,
            help = "database api key",
            env = "DATABASE_API_KEY"
        )]
        api_key: Option<String>,
        #[command(subcommand)]
        command: Option<MigrateSubcommands>,
    },
}

#[derive(Parser, Debug)]
#[command(version, author)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

fn init_tracing() {
    let ansi = std::io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none();
    tracing_subscriber::fmt()
        .with_ansi(ansi)
        .with_target(false)
        .without_time()
        .init();
}

pub async fn main() -> Result<(), CliError> {
    init_tracing();

    let cli = Cli::parse();

    match cli.command {
        Commands::Migrate {
            api_key,
            command,
            database_url,
            migration_dir,
        } => run_migrate_command(command, migration_dir, database_url, api_key).await?,
    }

    Ok(())
}

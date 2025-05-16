use clap::{Parser, Subcommand};
use std::path::PathBuf;
use thiserror::Error;

use crate::commands::{MigrateSubcommands, run_migrate_command};

#[derive(Debug, Error)]
pub enum CliError {
    #[error(transparent)]
    Qdrant(#[from] qdrant_client::QdrantError),
    #[error(transparent)]
    Migrate(#[from] crate::commands::MigrateCommandError),
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
            short = 'u',
            long,
            help = "Qdrant database url",
            env = "QDRANT_URL",
            default_value = "http://localhost:6334"
        )]
        qdrant_url: url::Url,

        #[arg(
            short = 'k',
            long,
            help = "Qdrant api key",
            env = "QDRANT__SERVICE__API_KEY"
        )]
        qdrant_api_key: Option<String>,

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

pub async fn main() -> Result<(), CliError> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Migrate {
            migration_dir,
            command,
            qdrant_url,
            qdrant_api_key,
        } => {
            run_migrate_command(
                command,
                migration_dir.to_string_lossy().to_string().as_str(),
                &qdrant_url,
                qdrant_api_key,
            )
            .await?
        }
    }

    Ok(())
}

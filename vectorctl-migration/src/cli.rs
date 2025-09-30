use std::path::PathBuf;

use clap::Parser;
use thiserror::Error;
use vectorctl_backend::generic::VectorBackendError;
use vectorctl_cli::commands::{MigrateError, MigrateSubcommands, create_new_revision, init};

use crate::migrator::{MigrationError, MigratorTrait};

#[derive(Error, Debug)]
pub enum CliError {
    #[error(transparent)]
    Migrate(#[from] MigrationError),
    #[error(transparent)]
    Command(#[from] MigrateError),
    #[error(transparent)]
    Context(#[from] crate::context::ContextError),
    #[error(transparent)]
    Backend(#[from] VectorBackendError),
}

#[derive(Parser)]
#[command(version)]
pub struct Cli {
    #[arg(
        global = true,
        short = 'u',
        long,
        env = "DATABASE_URL",
        help = "vector database URL",
        default_value = "http://localhost:6334"
    )]
    pub database_url: String,

    #[arg(
        global = true,
        short = 'd',
        long,
        env = "MIGRATION_DIR",
        default_value = "./"
    )]
    migration_dir: PathBuf,

    #[arg(
        global = true,
        short = 'k',
        long,
        help = "database api key",
        env = "DATABASE_API_KEY"
    )]
    pub api_key: Option<String>,

    #[cfg(feature = "sea-backend")]
    #[arg(
        global = true,
        long,
        env = "SQL_DATABASE_URL",
        help = "sql database URL"
    )]
    pub sql_database_url: Option<String>,

    #[command(subcommand)]
    pub command: Option<MigrateSubcommands>,
}

pub async fn run_migrate<M>(_: M, context: &crate::context::Context) -> Result<(), CliError>
where
    M: MigratorTrait,
{
    let cli = Cli::parse();

    let migration_dir = cli.migration_dir;

    match cli.command {
        Some(MigrateSubcommands::Init {
            package_name,
            rust_edition,
        }) => {
            init(
                package_name.as_deref(),
                rust_edition.as_deref(),
                migration_dir,
            )
            .await?
        }
        Some(MigrateSubcommands::Generate { name, message }) => {
            create_new_revision(
                migration_dir,
                name.as_ref(),
                M::latest_revision()?.revision().revision,
                message.as_deref(),
            )
            .await?
        }
        Some(MigrateSubcommands::Up { to }) => M::up(context, to).await?,
        Some(MigrateSubcommands::Down { to }) => M::down(context, to).await?,
        Some(MigrateSubcommands::Refresh) => M::refresh(context).await?,
        Some(MigrateSubcommands::Reset) => M::reset(context).await?,
        Some(MigrateSubcommands::Status) => M::status(context).await?,
        None => M::up(context, None).await?,
    }
    Ok(())
}

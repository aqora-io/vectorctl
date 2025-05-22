use clap::Parser;
use cli::commands::{MigrateError, MigrateSubcommands, create_new_revision, init};
use qdrant_client::Qdrant;
use thiserror::Error;

use crate::migrator::{MigrationError, MigratorTrait};

#[derive(Error, Debug)]
pub enum CliError {
    #[error(transparent)]
    Migrate(#[from] MigrationError),
    #[error(transparent)]
    Command(#[from] MigrateError),
    #[error(transparent)]
    Qdrant(#[from] qdrant_client::QdrantError),
    #[error(transparent)]
    Context(#[from] crate::context::ContextError),
}

const MIGRATION_DIR: &str = "./";

#[derive(Parser)]
#[command(version)]
pub struct Cli {
    #[arg(
        global = true,
        short = 'u',
        long,
        env = "DATABASE_URL",
        help = "Database URL",
        default_value = "http://localhost:6334"
    )]
    database_url: Option<String>,

    #[arg(short = 'k', long, help = "database api key", env = "DATABASE_API_KEY")]
    api_key: Option<String>,

    #[command(subcommand)]
    command: Option<MigrateSubcommands>,
}

pub async fn run_migrate<M>(_: M) -> Result<(), CliError>
where
    M: MigratorTrait,
{
    let cli = Cli::parse();

    let database_url = cli
        .database_url
        .expect("Environment variable 'DATABASE_URL' not set");

    let qdrant = Qdrant::from_url(database_url.as_str())
        .api_key(cli.api_key)
        .build()?;

    let context = crate::context::Context::new(&qdrant);

    match cli.command {
        Some(MigrateSubcommands::Init {
            package_name,
            rust_edition,
        }) => {
            init(
                package_name.as_deref(),
                rust_edition.as_deref(),
                MIGRATION_DIR,
            )
            .await?
        }
        Some(MigrateSubcommands::Generate {
            migration_name,
            message,
        }) => {
            create_new_revision(
                MIGRATION_DIR,
                &migration_name,
                M::latest_revision()?.revision().revision,
                message.as_deref(),
            )
            .await?
        }
        Some(MigrateSubcommands::Up { to }) => M::up(&context, to).await?,
        Some(MigrateSubcommands::Down { to }) => M::down(&context, to).await?,
        Some(MigrateSubcommands::Status) => M::status(&context).await?,
        None => M::up(&context, None).await?,
    }
    Ok(())
}

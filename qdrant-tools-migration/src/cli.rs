use clap::Parser;
use qdrant_client::Qdrant;
use qdrant_tools_cli::commands::{
    MigrateCommandError, MigrateSubcommands, create_new_migration, init,
};
use thiserror::Error;

use crate::migrator::{MigrationError, MigratorTrait};

#[derive(Error, Debug)]
pub enum CliError {
    #[error(transparent)]
    Migrate(#[from] MigrationError),
    #[error(transparent)]
    Command(#[from] MigrateCommandError),
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
        short = 'q',
        long,
        env = "QDRANT_URL",
        help = "Qdrant URL"
    )]
    qdrant_url: Option<String>,

    #[command(subcommand)]
    command: Option<MigrateSubcommands>,
}

pub async fn run_migrate<M>(_: M) -> Result<(), CliError>
where
    M: MigratorTrait,
{
    let cli = Cli::parse();

    let url = cli
        .qdrant_url
        .expect("Environment variable 'QDRANT_URL' not set");

    let qdrant = Qdrant::from_url(url.as_str()).build()?;
    let context = crate::context::Context::new(&qdrant);

    match cli.command {
        Some(MigrateSubcommands::Init {
            package_name,
            rust_edition,
        }) => init(package_name, rust_edition, MIGRATION_DIR).await?,
        Some(MigrateSubcommands::Generate { migration_name }) => {
            create_new_migration(MIGRATION_DIR, &migration_name).await?
        }
        Some(MigrateSubcommands::Up) | None => M::up(&context).await?,
        Some(MigrateSubcommands::Down) => M::down(&context).await?,
        Some(MigrateSubcommands::Status) => M::status(&context).await?,
    }
    Ok(())
}

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

    #[arg(
        short = 't',
        long,
        help = "Database type",
        default_value = "sea_orm::DbConn"
    )]
    db_type: String,

    #[command(subcommand)]
    command: Option<MigrateSubcommands>,
}

pub async fn run_migrate<M>(
    _: M,
    db: &M::Db,
    command: Option<MigrateSubcommands>,
) -> Result<(), CliError>
where
    M: MigratorTrait,
{
    let cli = Cli::parse();

    let url = cli
        .qdrant_url
        .expect("Environment variable 'QDRANT_URL' not set");
    let db_type = cli.db_type;
    let qdrant = Qdrant::from_url(url.as_str()).build()?;

    match command {
        Some(MigrateSubcommands::Init) => init(db_type, MIGRATION_DIR).await?,
        Some(MigrateSubcommands::Generate { migration_name }) => {
            create_new_migration(db_type, MIGRATION_DIR, &migration_name).await?
        }
        Some(MigrateSubcommands::Up) | None => M::up(&qdrant, db).await?,
        Some(MigrateSubcommands::Down) => M::down(&qdrant, db).await?,
    }
    Ok(())
}

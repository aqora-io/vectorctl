use std::{any::Any, path::PathBuf};

use clap::Parser;
#[cfg(feature = "sea-backend")]
use sea_orm::{ConnectOptions, Database, DbConn, DbErr};
use thiserror::Error;
use vectorctl_backend::generic::{VectorBackendError, VectorTrait};
use vectorctl_cli::commands::{MigrateError, MigrateSubcommands, create_new_revision, init};

use crate::{
    context::Backend,
    migrator::{MigrationError, MigratorTrait},
};

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
    #[cfg(feature = "sea-backend")]
    #[error(transparent)]
    Db(#[from] DbErr),
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
    database_url: Option<String>,

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
    api_key: Option<String>,

    #[cfg(feature = "sea-backend")]
    #[arg(
        global = true,
        long,
        env = "SQL_DATABASE_URL",
        help = "sql database URL"
    )]
    sql_database_url: Option<String>,

    #[command(subcommand)]
    command: Option<MigrateSubcommands>,
}

pub async fn run_migrate<M>(
    _: M,
    resources: Option<Vec<Box<dyn Any + 'static + Send + Sync>>>,
) -> Result<(), CliError>
where
    M: MigratorTrait,
{
    let cli = Cli::parse();

    let migration_dir = cli.migration_dir;

    let database_url = cli
        .database_url
        .expect("Environment variable 'DATABASE_URL' not set");

    #[cfg(feature = "qdrant-backend")]
    let api_key = cli.api_key;

    #[cfg(not(feature = "qdrant-backend"))]
    let api_key = None;

    let mut context = crate::context::Context::new(Backend::new(&database_url, api_key)?);

    #[cfg(feature = "sea-backend")]
    if let Some(database_url) = cli.sql_database_url {
        let db_conn = {
            let connect_opts = ConnectOptions::from(database_url);
            Database::connect(connect_opts).await?
        };
        context.insert_resource::<DbConn>(db_conn);
    }

    if let Some(resources) = resources {
        context.insert_resources(resources);
    };

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
        Some(MigrateSubcommands::Up { to }) => M::up(&context, to).await?,
        Some(MigrateSubcommands::Down { to }) => M::down(&context, to).await?,
        Some(MigrateSubcommands::Refresh) => M::refresh(&context).await?,
        Some(MigrateSubcommands::Reset) => M::reset(&context).await?,
        Some(MigrateSubcommands::Status) => M::status(&context).await?,
        None => M::up(&context, None).await?,
    }
    Ok(())
}

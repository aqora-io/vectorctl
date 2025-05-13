use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};
use thiserror::Error;

fn parse_migration_name(raw: &str) -> Result<String, String> {
    if raw.contains('-') {
        Err(String::from("must not contain a hyphen (\"-\")"))
    } else {
        Ok(raw.trim().to_lowercase().replace(" ", "_"))
    }
}

#[derive(Debug, Error)]
pub enum CliError {
    #[error(transparent)]
    Migrate(#[from] crate::commands::MigrateCommandError),
}

#[derive(Subcommand, PartialEq, Eq, Debug)]
pub enum MigrateSubcommands {
    #[command(about = "Initialize migration directory")]
    Init,
    #[command(about = "Generate new migration")]
    Generate {
        #[arg(required = true, value_parser = parse_migration_name)]
        migration_name: String,
    },
}

async fn run_migrate_command(
    command: Option<MigrateSubcommands>,
    migration_dir: impl AsRef<Path>,
    db_type: String,
) -> Result<(), CliError> {
    match command {
        Some(MigrateSubcommands::Init) => crate::commands::init(db_type, migration_dir).await?,
        Some(MigrateSubcommands::Generate { migration_name }) => {
            crate::commands::create_new_migration(db_type, migration_dir, &migration_name).await?
        }
        None => println!("None"),
    }

    Ok(())
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
            short = 't',
            long,
            help = "Database type",
            default_value = "sea_orm::DbConn"
        )]
        db_type: String,

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
            db_type,
            command,
        } => run_migrate_command(command, migration_dir, db_type).await?,
    }

    Ok(())
}

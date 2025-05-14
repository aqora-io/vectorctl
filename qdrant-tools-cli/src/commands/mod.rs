mod migrate;

use clap::{Subcommand, command};
use std::path::Path;

pub use migrate::{MigrateCommandError, create_new_migration, init};

use crate::CliError;

fn parse_migration_name(raw: &str) -> Result<String, String> {
    if raw.contains('-') {
        Err(String::from("must not contain a hyphen (\"-\")"))
    } else {
        Ok(raw.trim().to_lowercase().replace(" ", "_"))
    }
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
    #[command(about = "Running up migratiosn")]
    Up,
    #[command(about = "Running up migratiosn")]
    Down,
}

pub async fn run_migrate_command(
    command: Option<MigrateSubcommands>,
    migration_dir: impl AsRef<Path>,
    db_type: &str,
    _qdrant_url: &url::Url,
    _qdrant_api_key: Option<String>,
) -> Result<(), CliError> {
    match command {
        Some(MigrateSubcommands::Init) | None => migrate::init(db_type, migration_dir).await?,
        Some(MigrateSubcommands::Generate { migration_name }) => {
            migrate::create_new_migration(db_type, migration_dir, &migration_name).await?
        }
        Some(MigrateSubcommands::Up) => {
            print!("TODO: trigger `cargo run ...` with the cli defined in the migration crate")
        }
        Some(MigrateSubcommands::Down) => println!("Up"),
    }

    Ok(())
}

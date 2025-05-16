mod migrate;

use clap::{Subcommand, command};
use std::process::{self};

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
    Init {
        #[arg(
            required = false,
            long,
            help = "set a package name for the generated template"
        )]
        package_name: Option<String>,
        #[arg(
            required = false,
            long,
            help = "set rust edition for the generated template"
        )]
        rust_edition: Option<String>,
    },
    #[command(about = "Generate new migration")]
    Generate {
        #[arg(required = true, value_parser = parse_migration_name)]
        migration_name: String,
    },
    #[command(about = "Running up migratiosn")]
    Up,
    #[command(about = "Running up migratiosn")]
    Down,
    #[command(about = "Get migration status")]
    Status,
}

pub async fn run_migrate_command(
    command: Option<MigrateSubcommands>,
    migration_dir: &str,
    _database_url: &url::Url,
    api_key: Option<String>,
) -> Result<(), CliError> {
    match command {
        Some(MigrateSubcommands::Init {
            package_name,
            rust_edition,
        }) => migrate::init(package_name, rust_edition, migration_dir).await?,
        Some(MigrateSubcommands::Generate { migration_name }) => {
            migrate::create_new_migration(migration_dir, &migration_name).await?
        }
        _ => {
            let subcommand = match command {
                Some(MigrateSubcommands::Up) => "up",
                Some(MigrateSubcommands::Down) => "down",
                Some(MigrateSubcommands::Status) => "status",
                _ => "up",
            };

            // Construct the `--manifest-path`
            let manifest_path = if migration_dir.ends_with('/') {
                format!("{migration_dir}Cargo.toml")
            } else {
                format!("{migration_dir}/Cargo.toml")
            };
            // Construct the arguments that will be supplied to `cargo` command
            let mut args = vec!["run", "--manifest-path", &manifest_path, "--", subcommand];

            if let Some(api_key) = api_key.as_ref() {
                args.extend(["-k", api_key]);
            }

            // Run migrator CLI on user's behalf
            println!("Running `cargo {}`", args.join(" "));
            let exit_status = process::Command::new("cargo")
                .args(args)
                .status()
                .map_err(|err| CliError::Custom(err.to_string()))?;
            if !exit_status.success() {
                // Propagate the error if any
                return Err(CliError::Custom("naa".into()));
            }
        }
    }

    Ok(())
}

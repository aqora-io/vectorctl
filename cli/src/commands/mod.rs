mod migrate;
use clap::{Subcommand, command};
use std::process::{self};

pub use migrate::{MigrateError, create_new_revision, init};

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
        #[arg(short = 'm', long, required = false)]
        message: Option<String>,
    },
    #[command(about = "Running up migratiosn")]
    Up {
        #[arg(long, required = false)]
        to: Option<String>,
    },
    #[command(about = "Running up migratiosn")]
    Down {
        #[arg(long, required = false)]
        to: Option<String>,
    },
    #[command(about = "Get migration status")]
    Status,
}

pub async fn run_migrate_command(
    command: Option<MigrateSubcommands>,
    migration_dir: &str,
    database_url: &url::Url,
    api_key: Option<String>,
) -> Result<(), CliError> {
    match command {
        Some(MigrateSubcommands::Init {
            package_name,
            rust_edition,
        }) => {
            migrate::init(
                package_name.as_deref(),
                rust_edition.as_deref(),
                migration_dir,
            )
            .await?
        }
        _ => {
            let (subcommand, migration_name, message, to) = match command {
                Some(MigrateSubcommands::Up { to }) => ("up", None, None, Some(to)),
                Some(MigrateSubcommands::Down { to }) => ("down", None, None, Some(to)),
                Some(MigrateSubcommands::Status) => ("status", None, None, None),
                Some(MigrateSubcommands::Generate {
                    migration_name,
                    message,
                }) => ("generate", Some(migration_name), Some(message), None),
                _ => ("up", None, None, None),
            };

            let manifest_path = if migration_dir.ends_with('/') {
                format!("{migration_dir}Cargo.toml")
            } else {
                format!("{migration_dir}/Cargo.toml")
            };

            let mut args = vec!["run", "--manifest-path", &manifest_path, "--", subcommand];

            if let Some(name) = migration_name.as_ref() {
                args.extend(["", name.as_str()]);
            }

            args.extend(["-u", database_url.as_str()]);

            if let Some(api_key) = api_key.as_ref() {
                args.extend(["-k", api_key]);
            }

            if let Some(Some(message)) = message.as_ref() {
                args.extend(["-m", message]);
            }

            if let Some(Some(to)) = to.as_ref() {
                args.extend(["--to", to]);
            }

            println!("Running `cargo {}`", args.join(" "));
            let exit_status = process::Command::new("cargo")
                .args(args)
                .status()
                .map_err(|err| CliError::Custom(err.to_string()))?;
            if !exit_status.success() {
                return Err(CliError::Custom(format!("exit status {}", exit_status)));
            }
        }
    }

    Ok(())
}

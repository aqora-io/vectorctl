mod migrate;
use clap::{Subcommand, command};
use std::{
    path::PathBuf,
    process::{self},
};

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
        name: String,
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
    migration_dir: PathBuf,
    database_url: url::Url,
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

        sub @ Some(MigrateSubcommands::Generate { .. })
        | sub @ Some(MigrateSubcommands::Up { .. })
        | sub @ Some(MigrateSubcommands::Down { .. })
        | sub @ Some(MigrateSubcommands::Status) => {
            let (cmd_str, extra_args) = match sub {
                Some(MigrateSubcommands::Generate { name, message }) => ("generate", {
                    let mut args = vec![name];
                    if let Some(msg) = message {
                        args.push("-m".into());
                        args.push(msg);
                    }
                    args
                }),
                Some(MigrateSubcommands::Up { to }) => (
                    "up",
                    to.into_iter()
                        .flat_map(|to| vec!["--to".into(), to])
                        .collect(),
                ),
                Some(MigrateSubcommands::Down { to }) => (
                    "down",
                    to.into_iter()
                        .flat_map(|to| vec!["--to".into(), to])
                        .collect(),
                ),
                Some(MigrateSubcommands::Status) => ("status", vec![]),
                _ => ("up", vec![]),
            };

            let manifest = migration_dir.join("Cargo.toml");

            let mut args = vec![
                "run".into(),
                "--manifest-path".into(),
                manifest.to_string_lossy().into_owned(),
                "--".into(),
                cmd_str.into(),
                "-u".into(),
                database_url.to_string(),
            ];
            if let Some(key) = api_key {
                args.push("-k".into());
                args.push(key);
            }
            args.extend(extra_args);

            println!("> cargo {}", args.join(" "));
            let status = process::Command::new("cargo")
                .args(&args)
                .status()
                .map_err(|e| CliError::Custom(e.to_string()))?;
            if !status.success() {
                return Err(CliError::Custom(format!(
                    "Migration process failed with {}",
                    status
                )));
            }
        }
        None => unreachable!(),
    }

    Ok(())
}

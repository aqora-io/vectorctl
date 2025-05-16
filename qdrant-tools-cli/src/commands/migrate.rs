use std::{
    iter::once,
    path::{Path, PathBuf},
};

use chrono::Utc;
use proc_macro2::Span;
use quote::quote;
use syn::{Ident, Item, ItemMod, parse_file, parse2};
use template::MigrationTemplate;
use thiserror::Error;
use tokio::fs;

const MIGRATION_FILE_PREFIX: &str = "version";

#[derive(Debug, Error)]
pub enum MigrateCommandError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Parser(#[from] syn::Error),
    #[error("toml {0}")]
    Toml(String),
    #[error("migration crate already exist")]
    Exist,
    #[error("{0}")]
    Custom(String),
}

fn timestamp() -> String {
    format!(
        "{}_{}",
        MIGRATION_FILE_PREFIX,
        Utc::now().format("%Y%m%d_%H%M%S")
    )
}

async fn create_migration_file_template(
    migration_dir: impl AsRef<Path>,
    filename: &str,
) -> Result<(), MigrateCommandError> {
    let path = migration_dir.as_ref();
    // file defined in template/assets/migration/src
    fs::copy(
        path.join("version_20250101_011111_init_migration.rs"),
        path.join(format!("{}.rs", filename)),
    )
    .await?;

    Ok(())
}

fn generate_migrator_file_template(mod_idents: Vec<Ident>) -> Result<String, MigrateCommandError> {
    let tokens = quote! {
        pub use qdrant_tools_migration::migrator::MigratorTrait;
        use qdrant_tools_migration::MigrationTrait;

        #( mod #mod_idents; )*

        pub struct Migrator;

        #[async_trait::async_trait]
        impl MigratorTrait for Migrator {
             fn migrations() -> Vec<Box<dyn MigrationTrait>> {
                vec![
                    #( Box::new(#mod_idents::Migration) ),*
                ]
            }

        }
    };
    Ok(prettyplease::unparse(&parse2(tokens)?))
}

async fn write_out(
    path: impl AsRef<Path>,
    contents: impl AsRef<str>,
) -> Result<(), MigrateCommandError> {
    let path = path.as_ref();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await?;
    }
    fs::write(path, contents.as_ref()).await?;
    Ok(())
}

async fn get_mods(path: &Path) -> Result<Vec<Ident>, MigrateCommandError> {
    Ok(parse_file(&fs::read_to_string(path).await?)?
        .items
        .into_iter()
        .filter_map(|item| match item {
            Item::Mod(ItemMod { ref ident, .. }) => {
                Some(Ident::new(&ident.to_string(), ident.span()))
            }
            _ => None,
        })
        .collect::<Vec<_>>())
}

// https://github.com/SeaQL/sea-orm/blob/4b4af1605addc5f5edbb0bd75b03644490004373/sea-orm-cli/src/commands/migrate.rs#L169
fn get_full_migration_dir(migration_dir: impl AsRef<Path>) -> PathBuf {
    let without_src = migration_dir.as_ref().to_owned();
    let with_src = without_src.join("src");
    match () {
        _ if with_src.is_dir() => with_src,
        _ => without_src,
    }
}

fn get_migrator_filepath(migration_dir: impl AsRef<Path>) -> PathBuf {
    get_full_migration_dir(migration_dir).join("lib.rs")
}

async fn create_rust_files(
    migration_dir: impl AsRef<Path>,
    mod_name: &str,
    idents: Option<Vec<Ident>>,
) -> Result<(), MigrateCommandError> {
    let path = get_full_migration_dir(&migration_dir);
    let mod_ident = Ident::new(mod_name, Span::call_site());
    create_migration_file_template(&path, mod_name).await?;

    let init_migrator = generate_migrator_file_template(match idents {
        Some(idents) => idents
            .into_iter()
            .chain(once(mod_ident))
            .collect::<Vec<_>>(),
        None => once(mod_ident).collect::<Vec<_>>(),
    })?;

    write_out(get_migrator_filepath(path), init_migrator).await
}

pub async fn init(
    package_name: Option<String>,
    rust_edition: Option<String>,
    migration_dir: impl AsRef<Path>,
) -> Result<(), MigrateCommandError> {
    let mut builder = MigrationTemplate::builder();
    if let Some(package_name) = package_name.as_ref() {
        builder.package_name(package_name);
    }
    if let Some(rust_edition) = rust_edition.as_ref() {
        builder.rust_edition(rust_edition);
    }
    builder
        .render(migration_dir)
        .map_err(|err| MigrateCommandError::Custom(err.to_string()))?;

    Ok(())
}

pub async fn create_new_migration(
    migration_dir: impl AsRef<Path>,
    migration_name: &str,
) -> Result<(), MigrateCommandError> {
    let migrator_filepath = get_migrator_filepath(&migration_dir);
    let migrator_backup_filepath = migrator_filepath.with_extension("rs.bak");
    fs::copy(&migrator_filepath, &migrator_backup_filepath).await?;

    create_rust_files(
        migration_dir,
        &format!("{}_{}", timestamp(), migration_name),
        Some(get_mods(&migrator_filepath).await?),
    )
    .await?;

    fs::remove_file(&migrator_backup_filepath).await?;

    Ok(())
}

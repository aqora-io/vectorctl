use std::{
    collections::BTreeMap,
    iter::once,
    path::{Path, PathBuf},
};

use chrono::Utc;
use proc_macro2::Span;
use quote::quote;
use serde::Serialize;
use syn::{Ident, Item, ItemMod, LitStr, parse_file, parse2};
use thiserror::Error;
use tokio::fs;
use toml::Value;

const MIGRATION_FILE_PREFIX: &str = "version";
const HISTORY_FILE_NAME: &str = "history.json";

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
}

#[derive(Serialize, Default)]
struct Package {
    name: String,
    version: String,
    edition: String,
}

#[derive(Serialize)]
struct CargoToml {
    package: Package,
    dependencies: BTreeMap<String, toml::Value>,
}

async fn create_cargo_toml(path: impl AsRef<Path>) -> Result<(), MigrateCommandError> {
    let pkg = Package {
        name: "migration".into(),
        version: "0.1.0".into(),
        edition: "2021".into(),
    };

    let mut deps = BTreeMap::new();
    deps.insert("async-trait".into(), Value::from("0.1.88"));

    let doc = CargoToml {
        package: pkg,
        dependencies: deps,
    };

    fs::write(
        path,
        toml::to_string(&doc).map_err(|err| MigrateCommandError::Toml(err.to_string()))?,
    )
    .await?;

    Ok(())
}

fn timestamp() -> String {
    format!(
        "{}_{}",
        MIGRATION_FILE_PREFIX,
        Utc::now().format("%Y%m%d_%H%M%S")
    )
}

fn generate_migration_file_template(
    mod_ident: &Ident,
    db_type: &syn::Path,
) -> Result<String, MigrateCommandError> {
    let description = format!("description for {} migration", mod_ident);
    let tokens = quote! {
        use qdrant_tools_macro::DeriveMigrationName;
        use qdrant_tools_migration::{MigrationTrait, migrator::{MigrationError}};

        #[derive(DeriveMigrationName)]
            pub struct Migration;

            #[async_trait::async_trait]
            impl MigrationTrait for Migration {
                type Db = #db_type;

                fn description(&self) -> String {
                    #description.into()
                }

                async fn up(&self, qdrant: &Qdrant, _db: &Self::Db) -> Result<(), MigrationError> {
                    todo!();
                }

                async fn down(&self, qdrant: &Qdrant, _db: &Self::Db) -> Result<(), MigrationError> {
                    todo!();
                }
        }
    };
    Ok(prettyplease::unparse(&parse2(tokens)?))
}

fn generate_migrator_file_template(
    mod_idents: Vec<Ident>,
    db_type: &syn::Path,
    path_ident: LitStr,
) -> Result<String, MigrateCommandError> {
    let tokens = quote! {

        use

        #( mod #mod_idents; )*

        pub struct Migrator;

        #[async_trait::async_trait]
        impl MigratorTrait for Migrator {
             type Db = #db_type;

             fn migrations() -> Vec<Box<dyn MigrationTrait<Db = Self::Db>>> {
                vec![
                    #( Box::new(#mod_idents::Migration) ),*
                ]
            }

            fn history_path() -> std::path::PathBuf {
                std::path::PathBuf::from(#path_ident)
            }

        }
    };
    Ok(prettyplease::unparse(&parse2(tokens)?))
}

fn generate_main_file_template() -> Result<String, MigrateCommandError> {
    let tokens = quote! {
        #[async_std::main]
        async fn main() {
            cli::run_cli(migration::Migrator).await;
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
    db_type: impl AsRef<str>,
    migration_dir: impl AsRef<Path>,
    mod_name: &str,
    idents: Option<Vec<Ident>>,
) -> Result<(), MigrateCommandError> {
    let path = get_full_migration_dir(&migration_dir);
    let migration_file = path.join(format!("{}.rs", mod_name));
    let db_tye_ident = syn::parse_str::<syn::Path>(db_type.as_ref())?;
    let mod_ident = Ident::new(mod_name, Span::call_site());
    let init_migration = generate_migration_file_template(&mod_ident, &db_tye_ident)?;
    write_out(migration_file, init_migration).await?;

    let history_path_lit = LitStr::new(
        migration_dir
            .as_ref()
            .join(HISTORY_FILE_NAME)
            .to_str()
            .expect("utf-8 path"),
        Span::call_site(),
    );
    let init_migrator = generate_migrator_file_template(
        match idents {
            Some(idents) => idents
                .into_iter()
                .chain(once(mod_ident))
                .collect::<Vec<_>>(),
            None => once(mod_ident).collect::<Vec<_>>(),
        },
        &db_tye_ident,
        history_path_lit,
    )?;

    write_out(get_migrator_filepath(path), init_migrator).await
}

pub async fn init(
    db_type: impl AsRef<str>,
    migration_dir: impl AsRef<Path>,
) -> Result<(), MigrateCommandError> {
    let src_path = migration_dir.as_ref().join("src");
    let cargo_toml_path = migration_dir.as_ref().join("Cargo.toml");
    if cargo_toml_path.exists() {
        return Err(MigrateCommandError::Exist);
    }
    create_cargo_toml(cargo_toml_path).await?;
    create_rust_files(
        db_type,
        &src_path,
        &format!("{}_init_migration", timestamp()),
        None,
    )
    .await?;
    write_out(src_path.join("main.rs"), generate_main_file_template()?).await
}

pub async fn create_new_migration(
    db_type: impl AsRef<str>,
    migration_dir: impl AsRef<Path>,
    migration_name: &str,
) -> Result<(), MigrateCommandError> {
    let migrator_filepath = get_migrator_filepath(&migration_dir);
    let migrator_backup_filepath = migrator_filepath.with_extension("rs.bak");
    fs::copy(&migrator_filepath, &migrator_backup_filepath).await?;

    create_rust_files(
        db_type,
        migration_dir,
        &format!("{}_{}", timestamp(), migration_name),
        Some(get_mods(&migrator_filepath).await?),
    )
    .await?;

    fs::remove_file(&migrator_backup_filepath).await?;

    Ok(())
}

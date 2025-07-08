mod cli;
mod context;
mod migrator;
mod revision;

use std::fmt::Debug;

pub use cli::{CliError as CliMigrationError, run_migrate};
pub use context::{Context, ContextError, Resource};
pub use migrator::{MigrationError, MigratorTrait};

pub fn get_file_stem(path: &str) -> &str {
    std::path::Path::new(path)
        .file_stem()
        .map(|file| file.to_str().unwrap())
        .unwrap()
}

#[derive(Debug)]
pub struct Revision<'a> {
    pub message: Option<&'a str>,
    pub revision: &'a str,
    pub down_revision: Option<&'a str>,
    pub date: &'a str,
}

pub trait MigrationMeta {
    fn name(&self) -> String;
    fn revision(&self) -> Revision;
}

#[async_trait::async_trait]
pub trait MigrationTrait: MigrationMeta + Send + Sync + Debug {
    async fn up(&self, ctx: &context::Context) -> Result<(), MigrationError>;
    async fn down(&self, ctx: &context::Context) -> Result<(), MigrationError>;
}

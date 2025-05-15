use chrono::{DateTime, Utc};
use migrator::{MigrationError, MigrationId};

mod context;

pub mod cli;
pub mod migrator;

pub fn get_file_stem(path: &str) -> &str {
    std::path::Path::new(path)
        .file_stem()
        .map(|file| file.to_str().unwrap())
        .unwrap()
}

pub trait MigrationName {
    fn id(&self) -> MigrationId;
    fn applied_at(&self) -> DateTime<Utc>;
}

#[async_trait::async_trait]
pub trait MigrationTrait: MigrationName + Send + Sync {
    fn description(&self) -> String;
    async fn up(&self, ctx: &context::Context<'_>) -> Result<(), MigrationError>;
    async fn down(&self, ctx: &context::Context<'_>) -> Result<(), MigrationError>;
}

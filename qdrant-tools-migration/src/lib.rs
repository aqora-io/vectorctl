use migrator::{MigrationError, MigrationId};

pub mod cli;
pub mod context;
pub mod migrator;

pub fn get_file_stem(path: &str) -> &str {
    std::path::Path::new(path)
        .file_stem()
        .map(|file| file.to_str().unwrap())
        .unwrap()
}

pub trait MigrationMeta {
    fn id(&self) -> MigrationId;
    fn message(&self) -> String;
}

#[async_trait::async_trait]
pub trait MigrationTrait: MigrationMeta + Send + Sync {
    async fn up(&self, ctx: &context::Context<'_>) -> Result<(), MigrationError>;
    async fn down(&self, ctx: &context::Context<'_>) -> Result<(), MigrationError>;
}

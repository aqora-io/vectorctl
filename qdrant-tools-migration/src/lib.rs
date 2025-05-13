use migrator::MigrationError;
use qdrant_client::Qdrant;

pub mod migrator;

pub fn get_file_stem(path: &str) -> &str {
    std::path::Path::new(path)
        .file_stem()
        .map(|file| file.to_str().unwrap())
        .unwrap()
}

pub trait MigrationName {
    fn name(&self) -> &str;
}

#[async_trait::async_trait]
pub trait MigrationTrait: MigrationName + Send + Sync {
    type Db: Send + Sync + 'static;

    fn description(&self) -> String;
    async fn up(&self, qdrant: &Qdrant, db: &Self::Db) -> Result<(), MigrationError>;
    async fn down(&self, qdrant: &Qdrant, db: &Self::Db) -> Result<(), MigrationError>;
}

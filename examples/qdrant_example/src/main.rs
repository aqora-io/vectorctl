#[tokio::main]
async fn main() -> Result<(), vectorctl::CliMigrationError> {
    vectorctl::run_migrate(qdrant_exemple::Migrator, None).await
}

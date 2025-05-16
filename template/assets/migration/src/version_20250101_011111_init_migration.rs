use qdrant_tools_macro::DeriveMigrationName;
use qdrant_tools_migration::{MigrationTrait, migrator::MigrationError};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(
        &self,
        _ctx: &qdrant_tools_migration::context::Context<'_>,
    ) -> Result<(), MigrationError> {
        todo!();
    }
    async fn down(
        &self,
        _ctx: &qdrant_tools_migration::context::Context<'_>,
    ) -> Result<(), MigrationError> {
        todo!();
    }
}

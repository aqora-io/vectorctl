use qdrant_tool_macros::DeriveMigrationMeta;
use migration::{MigrationTrait, migrator::MigrationError};

#[derive(DeriveMigrationMeta)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(
        &self,
        _ctx: &migration::context::Context<'_>,
    ) -> Result<(), MigrationError> {
        todo!();
    }
    async fn down(
        &self,
        _ctx: &migration::context::Context<'_>,
    ) -> Result<(), MigrationError> {
        todo!();
    }
}

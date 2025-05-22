use qdrant_client::qdrant::{CreateCollectionBuilder, Distance, VectorParamsBuilder};
use vectorctl::{DeriveMigrationMeta, MigrationError, MigrationTrait, Revision};

pub const REVISION: Revision = Revision {
    date: "2025-05-22T14:59:10",
    down_revision: None,
    revision: "t1Jy_CxeQoU",
    message: None,
};

#[derive(DeriveMigrationMeta, Debug)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, ctx: &vectorctl::Context<'_>) -> Result<(), MigrationError> {
        let qdrant = ctx.qdrant;

        qdrant
            .create_collection(
                CreateCollectionBuilder::new("my_collection")
                    .vectors_config(VectorParamsBuilder::new(100, Distance::Cosine)),
            )
            .await?;

        Ok(())
    }
    async fn down(&self, ctx: &vectorctl::Context<'_>) -> Result<(), MigrationError> {
        let qdrant = ctx.qdrant;

        qdrant.delete_collection("my_collection").await?;

        Ok(())
    }
}

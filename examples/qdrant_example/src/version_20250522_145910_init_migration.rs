use qdrant_client::qdrant::{CreateCollectionBuilder, Distance, VectorParamsBuilder};
use vectorctl::{
    DeriveMigrationMeta, MigrationError, MigrationTrait, Revision, VectorBackendError,
};

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
    async fn up(&self, ctx: &vectorctl::Context) -> Result<(), MigrationError> {
        let qdrant = &ctx.backend.client;

        qdrant
            .create_collection(
                CreateCollectionBuilder::new("my_collection")
                    .vectors_config(VectorParamsBuilder::new(100, Distance::Cosine)),
            )
            .await
            .map_err(VectorBackendError::from)?;

        Ok(())
    }
    async fn down(&self, ctx: &vectorctl::Context) -> Result<(), MigrationError> {
        let qdrant = &ctx.backend.client;

        qdrant
            .delete_collection("my_collection")
            .await
            .map_err(VectorBackendError::from)?;

        Ok(())
    }
}

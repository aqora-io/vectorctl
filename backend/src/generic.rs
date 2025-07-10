use std::{collections::HashMap, sync::Arc};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum VectorBackendError {
    #[cfg(feature = "qdrant-backend")]
    #[error(transparent)]
    Serde(#[from] serde_json::Error),
    #[cfg(feature = "qdrant-backend")]
    #[error(transparent)]
    Qdrant(Box<qdrant_client::QdrantError>),
    #[error(transparent)]
    Uuid(#[from] uuid::Error),
    #[error("Other {0}")]
    Other(String),
}

#[cfg(feature = "qdrant-backend")]
impl From<qdrant_client::QdrantError> for VectorBackendError {
    fn from(value: qdrant_client::QdrantError) -> Self {
        Self::Qdrant(Box::new(value))
    }
}

#[async_trait::async_trait]
pub trait VectorTrait: Send + Sync + 'static {
    type Error: Into<VectorBackendError>;
    type Client: Send + Sync + 'static;
    type Key: Send + Sync + 'static;
    type Value: Send + Sync + 'static;
    type Ledger: LedgerTrait<Key = Self::Key, Value = Self::Value>;

    fn new(uri: &str, api_key: Option<String>) -> Result<Self, Self::Error>
    where
        Self: Sized;

    fn new_with_client(client: Arc<Self::Client>) -> Self
    where
        Self: Sized;

    fn ledger(&self) -> Self::Ledger;
}

#[async_trait::async_trait]
pub trait LedgerTrait: Send + Sync {
    type Key;
    type Value;

    fn collection_name(&self) -> String;
    async fn ensure(&self) -> Result<(), VectorBackendError>;
    async fn retrieve(&self) -> Result<HashMap<Self::Key, Self::Value>, VectorBackendError>;
    async fn insert_many(&self, ids: Vec<Self::Key>) -> Result<(), VectorBackendError>;
    async fn delete_many(&self, ids: Vec<Self::Value>) -> Result<(), VectorBackendError>;
}

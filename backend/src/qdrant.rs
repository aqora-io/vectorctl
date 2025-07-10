use crate::generic::{LedgerTrait, VectorBackendError, VectorTrait};
use chrono::{DateTime, Utc};
use qdrant_client::{
    Payload as QdrantPayload, Qdrant,
    config::CompressionEncoding,
    qdrant::{
        CreateCollectionBuilder, DeletePointsBuilder, Distance, PointId, PointStruct,
        PointsIdsList, ScrollPointsBuilder, UpsertPointsBuilder, Value, VectorParamsBuilder,
        point_id::PointIdOptions,
    },
};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, convert::TryFrom, sync::Arc};
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
struct Payload {
    name: String,
    applied_at: DateTime<Utc>,
}

impl TryFrom<HashMap<String, Value>> for Payload {
    type Error = serde_json::Error;

    fn try_from(map: HashMap<String, Value>) -> Result<Self, Self::Error> {
        serde_json::from_value(serde_json::to_value(map)?)
    }
}

impl TryFrom<Payload> for PointStruct {
    type Error = VectorBackendError;

    fn try_from(payload: Payload) -> Result<Self, Self::Error> {
        Ok(PointStruct::new(
            Uuid::now_v7().to_string(),
            vec![0.0_f32; 1],
            QdrantPayload::try_from(serde_json::to_value(payload)?)?,
        ))
    }
}

pub struct QdrantBackend {
    pub client: Arc<Qdrant>,
}

impl QdrantBackend {
    fn new(client: Qdrant) -> Self {
        Self {
            client: Arc::new(client),
        }
    }
}

#[derive(Clone)]
pub struct Ledger {
    client: Arc<Qdrant>,
}

impl Ledger {
    pub fn new(client: Arc<Qdrant>) -> Self {
        Self { client }
    }
}

#[async_trait::async_trait]
impl LedgerTrait for Ledger {
    type Key = String;
    type Value = Uuid;

    fn collection_name(&self) -> String {
        "_qdrant_migration".into()
    }

    async fn ensure(&self) -> Result<(), VectorBackendError> {
        if self
            .client
            .collection_exists(self.collection_name())
            .await?
        {
            return Ok(());
        }

        let builder = CreateCollectionBuilder::new(self.collection_name())
            .vectors_config(VectorParamsBuilder::new(1, Distance::Cosine))
            .build();

        self.client.create_collection(builder).await?;
        Ok(())
    }

    async fn retrieve(&self) -> Result<HashMap<Self::Key, Self::Value>, VectorBackendError> {
        let scroll = self
            .client
            .scroll(
                ScrollPointsBuilder::new(self.collection_name())
                    .with_payload(true)
                    .with_vectors(false),
            )
            .await?;

        Ok(scroll
            .result
            .into_iter()
            .filter_map(|point| {
                let id = match point.id?.point_id_options? {
                    PointIdOptions::Uuid(ref s) => Uuid::try_parse(s).ok()?,
                    PointIdOptions::Num(_) => return None,
                };

                Payload::try_from(point.payload)
                    .ok()
                    .map(|payload| (payload.name, id))
            })
            .collect())
    }

    async fn insert_many(&self, ids: Vec<Self::Key>) -> Result<(), VectorBackendError> {
        let now = Utc::now();

        let points = ids
            .into_iter()
            .map(|id| {
                let payload = Payload {
                    name: id,
                    applied_at: now,
                };
                PointStruct::try_from(payload)
            })
            .collect::<Result<Vec<_>, _>>()?;

        self.client
            .upsert_points(UpsertPointsBuilder::new(self.collection_name(), points).wait(true))
            .await?;

        Ok(())
    }

    async fn delete_many(&self, ids: Vec<Self::Value>) -> Result<(), VectorBackendError> {
        let points = PointsIdsList {
            ids: ids
                .into_iter()
                .map(|id| PointId::from(id.to_string()))
                .collect(),
        };

        let builder = DeletePointsBuilder::new(self.collection_name())
            .points(points)
            .build();

        self.client.delete_points(builder).await?;
        Ok(())
    }
}

impl VectorTrait for QdrantBackend {
    type Client = Qdrant;
    type Key = String;
    type Value = Uuid;
    type Error = VectorBackendError;
    type Ledger = Ledger;

    fn new(uri: &str, api_key: Option<String>) -> Result<Self, Self::Error> {
        Ok(Self::new(
            Qdrant::from_url(uri)
                .api_key(api_key)
                .compression(Some(CompressionEncoding::Gzip))
                .build()?,
        ))
    }

    fn new_with_client(client: Arc<Self::Client>) -> Self {
        Self { client }
    }

    fn ledger(&self) -> Self::Ledger {
        Ledger::new(Arc::clone(&self.client))
    }
}

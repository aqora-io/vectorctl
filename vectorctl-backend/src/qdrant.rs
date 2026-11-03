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
use std::{
    collections::{HashMap, HashSet},
    convert::TryFrom,
    sync::Arc,
};
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

    // Every (name, point id) in the ledger. A scroll call returns one page (10 points
    // by default), so keep following `next_page_offset`. A name can have several
    // points if its migration was applied more than once.
    async fn entries(&self) -> Result<Vec<(String, Uuid)>, VectorBackendError> {
        let mut entries = Vec::new();
        let mut offset = None;

        loop {
            let mut builder = ScrollPointsBuilder::new(self.collection_name())
                .with_payload(true)
                .with_vectors(false);
            if let Some(offset) = offset {
                builder = builder.offset(offset);
            }

            let scroll = self.client.scroll(builder).await?;

            entries.extend(scroll.result.into_iter().filter_map(|point| {
                let id = match point.id?.point_id_options? {
                    PointIdOptions::Uuid(ref s) => Uuid::try_parse(s).ok()?,
                    PointIdOptions::Num(_) => return None,
                };

                Payload::try_from(point.payload)
                    .ok()
                    .map(|payload| (payload.name, id))
            }));

            offset = scroll.next_page_offset;
            if offset.is_none() {
                return Ok(entries);
            }
        }
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
        Ok(self.entries().await?.into_iter().collect())
    }

    async fn insert_many(&self, ids: Vec<Self::Key>) -> Result<(), VectorBackendError> {
        let now = Utc::now();

        let points = ids
            .into_iter()
            .map(|id| {
                PointStruct::try_from(Payload {
                    name: id,
                    applied_at: now,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;

        self.client
            .upsert_points(UpsertPointsBuilder::new(self.collection_name(), points).wait(true))
            .await?;

        Ok(())
    }

    // Deletes every point recorded under the same names as `ids`, so a rolled-back
    // migration with duplicate points no longer reads as applied.
    async fn delete_many(&self, ids: Vec<Self::Value>) -> Result<(), VectorBackendError> {
        let entries = self.entries().await?;
        let names = entries
            .iter()
            .filter(|(_, id)| ids.contains(id))
            .map(|(name, _)| name)
            .collect::<HashSet<_>>();

        let points = PointsIdsList {
            ids: entries
                .iter()
                .filter(|(name, _)| names.contains(name))
                .map(|(_, id)| PointId::from(id.to_string()))
                .collect(),
        };
        if points.ids.is_empty() {
            return Ok(());
        }

        let builder = DeletePointsBuilder::new(self.collection_name())
            .points(points)
            .wait(true)
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

#[cfg(test)]
mod tests {
    use super::*;

    // Drops and recreates the `_qdrant_migration` collection, so only point it at a
    // throwaway Qdrant (gRPC url, e.g. `docker run -p 6334:6334 qdrant/qdrant`).
    #[tokio::test]
    #[ignore = "needs a throwaway Qdrant at VECTORCTL_TEST_QDRANT_URL"]
    async fn ledger_reads_and_deletes_past_the_first_scroll_page() {
        let url = std::env::var("VECTORCTL_TEST_QDRANT_URL")
            .expect("VECTORCTL_TEST_QDRANT_URL must point at a throwaway Qdrant");
        let client = Arc::new(Qdrant::from_url(&url).build().unwrap());
        let ledger = Ledger::new(Arc::clone(&client));
        if client
            .collection_exists(ledger.collection_name())
            .await
            .unwrap()
        {
            client
                .delete_collection(ledger.collection_name())
                .await
                .unwrap();
        }
        ledger.ensure().await.unwrap();

        // More points than one scroll page holds, plus a duplicate point for "m0000"
        // like the ones re-applied migrations left behind.
        let names = (0..25).map(|i| format!("m{i:04}")).collect::<Vec<_>>();
        ledger.insert_many(names).await.unwrap();
        ledger.insert_many(vec!["m0000".into()]).await.unwrap();

        let applied = ledger.retrieve().await.unwrap();
        assert_eq!(applied.len(), 25);

        ledger.delete_many(vec![applied["m0000"]]).await.unwrap();

        let applied = ledger.retrieve().await.unwrap();
        assert_eq!(applied.len(), 24);
        assert!(!applied.contains_key("m0000"));

        ledger.delete_many(vec![Uuid::now_v7()]).await.unwrap();
        assert_eq!(ledger.retrieve().await.unwrap().len(), 24);
    }
}

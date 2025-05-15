use chrono::{DateTime, Utc};
use itertools::Itertools;
use qdrant_client::{
    Payload, Qdrant,
    qdrant::{
        CreateCollectionBuilder, DeletePointsBuilder, PointId, PointStruct, PointsIdsList,
        ScrollPointsBuilder, UpsertPointsBuilder, point_id::PointIdOptions,
    },
};
use serde::{Deserialize, Serialize};
use std::{
    borrow::Cow,
    collections::{HashMap, HashSet},
};
use thiserror::Error;

use crate::MigrationTrait;

const LEDGER_COLLECTION: &str = "_qdrant_migration";

#[repr(transparent)]
#[derive(Clone, Hash, Eq, PartialEq, Debug, Deserialize, Serialize)]
pub struct MigrationId(Cow<'static, str>);

impl From<&'static str> for MigrationId {
    fn from(s: &'static str) -> Self {
        Self(Cow::Borrowed(s))
    }
}

impl std::fmt::Display for MigrationId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Error)]
pub enum MigrationError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Serde(#[from] serde_json::Error),
    #[error(transparent)]
    Qdrant(#[from] qdrant_client::QdrantError),
    #[error("migration {0} missing in file‑system")]
    Missing(MigrationId),
    #[error("payload field {0} absent")]
    PayloadMissing(&'static str),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MigrationRecord {
    id: MigrationId,
    applied_at: DateTime<Utc>,
}

impl TryFrom<HashMap<String, qdrant_client::qdrant::Value>> for MigrationRecord {
    type Error = serde_json::Error;

    fn try_from(
        payload: HashMap<String, qdrant_client::qdrant::Value>,
    ) -> Result<Self, Self::Error> {
        let json_value = serde_json::to_value(payload)?;
        serde_json::from_value(json_value)
    }
}

impl MigrationRecord {
    fn try_into_point(self) -> Result<PointStruct, MigrationError> {
        Ok(PointStruct::new(
            self.id.to_string(),
            vec![0.0],
            Payload::try_from(serde_json::json!({
                "id": self.id.to_string(),
                "applied_at": self.applied_at.timestamp(),
            }))?,
        ))
    }
}

#[async_trait::async_trait]
pub trait LedgerTrait {
    async fn ensure(&self) -> Result<(), MigrationError>;
    async fn retrieve(&self) -> Result<HashSet<MigrationId>, MigrationError>;
    async fn insert_many(&self, ids: Vec<MigrationId>) -> Result<(), MigrationError>;
    async fn delete(&self, id: MigrationId) -> Result<(), MigrationError>;
}

pub struct Ledger<'a> {
    client: &'a Qdrant,
}

impl<'a> Ledger<'a> {
    pub fn new(client: &'a Qdrant) -> Self {
        Self { client }
    }
}

#[async_trait::async_trait]
impl LedgerTrait for Ledger<'_> {
    async fn ensure(&self) -> Result<(), MigrationError> {
        if self.client.collection_exists(LEDGER_COLLECTION).await? {
            return Ok(());
        }
        self.client
            .create_collection(CreateCollectionBuilder::new(LEDGER_COLLECTION).build())
            .await?;
        Ok(())
    }

    async fn retrieve(&self) -> Result<HashSet<MigrationId>, MigrationError> {
        self.client
            .scroll(
                ScrollPointsBuilder::new(LEDGER_COLLECTION)
                    .with_payload(true)
                    .with_vectors(false),
            )
            .await?
            .result
            .into_iter()
            .map(|item| Ok(MigrationRecord::try_from(item.payload)?.id))
            .collect()
    }

    async fn insert_many(&self, ids: Vec<MigrationId>) -> Result<(), MigrationError> {
        let now = Utc::now();

        let points: Vec<_> = ids
            .into_iter()
            .map(|id| {
                MigrationRecord {
                    id,
                    applied_at: now,
                }
                .try_into_point()
            })
            .collect::<Result<Vec<_>, _>>()?;

        self.client
            .upsert_points(UpsertPointsBuilder::new(LEDGER_COLLECTION, points).wait(true))
            .await?;

        Ok(())
    }

    async fn delete(&self, id: MigrationId) -> Result<(), MigrationError> {
        self.client
            .delete_points(
                DeletePointsBuilder::new(LEDGER_COLLECTION)
                    .points(PointsIdsList {
                        ids: vec![PointId {
                            point_id_options: Some(PointIdOptions::Uuid(id.to_string())),
                        }],
                    })
                    .wait(true),
            )
            .await?;

        Ok(())
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum MigrationStatus {
    Pending,
    Applied,
}

impl std::fmt::Display for MigrationStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                Self::Pending => "Pending",
                Self::Applied => "Applied",
            }
        )
    }
}

pub struct Migration {
    migration: Box<dyn MigrationTrait>,
    status: MigrationStatus,
}

#[async_trait::async_trait]
pub trait MigratorTrait: Send {
    fn migrations() -> Vec<Box<dyn MigrationTrait>>;

    async fn migrations_with_status(qdrant: &Qdrant) -> Result<Vec<Migration>, MigrationError> {
        let applied = Ledger::new(qdrant).retrieve().await?;
        Ok(Self::migrations()
            .into_iter()
            .map(|migration| {
                let status = if applied.contains(&migration.id()) {
                    MigrationStatus::Applied
                } else {
                    MigrationStatus::Pending
                };
                Migration { migration, status }
            })
            .sorted_by(|a, b| a.migration.applied_at().cmp(&b.migration.applied_at()))
            .collect())
    }

    async fn migrations_by_status(
        status: MigrationStatus,
        qdrant: &Qdrant,
    ) -> Result<Vec<Migration>, MigrationError> {
        Ok(Self::migrations_with_status(qdrant)
            .await?
            .into_iter()
            .filter(|file| file.status == status)
            .collect())
    }

    async fn status(qdrant: &Qdrant) -> Result<(), MigrationError> {
        Self::migrations_with_status(qdrant)
            .await?
            .into_iter()
            .for_each(|migration| {
                println!(
                    "Migration `{}`, status : `{}`",
                    migration.migration.id(),
                    migration.status
                )
            });
        Ok(())
    }

    async fn refresh(ctx: &crate::context::Context<'_>) -> Result<(), MigrationError> {
        exec_down::<Self>(ctx).await?;
        exec_up::<Self>(ctx).await
    }

    async fn reset(ctx: &crate::context::Context<'_>) -> Result<(), MigrationError> {
        exec_down::<Self>(ctx).await
    }

    async fn up(ctx: &crate::context::Context<'_>) -> Result<(), MigrationError> {
        exec_up::<Self>(ctx).await
    }

    async fn down(ctx: &crate::context::Context<'_>) -> Result<(), MigrationError> {
        exec_down::<Self>(ctx).await
    }
}

async fn exec_up<M>(ctx: &crate::context::Context<'_>) -> Result<(), MigrationError>
where
    M: MigratorTrait + ?Sized,
{
    let qdrant = ctx.qdrant;
    let ledger = Ledger::new(qdrant);
    let mut migration_ids: Vec<MigrationId> = Vec::new();
    for migration in M::migrations_by_status(MigrationStatus::Pending, qdrant).await? {
        migration.migration.up(ctx).await?;
        println!(
            "applying {}, {}",
            migration.migration.id(),
            migration.migration.description()
        );
        migration_ids.push(migration.migration.id());
    }
    ledger.insert_many(migration_ids).await?;
    Ok(())
}

async fn exec_down<M>(ctx: &crate::context::Context<'_>) -> Result<(), MigrationError>
where
    M: MigratorTrait + ?Sized,
{
    let qdrant = ctx.qdrant;
    let ledger = Ledger::new(qdrant);
    for migration in M::migrations_by_status(MigrationStatus::Applied, qdrant)
        .await?
        .into_iter()
        .rev()
    {
        migration.migration.down(ctx).await?;
        ledger.delete(migration.migration.id()).await?;
    }
    Ok(())
}

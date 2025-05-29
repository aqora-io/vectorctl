use crate::{
    ContextError, MigrationTrait,
    revision::{RevisionGraph, RevisionGraphError},
};
use chrono::{DateTime, Utc};
use futures::future::join_all;
use once_cell::sync::OnceCell;
use qdrant_client::{
    Payload, Qdrant,
    qdrant::{
        CreateCollectionBuilder, DeletePointsBuilder, Distance, PointId, PointStruct,
        PointsIdsList, ScrollPointsBuilder, UpsertPointsBuilder, VectorParamsBuilder,
        point_id::PointIdOptions,
    },
};
use serde::{Deserialize, Serialize};
use std::{borrow::Cow, collections::HashMap};
use thiserror::Error;
use uuid::Uuid;

static GRAPH: OnceCell<RevisionGraph> = OnceCell::new();

#[repr(transparent)]
#[derive(Clone, Hash, Eq, PartialEq, Debug, Deserialize, Serialize)]
pub struct MigrationName(Cow<'static, str>);

impl From<&'static str> for MigrationName {
    fn from(s: &'static str) -> Self {
        Self(Cow::Borrowed(s))
    }
}

impl std::fmt::Display for MigrationName {
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
    Qdrant(Box<qdrant_client::QdrantError>),
    #[error(transparent)]
    Graph(#[from] RevisionGraphError),
    #[error("migration {0} missing in file‑system")]
    Missing(MigrationName),
    #[error("payload field {0} absent")]
    PayloadMissing(&'static str),
    #[error(transparent)]
    Uuid(#[from] uuid::Error),
    #[error(transparent)]
    Context(#[from] ContextError),
}

impl From<qdrant_client::QdrantError> for MigrationError {
    fn from(e: qdrant_client::QdrantError) -> Self {
        Self::Qdrant(Box::new(e))
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct MigrationPayload {
    name: MigrationName,
    applied_at: DateTime<Utc>,
}

impl TryFrom<HashMap<String, qdrant_client::qdrant::Value>> for MigrationPayload {
    type Error = serde_json::Error;

    fn try_from(v: HashMap<String, qdrant_client::qdrant::Value>) -> Result<Self, Self::Error> {
        serde_json::from_value(serde_json::to_value(v)?)
    }
}

impl MigrationPayload {
    fn try_into_point(self) -> Result<PointStruct, MigrationError> {
        Ok(PointStruct::new(
            uuid::Uuid::now_v7().to_string(),
            vec![0.0_f32; 1],
            Payload::try_from(serde_json::json!({
                "name": self.name.to_string(),
                "applied_at": self.applied_at,
            }))?,
        ))
    }
}

#[async_trait::async_trait]
pub trait LedgerTrait {
    const LEDGER_COLLECTION: &'static str;

    async fn ensure(&self) -> Result<(), MigrationError>;
    async fn retrieve(&self) -> Result<HashMap<MigrationName, Uuid>, MigrationError>;
    async fn insert_many(&self, ids: Vec<MigrationName>) -> Result<(), MigrationError>;
    async fn delete_many(&self, ids: Vec<Uuid>) -> Result<(), MigrationError>;
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
    const LEDGER_COLLECTION: &'static str = "_qdrant_migration";

    async fn ensure(&self) -> Result<(), MigrationError> {
        if self
            .client
            .collection_exists(Self::LEDGER_COLLECTION)
            .await?
        {
            return Ok(());
        }
        self.client
            .create_collection(
                CreateCollectionBuilder::new(Self::LEDGER_COLLECTION)
                    .vectors_config(VectorParamsBuilder::new(1, Distance::Cosine))
                    .build(),
            )
            .await?;
        Ok(())
    }

    async fn retrieve(&self) -> Result<HashMap<MigrationName, Uuid>, MigrationError> {
        Ok(self
            .client
            .scroll(
                ScrollPointsBuilder::new(Self::LEDGER_COLLECTION)
                    .with_payload(true)
                    .with_vectors(false),
            )
            .await?
            .result
            .into_iter()
            .filter_map(|item| {
                let id = item.id.and_then(|id| id.point_id_options);
                let uuid = match id? {
                    PointIdOptions::Num(_) => return None,
                    PointIdOptions::Uuid(uuid) => Uuid::try_parse(uuid.as_ref())
                        .map_err(MigrationError::Uuid)
                        .ok()?,
                };
                MigrationPayload::try_from(item.payload)
                    .ok()
                    .map(|payload| (payload.name, uuid))
            })
            .collect())
    }
    async fn insert_many(&self, ids: Vec<MigrationName>) -> Result<(), MigrationError> {
        let points: Vec<_> = ids
            .into_iter()
            .map(|id| {
                MigrationPayload {
                    name: id,
                    applied_at: Utc::now(),
                }
                .try_into_point()
            })
            .collect::<Result<Vec<_>, _>>()?;

        self.client
            .upsert_points(UpsertPointsBuilder::new(Self::LEDGER_COLLECTION, points).wait(true))
            .await?;

        Ok(())
    }

    async fn delete_many(&self, ids: Vec<Uuid>) -> Result<(), MigrationError> {
        self.client
            .delete_points(
                DeletePointsBuilder::new(Self::LEDGER_COLLECTION)
                    .points(PointsIdsList {
                        ids: ids
                            .into_iter()
                            .map(|id| PointId::from(id.to_string()))
                            .collect(),
                    })
                    .build(),
            )
            .await?;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
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
    pub migration: Box<dyn MigrationTrait>,
    pub id: Option<Uuid>,
    status: MigrationStatus,
}

#[async_trait::async_trait]
pub trait MigratorTrait: Send {
    fn migrations() -> Vec<Box<dyn MigrationTrait>>;

    fn revision_graph(
        migrations: Vec<Migration>,
    ) -> Result<&'static RevisionGraph, MigrationError> {
        Ok(GRAPH.get_or_try_init(|| RevisionGraph::try_from(migrations))?)
    }

    async fn migrations_with_status(qdrant: &Qdrant) -> Result<Vec<Migration>, MigrationError> {
        let applied = Ledger::new(qdrant).retrieve().await?;
        Ok(Self::migrations()
            .into_iter()
            .map(|migration| {
                let migration_name = migration.name();
                let status = if applied.contains_key(&migration_name) {
                    MigrationStatus::Applied
                } else {
                    MigrationStatus::Pending
                };
                Migration {
                    migration,
                    status,
                    id: applied.get(&migration_name).map(|id| id.to_owned()),
                }
            })
            .collect())
    }

    async fn status(ctx: &crate::context::Context) -> Result<(), MigrationError> {
        Self::migrations_with_status(ctx.resource::<Qdrant>()?)
            .await?
            .into_iter()
            .for_each(|migration| {
                println!(
                    "Migration `{}`, status : `{}`",
                    migration.migration.name(),
                    migration.status
                )
            });
        Ok(())
    }

    fn latest_revision() -> Result<Box<dyn MigrationTrait>, MigrationError> {
        Ok(Self::migrations()
            .into_iter()
            .max_by_key(|migration| migration.revision().date.to_owned())
            .expect("At this point we should at least have one migration"))
    }

    async fn refresh(ctx: &crate::context::Context) -> Result<(), MigrationError> {
        exec_down::<Self>(ctx, None).await?;
        exec_up::<Self>(ctx, None, None).await
    }

    async fn reset(ctx: &crate::context::Context) -> Result<(), MigrationError> {
        exec_down::<Self>(ctx, None).await
    }

    async fn up(ctx: &crate::context::Context, to: Option<String>) -> Result<(), MigrationError> {
        exec_up::<Self>(ctx, None, to.as_deref()).await
    }

    async fn down(ctx: &crate::context::Context, to: Option<String>) -> Result<(), MigrationError> {
        exec_down::<Self>(ctx, to.as_deref()).await
    }
}

async fn exec_up<M>(
    ctx: &crate::context::Context,
    from: Option<&str>,
    to: Option<&str>,
) -> Result<(), MigrationError>
where
    M: MigratorTrait + ?Sized,
{
    let qdrant = ctx.resource::<Qdrant>()?;
    let ledger = Ledger::new(qdrant);
    ledger.ensure().await?;
    let migrations = M::migrations_with_status(qdrant).await?;
    let graph = M::revision_graph(
        migrations
            .into_iter()
            .filter(|migration| migration.status == MigrationStatus::Pending)
            .collect(),
    )?;
    let path = graph.forward_path(
        Some(from.unwrap_or(graph.queue())),
        to.unwrap_or(graph.head()),
    );

    let ids_to_insert = join_all(path.into_iter().map(|revision| async move {
        match graph.get(&revision) {
            Some((_, migration)) => migration.up(ctx).await.map(|_| migration.name()),
            None => Err(MigrationError::Graph(RevisionGraphError::NotFound(
                format!("revision: `{:?}`", revision),
            ))),
        }
    }))
    .await
    .into_iter()
    .collect::<Result<Vec<_>, _>>()?;

    if !ids_to_insert.is_empty() {
        ledger.insert_many(ids_to_insert).await?;
    }
    Ok(())
}

async fn exec_down<M>(ctx: &crate::context::Context, to: Option<&str>) -> Result<(), MigrationError>
where
    M: MigratorTrait + ?Sized,
{
    let qdrant = ctx.resource::<Qdrant>()?;
    let ledger = Ledger::new(qdrant);
    ledger.ensure().await?;
    let migrations = M::migrations_with_status(qdrant).await?;

    let graph = M::revision_graph(
        migrations
            .into_iter()
            .filter(|migration| migration.status == MigrationStatus::Applied)
            .collect(),
    )?;

    let path = graph.backward_path(Some(graph.head()), to);
    let ids_to_remove = join_all(
        path.into_iter()
            .filter_map(|revision| graph.get(revision.as_ref()))
            .map(|(id, migration)| async move {
                match id {
                    Some(id) => migration.down(ctx).await.map(|_| id),
                    None => Err(MigrationError::Graph(RevisionGraphError::NotFound(
                        format!("revision: `{:?}`", migration.revision().revision),
                    ))),
                }
            }),
    )
    .await
    .into_iter()
    .collect::<Result<Vec<_>, _>>()?;

    if !ids_to_remove.is_empty() {
        ledger.delete_many(ids_to_remove).await?;
    }

    Ok(())
}

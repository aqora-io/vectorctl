use crate::{
    ContextError, MigrationTrait,
    revision::{RevisionGraph, RevisionGraphError},
};
use backend::generic::{LedgerTrait, VectorTrait};
use futures::future::join_all;
use once_cell::sync::OnceCell;
use std::collections::HashMap;
use thiserror::Error;
use uuid::Uuid;

static GRAPH: OnceCell<RevisionGraph> = OnceCell::new();

#[derive(Debug, Error)]
pub enum MigrationError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Graph(#[from] RevisionGraphError),
    #[error("migration {0} missing in file‑system")]
    Missing(String),
    #[error(transparent)]
    Context(#[from] ContextError),
    #[error(transparent)]
    VectorBackend(#[from] backend::generic::VectorBackendError),
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

    async fn migrations_with_status(
        applied: HashMap<String, Uuid>,
    ) -> Result<Vec<Migration>, MigrationError> {
        Ok(Self::migrations()
            .into_iter()
            .map(|migration| {
                let name = migration.name();
                let id = applied.get(&name).cloned();
                let status = if id.is_some() {
                    MigrationStatus::Applied
                } else {
                    MigrationStatus::Pending
                };
                Migration {
                    migration,
                    id,
                    status,
                }
            })
            .collect())
    }

    async fn status(ctx: &crate::context::Context) -> Result<(), MigrationError> {
        let ledger = &ctx.backend.ledger();
        ledger.ensure().await?;

        let migrations = Self::migrations_with_status(ledger.retrieve().await?).await?;
        migrations.into_iter().for_each(|m| {
            println!(
                "Migration `{}`, status : `{}`",
                m.migration.name(),
                m.status
            )
        });
        Ok(())
    }

    fn latest_revision() -> Result<Box<dyn MigrationTrait>, MigrationError> {
        Self::migrations()
            .into_iter()
            .max_by_key(|m| m.revision().date.to_owned())
            .ok_or_else(|| MigrationError::Missing("No migrations found".into()))
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
    let ledger = &ctx.backend.ledger();
    ledger.ensure().await?;

    let migrations = M::migrations_with_status(ledger.retrieve().await?)
        .await?
        .into_iter()
        .filter(|m| m.status == MigrationStatus::Pending)
        .collect::<Vec<_>>();

    let graph = M::revision_graph(migrations)?;

    let path = graph.forward_path(
        Some(from.unwrap_or(graph.queue())),
        to.unwrap_or(graph.head()),
    );

    let ids = join_all(path.into_iter().map(|revision| async move {
        let (_, migration) = graph.get(&revision).ok_or_else(|| {
            MigrationError::Graph(RevisionGraphError::NotFound(format!(
                "revision: `{:?}`",
                revision
            )))
        })?;
        migration.up(ctx).await.map(|_| migration.name())
    }))
    .await
    .into_iter()
    .collect::<Result<Vec<_>, _>>()?;

    if !ids.is_empty() {
        ledger.insert_many(ids).await?;
    }

    Ok(())
}

async fn exec_down<M>(ctx: &crate::context::Context, to: Option<&str>) -> Result<(), MigrationError>
where
    M: MigratorTrait + ?Sized,
{
    let ledger = &ctx.backend.ledger();
    ledger.ensure().await?;

    let migrations = M::migrations_with_status(ledger.retrieve().await?)
        .await?
        .into_iter()
        .filter(|m| m.status == MigrationStatus::Applied)
        .collect::<Vec<_>>();

    let graph = M::revision_graph(migrations)?;

    let path = graph.backward_path(Some(graph.head()), to);

    let ids = join_all(
        path.into_iter()
            .filter_map(|revision| graph.get(revision.as_ref()))
            .map(|(id, migration)| async move {
                let id = id.ok_or_else(|| {
                    MigrationError::Graph(RevisionGraphError::NotFound(format!(
                        "revision: `{:?}`",
                        migration.revision().revision
                    )))
                })?;
                migration.down(ctx).await.map(|_| id)
            }),
    )
    .await
    .into_iter()
    .collect::<Result<Vec<_>, _>>()?;

    if !ids.is_empty() {
        ledger.delete_many(ids).await?;
    }

    Ok(())
}

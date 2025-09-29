use crate::{
    ContextError, MigrationTrait,
    revision::{Node, RevisionGraph, RevisionGraphError},
};
use once_cell::sync::OnceCell;
use std::collections::HashMap;
use thiserror::Error;
use uuid::Uuid;
use vectorctl_backend::generic::{LedgerTrait, VectorTrait};

static GRAPH: OnceCell<RevisionGraph> = OnceCell::new();

#[derive(Debug, Error)]
pub enum MigrationError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Graph(#[from] RevisionGraphError),
    #[error("migration {0} missing")]
    Missing(String),
    #[error(transparent)]
    Context(#[from] ContextError),
    #[error(transparent)]
    VectorBackend(#[from] vectorctl_backend::generic::VectorBackendError),
    #[cfg(feature = "sea-backend")]
    #[error(transparent)]
    Db(#[from] sea_orm::DbErr),
    #[error("Other {0}")]
    Other(String),
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
                MigrationStatus::Pending => "Pending",
                MigrationStatus::Applied => "Applied",
            }
        )
    }
}

#[derive(Debug)]
pub struct Migration {
    pub runner: Box<dyn MigrationTrait>,
    pub id: Option<Uuid>,
    pub status: MigrationStatus,
}

pub enum Direction {
    Up,
    Down,
    Refresh,
}

#[async_trait::async_trait]
pub trait MigratorTrait: Send {
    fn migrations() -> Vec<Box<dyn MigrationTrait>>;

    fn build_graph(
        applied: &HashMap<String, Uuid>,
    ) -> Result<&'static RevisionGraph, MigrationError> {
        Ok(GRAPH.get_or_try_init(|| {
            RevisionGraph::try_from(
                Self::migrations()
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
                            runner: migration,
                            id,
                            status,
                        }
                    })
                    .collect(),
            )
        })?)
    }

    async fn status(ctx: &crate::context::Context) -> Result<(), MigrationError> {
        let ledger = ctx.backend.ledger();
        ledger.ensure().await?;

        let graph = Self::build_graph(&ledger.retrieve().await?)?;
        graph
            .forward_path(Some(graph.head()), graph.queue())
            .into_iter()
            .for_each(|Node { migration, .. }| {
                println!(
                    "Migration `{}`, status: `{}`",
                    migration.runner.name(),
                    migration.status,
                )
            });

        Ok(())
    }

    fn latest_revision() -> Result<Box<dyn MigrationTrait>, MigrationError> {
        Self::migrations()
            .into_iter()
            .max_by_key(|migration| migration.revision().date.to_owned())
            .ok_or_else(|| MigrationError::Missing("no migrations".into()))
    }

    async fn refresh(ctx: &crate::context::Context) -> Result<(), MigrationError> {
        Self::exec(ctx, None, None, Direction::Refresh).await
    }

    async fn reset(ctx: &crate::context::Context) -> Result<(), MigrationError> {
        Self::exec(ctx, None, None, Direction::Down).await
    }

    async fn up(ctx: &crate::context::Context, to: Option<String>) -> Result<(), MigrationError> {
        Self::exec(ctx, None, to.as_deref(), Direction::Up).await
    }

    async fn down(ctx: &crate::context::Context, to: Option<String>) -> Result<(), MigrationError> {
        Self::exec(ctx, None, to.as_deref(), Direction::Down).await
    }

    async fn exec(
        ctx: &crate::context::Context,
        from: Option<&str>,
        to: Option<&str>,
        direction: Direction,
    ) -> Result<(), MigrationError> {
        let ledger = ctx.backend.ledger();
        ledger.ensure().await?;

        let applied = ledger.retrieve().await?;
        let graph = Self::build_graph(&applied)?;
        let path = match direction {
            Direction::Up => graph.forward_path(
                Some(from.unwrap_or(graph.head())),
                to.unwrap_or(graph.queue()),
            ),
            Direction::Down => graph.backward_path(Some(graph.queue()), to),
            Direction::Refresh => graph.backward_path(Some(graph.queue()), None),
        };

        let iterator = path
            .into_iter()
            .filter(|Node { migration, .. }| match direction {
                Direction::Up => migration.status == MigrationStatus::Pending,
                Direction::Down | Direction::Refresh => {
                    migration.status == MigrationStatus::Applied
                }
            })
            .map(|Node { migration, .. }| (migration.id, migration.runner.as_ref()));

        match direction {
            Direction::Up => {
                run_up(ctx, iterator).await?;
            }
            Direction::Down => {
                run_down(ctx, iterator).await?;
            }
            Direction::Refresh => {
                let collected: Vec<_> = iterator.collect();
                run_down(ctx, collected.iter().cloned()).await?;
                run_up(ctx, collected.into_iter()).await?;
            }
        };

        Ok(())
    }
}

async fn run_down<'a, I>(ctx: &crate::context::Context, iterator: I) -> Result<(), MigrationError>
where
    I: Iterator<Item = (Option<Uuid>, &'a dyn MigrationTrait)> + Send,
{
    let ledger = ctx.backend.ledger();
    ledger.ensure().await?;

    let ids = futures::future::try_join_all(iterator.map(|(id_opt, migration)| async move {
        let id = id_opt.ok_or_else(|| {
            MigrationError::Graph(RevisionGraphError::NotFound(format!(
                "{:?}",
                migration.name()
            )))
        })?;
        migration.down(ctx).await.map(|_| id)
    }))
    .await?;

    if !ids.is_empty() {
        ledger.delete_many(ids).await?;
    }

    Ok(())
}

async fn run_up<'a, I>(ctx: &crate::context::Context, iterator: I) -> Result<(), MigrationError>
where
    I: Iterator<Item = (Option<Uuid>, &'a dyn MigrationTrait)> + Send,
{
    let ledger = ctx.backend.ledger();
    ledger.ensure().await?;

    let ids =
        futures::future::try_join_all(iterator.map(|(_, migration)| async move {
            migration.up(ctx).await.map(|_| migration.name())
        }))
        .await?;

    if !ids.is_empty() {
        ledger.insert_many(ids).await?;
    }

    Ok(())
}

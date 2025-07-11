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
    #[error("migration {0} missing")]
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
            .for_each(|revivion| {
                println!(
                    "Migration `{}`, status: `{}`",
                    revivion.migration.runner.name(),
                    revivion.migration.status,
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
                Some(from.unwrap_or(graph.queue())),
                to.unwrap_or(graph.head()),
            ),
            Direction::Down => graph.backward_path(Some(graph.head()), to),
            Direction::Refresh => graph.backward_path(Some(graph.head()), None),
        };

        let iterator = path
            .into_iter()
            .filter(|revision| match direction {
                Direction::Up => revision.migration.status == MigrationStatus::Pending,
                Direction::Down | Direction::Refresh => {
                    revision.migration.status == MigrationStatus::Applied
                }
            })
            .filter_map(|revision| graph.get(&revision.revision));

        match direction {
            Direction::Up | Direction::Refresh => {
                let ids = join_all(iterator.map(|(_, migration)| async move {
                    migration.up(ctx).await.map(|_| migration.name())
                }))
                .await
                .into_iter()
                .collect::<Result<Vec<_>, _>>()?;
                if !ids.is_empty() {
                    ledger.insert_many(ids).await?;
                }
            }
            Direction::Down => {
                let ids = join_all(iterator.map(|(id_opt, migration)| async move {
                    let id = id_opt.ok_or_else(|| {
                        MigrationError::Graph(RevisionGraphError::NotFound(format!(
                            "{:?}",
                            migration.name()
                        )))
                    })?;
                    migration.down(ctx).await.map(|_| id)
                }))
                .await
                .into_iter()
                .collect::<Result<Vec<_>, _>>()?;
                if !ids.is_empty() {
                    ledger.delete_many(ids).await?;
                }
            }
        };

        Ok(())
    }
}

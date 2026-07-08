use crate::{
    ContextError, MigrationTrait,
    revision::{Node, RevisionGraph, RevisionGraphError},
};
use once_cell::sync::OnceCell;
use owo_colors::OwoColorize;
use std::{collections::HashMap, io::IsTerminal};
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

        let use_colors = std::io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none();

        graph
            .forward_path(Some(graph.head()), graph.queue())
            .into_iter()
            .for_each(|Node { migration, .. }| {
                let status_str = match migration.status {
                    MigrationStatus::Applied => {
                        let text = "Applied";
                        if use_colors {
                            text.green().bold().to_string()
                        } else {
                            text.to_string()
                        }
                    }
                    MigrationStatus::Pending => {
                        let text = "Pending";
                        if use_colors {
                            text.yellow().bold().to_string()
                        } else {
                            text.to_string()
                        }
                    }
                };

                let message = migration
                    .runner
                    .revision()
                    .message
                    .map(|message| {
                        if use_colors {
                            format!(" — {}", message.dimmed().italic())
                        } else {
                            format!(" — {}", message)
                        }
                    })
                    .unwrap_or_default();

                let name = if use_colors {
                    migration.runner.name().blue().bold().to_string()
                } else {
                    migration.runner.name().to_string()
                };

                println!("{:<20} | {}{}", name, status_str, message);
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

    let use_colors = std::io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none();

    apply_down(ctx, &ledger, use_colors, iterator).await
}

// Roll migrations back sequentially in the order `iterator` yields, removing each
// from the ledger IMMEDIATELY after it succeeds. A mid-run failure therefore leaves
// durable partial progress and the next run resumes rather than replaying.
async fn apply_down<'a, L, I>(
    ctx: &crate::context::Context,
    ledger: &L,
    use_colors: bool,
    iterator: I,
) -> Result<(), MigrationError>
where
    L: LedgerTrait<Key = String, Value = Uuid>,
    I: Iterator<Item = (Option<Uuid>, &'a dyn MigrationTrait)> + Send,
{
    for (id_opt, migration) in iterator {
        let name = migration.name();

        let message = format!("Running down: {}", name);
        if use_colors {
            println!("{}", message.yellow().bold());
        } else {
            println!("{message}");
        }

        let id = id_opt.ok_or_else(|| {
            MigrationError::Graph(RevisionGraphError::NotFound(format!("{:?}", name)))
        })?;

        migration.down(ctx).await?;

        let message = format!("Rolled back: {}", name);
        ledger.delete_many(vec![id]).await?;
        if use_colors {
            println!("{}", message.green().bold());
        } else {
            println!("{message}");
        }
    }

    Ok(())
}

async fn run_up<'a, I>(ctx: &crate::context::Context, iterator: I) -> Result<(), MigrationError>
where
    I: Iterator<Item = (Option<Uuid>, &'a dyn MigrationTrait)> + Send,
{
    let ledger = ctx.backend.ledger();
    ledger.ensure().await?;

    let use_colors = std::io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none();

    apply_up(ctx, &ledger, use_colors, iterator).await
}

// Apply migrations sequentially in the DAG order `iterator` yields, recording each
// in the ledger IMMEDIATELY after it succeeds. A mid-run failure therefore leaves
// durable partial progress and the next run resumes at the failed migration.
async fn apply_up<'a, L, I>(
    ctx: &crate::context::Context,
    ledger: &L,
    use_colors: bool,
    iterator: I,
) -> Result<(), MigrationError>
where
    L: LedgerTrait<Key = String, Value = Uuid>,
    I: Iterator<Item = (Option<Uuid>, &'a dyn MigrationTrait)> + Send,
{
    for (_, migration) in iterator {
        let name = migration.name();

        let message = format!("Applying: {}", name);
        if use_colors {
            println!("{}", message.yellow().bold());
        } else {
            println!("{message}");
        }

        migration.up(ctx).await?;

        let message = format!("Applied: {}", name);
        ledger.insert_many(vec![name]).await?;
        if use_colors {
            println!("{}", message.green().bold());
        } else {
            println!("{message}");
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{MigrationMeta, Revision as RevisionMeta};
    use std::sync::{Arc, Mutex};
    use vectorctl_backend::generic::VectorBackendError;

    #[derive(Default)]
    struct MockLedger {
        inserted: Mutex<Vec<String>>,
        deleted: Mutex<Vec<Uuid>>,
    }

    #[async_trait::async_trait]
    impl LedgerTrait for MockLedger {
        type Key = String;
        type Value = Uuid;

        fn collection_name(&self) -> String {
            "_mock".into()
        }

        async fn ensure(&self) -> Result<(), VectorBackendError> {
            Ok(())
        }

        async fn retrieve(&self) -> Result<HashMap<Self::Key, Self::Value>, VectorBackendError> {
            Ok(HashMap::new())
        }

        async fn insert_many(&self, ids: Vec<Self::Key>) -> Result<(), VectorBackendError> {
            self.inserted.lock().unwrap().extend(ids);
            Ok(())
        }

        async fn delete_many(&self, ids: Vec<Self::Value>) -> Result<(), VectorBackendError> {
            self.deleted.lock().unwrap().extend(ids);
            Ok(())
        }
    }

    #[derive(Debug)]
    struct MockMigration {
        name: &'static str,
        fail: bool,
        calls: Arc<Mutex<Vec<String>>>,
    }

    // Record each up/down invocation into a shared log so tests can assert the exact
    // call sequence, not just the resulting ledger state.
    fn mock(name: &'static str, fail: bool, calls: &Arc<Mutex<Vec<String>>>) -> MockMigration {
        MockMigration {
            name,
            fail,
            calls: Arc::clone(calls),
        }
    }

    impl MigrationMeta for MockMigration {
        fn name(&self) -> String {
            self.name.to_string()
        }

        fn revision(&self) -> RevisionMeta<'_> {
            RevisionMeta {
                message: None,
                revision: self.name,
                down_revision: None,
                date: "2023-01-01",
            }
        }
    }

    #[async_trait::async_trait]
    impl MigrationTrait for MockMigration {
        async fn up(&self, _ctx: &crate::context::Context) -> Result<(), MigrationError> {
            self.calls.lock().unwrap().push(format!("up:{}", self.name));
            if self.fail {
                Err(MigrationError::Other(self.name.into()))
            } else {
                Ok(())
            }
        }

        async fn down(&self, _ctx: &crate::context::Context) -> Result<(), MigrationError> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("down:{}", self.name));
            if self.fail {
                Err(MigrationError::Other(self.name.into()))
            } else {
                Ok(())
            }
        }
    }

    // Build a Context offline without touching the network: `Qdrant::from_url(..).build()`
    // otherwise runs an eager, blocking ~5s compatibility health check, so skip it. The mock
    // ledger and mock migrations never use this client, so no RPC is ever issued.
    fn test_ctx() -> crate::context::Context {
        let client = qdrant_client::Qdrant::from_url("http://localhost:6334")
            .skip_compatibility_check()
            .build()
            .unwrap();
        crate::context::Context::new(vectorctl_backend::Qdrant::new_with_client(Arc::new(client)))
    }

    #[test]
    fn apply_up_records_sequentially_in_order() {
        let ctx = test_ctx();
        let ledger = MockLedger::default();
        let calls = Arc::new(Mutex::new(Vec::new()));
        let migrations = [
            mock("a", false, &calls),
            mock("b", false, &calls),
            mock("c", false, &calls),
        ];
        let iter = migrations.iter().map(|m| (None, m as &dyn MigrationTrait));

        let result = futures::executor::block_on(apply_up(&ctx, &ledger, false, iter));

        assert!(result.is_ok());
        assert_eq!(*calls.lock().unwrap(), vec!["up:a", "up:b", "up:c"]);
        assert_eq!(*ledger.inserted.lock().unwrap(), vec!["a", "b", "c"]);
    }

    #[test]
    fn apply_up_stops_and_persists_prefix_on_failure() {
        let ctx = test_ctx();
        let ledger = MockLedger::default();
        let calls = Arc::new(Mutex::new(Vec::new()));
        let migrations = [
            mock("a", false, &calls),
            mock("b", false, &calls),
            mock("c", true, &calls),
            mock("d", false, &calls),
        ];
        let iter = migrations.iter().map(|m| (None, m as &dyn MigrationTrait));

        let result = futures::executor::block_on(apply_up(&ctx, &ledger, false, iter));

        assert!(result.is_err());
        // Ran up through the failing "c" in order and never invoked "d".
        assert_eq!(*calls.lock().unwrap(), vec!["up:a", "up:b", "up:c"]);
        // Prefix persisted; the run stopped at the failure so "d" never ran.
        assert_eq!(*ledger.inserted.lock().unwrap(), vec!["a", "b"]);
    }

    #[test]
    fn apply_down_stops_and_persists_prefix_on_failure() {
        let ctx = test_ctx();
        let ledger = MockLedger::default();
        let calls = Arc::new(Mutex::new(Vec::new()));
        let id_a = Uuid::now_v7();
        let id_b = Uuid::now_v7();
        let id_c = Uuid::now_v7();
        let migrations = [
            mock("a", false, &calls),
            mock("b", false, &calls),
            mock("c", true, &calls),
        ];
        let ids = [Some(id_a), Some(id_b), Some(id_c)];
        let iter = migrations
            .iter()
            .zip(ids)
            .map(|(m, id)| (id, m as &dyn MigrationTrait));

        let result = futures::executor::block_on(apply_down(&ctx, &ledger, false, iter));

        assert!(result.is_err());
        assert_eq!(*calls.lock().unwrap(), vec!["down:a", "down:b", "down:c"]);
        assert_eq!(*ledger.deleted.lock().unwrap(), vec![id_a, id_b]);
    }

    #[test]
    fn apply_down_missing_id_errors() {
        let ctx = test_ctx();
        let ledger = MockLedger::default();
        let calls = Arc::new(Mutex::new(Vec::new()));
        let migrations = [mock("a", false, &calls)];
        let iter = migrations.iter().map(|m| (None, m as &dyn MigrationTrait));

        let result = futures::executor::block_on(apply_down(&ctx, &ledger, false, iter));

        assert!(matches!(result, Err(MigrationError::Graph(_))));
        assert!(ledger.deleted.lock().unwrap().is_empty());
    }
}

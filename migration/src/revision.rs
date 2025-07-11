use crate::{MigrationTrait, migrator::Migration};
use rustc_hash::FxHashMap as HashMap;
use std::sync::Arc;
use tinyvec::ArrayVec;
use uuid::Uuid;

type Revision = Arc<str>;
type Ix = usize;

#[derive(Debug, thiserror::Error)]
pub enum RevisionGraphError {
    #[error("{0} not found")]
    NotFound(String),
}

#[derive(Debug)]
pub struct Node {
    /// revision ID
    pub revision: Revision,
    /// child indices
    pub children: ArrayVec<[Ix; 4]>,
    /// migration for this revision
    pub migration: Migration,
    /// parent index
    pub parent: Option<Ix>,
}

#[derive(Debug)]
pub struct RevisionGraph {
    nodes: Vec<Node>,
    index: HashMap<Revision, Ix>,
    /// index of the latest revision
    head_ix: Ix,
}

impl RevisionGraph {
    pub fn try_from(migrations: Vec<Migration>) -> Result<Self, RevisionGraphError> {
        let capacity = migrations.len();
        let mut nodes = Vec::with_capacity(capacity);
        let mut index = HashMap::with_capacity_and_hasher(capacity, Default::default());
        migrations
            .into_iter()
            .enumerate()
            .for_each(|(ix, migration)| {
                let meta = migration.runner.revision();
                let revision: Revision = Arc::from(meta.revision);
                index.insert(revision.clone(), ix);
                nodes.push(Node {
                    revision,
                    migration,
                    parent: None,
                    children: ArrayVec::new(),
                });
            });
        (0..nodes.len()).for_each(|ix| {
            if let Some(parent_rev) = nodes[ix].migration.runner.revision().down_revision {
                if let Some(&p_ix) = index.get(parent_rev) {
                    nodes[ix].parent = Some(p_ix);
                    nodes[p_ix].children.push(ix);
                }
            }
        });
        let head_ix = nodes
            .iter()
            .position(|n| n.children.is_empty())
            .ok_or_else(|| RevisionGraphError::NotFound("head".into()))?;

        Ok(Self {
            nodes,
            index,
            head_ix,
        })
    }

    fn ix(&self, rev: &str) -> Option<Ix> {
        self.index.get(rev).copied()
    }

    fn child_ix(&self, ix: Ix) -> Option<Ix> {
        self.nodes[ix].children.first().copied()
    }

    fn parent_ix(&self, ix: Ix) -> Option<Ix> {
        self.nodes[ix].parent
    }

    pub fn head(&self) -> &str {
        self.nodes[self.head_ix].revision.as_ref()
    }

    pub fn queue(&self) -> &str {
        self.nodes[0].revision.as_ref()
    }

    pub fn forward_path(&self, current: Option<&str>, target: &str) -> Vec<&Node> {
        let start_ix = current.and_then(|rev| self.ix(rev));
        let target_ix = self.ix(target).expect("target revision must exist");

        std::iter::successors(start_ix, |&ix| self.child_ix(ix))
            .take_while(|&ix| ix != target_ix)
            .chain(std::iter::once(target_ix))
            .map(|ix| &self.nodes[ix])
            .collect()
    }

    pub fn backward_path(&self, current: Option<&str>, stop: Option<&str>) -> Vec<&Node> {
        let stop_ix = stop.and_then(|r| self.ix(r));
        let start_ix = current.and_then(|r| self.ix(r));

        std::iter::successors(start_ix, |&ix| self.parent_ix(ix))
            .take_while(|&ix| Some(ix) != stop_ix)
            .map(|ix| &self.nodes[ix])
            .collect()
    }

    pub fn get(&self, rev: &str) -> Option<(Option<Uuid>, &dyn MigrationTrait)> {
        self.ix(rev).map(|ix| {
            let node = &self.nodes[ix];
            (node.migration.id, node.migration.runner.as_ref())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MigrationError;
    use crate::MigrationMeta;
    use crate::Revision as RevisionMeta;
    use crate::migrator::MigrationStatus;

    use uuid::Uuid;

    #[derive(Debug)]
    struct TestMigration {
        rev: &'static str,
        down_rev: Option<&'static str>,
    }

    impl MigrationMeta for TestMigration {
        fn name(&self) -> String {
            self.rev.to_string()
        }

        fn revision(&self) -> RevisionMeta<'_> {
            RevisionMeta {
                message: None,
                revision: self.rev,
                down_revision: self.down_rev,
                date: "2023-01-01",
            }
        }
    }

    #[async_trait::async_trait]
    impl MigrationTrait for TestMigration {
        async fn up(&self, _ctx: &crate::context::Context) -> Result<(), MigrationError> {
            Ok(())
        }

        async fn down(&self, _ctx: &crate::context::Context) -> Result<(), MigrationError> {
            Ok(())
        }
    }

    fn make_migration(
        rev: &'static str,
        down_rev: Option<&'static str>,
        status: Option<MigrationStatus>,
    ) -> Migration {
        Migration {
            id: Some(Uuid::now_v7()),
            status: status.unwrap_or(MigrationStatus::Pending),
            runner: Box::new(TestMigration { rev, down_rev }),
        }
    }

    #[test]
    fn test_graph_construction_and_paths() {
        let migrations = vec![
            make_migration("a", None, None),
            make_migration("b", Some("a"), None),
            make_migration("c", Some("b"), None),
        ];

        let graph = RevisionGraph::try_from(migrations).expect("graph should be created");

        assert_eq!(graph.queue(), "a");
        assert_eq!(graph.head(), "c");

        let forward = graph.forward_path(Some("a"), "c");
        assert_eq!(
            forward
                .iter()
                .map(|r| r.revision.as_ref())
                .collect::<Vec<_>>(),
            vec!["a", "b", "c"]
        );

        let backward = graph.backward_path(Some("c"), Some("a"));
        assert_eq!(
            backward
                .iter()
                .map(|r| r.revision.as_ref())
                .collect::<Vec<_>>(),
            vec!["c", "b"]
        );

        let (id, migration) = graph.get("b").expect("should find 'b'");
        assert!(id.is_some());
        assert_eq!(migration.revision().revision, "b");
    }

    #[test]
    fn test_graph_contruction_with_applied_revision() {
        let migrations = vec![make_migration("a", None, Some(MigrationStatus::Applied))];

        let graph = RevisionGraph::try_from(migrations).expect("graph should be created");

        assert_eq!(graph.head(), "a");
        assert_eq!(graph.queue(), "a");

        let forward = graph.forward_path(None, "a");
        assert_eq!(
            forward
                .iter()
                .map(|r| r.revision.as_ref())
                .collect::<Vec<_>>(),
            vec!["a"]
        );

        let backward = graph.backward_path(Some("a"), None);
        assert_eq!(
            backward
                .iter()
                .map(|r| r.revision.as_ref())
                .collect::<Vec<_>>(),
            vec!["a"]
        );
    }

    #[test]
    fn test_no_head_error() {
        let migrations = vec![
            make_migration("a", Some("c"), None),
            make_migration("b", Some("a"), None),
            make_migration("c", Some("b"), None),
        ];

        let graph = RevisionGraph::try_from(migrations);
        assert!(matches!(graph, Err(RevisionGraphError::NotFound(_))));
    }
}

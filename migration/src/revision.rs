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
struct Node {
    /// revision ID
    revision: Revision,
    /// child indices
    children: ArrayVec<[Ix; 4]>,
    /// migration for this revision
    migration: Arc<dyn MigrationTrait>,
    /// backend UUID, if persisted
    id: Option<Uuid>,
    /// parent index
    parent: Option<Ix>,
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
        migrations.into_iter().enumerate().for_each(|(ix, m)| {
            let meta = m.migration.revision();
            let revision: Revision = Arc::from(meta.revision);
            index.insert(revision.clone(), ix);
            nodes.push(Node {
                revision,
                id: m.id,
                migration: Arc::from(m.migration),
                parent: None,
                children: ArrayVec::new(),
            });
        });
        (0..nodes.len()).for_each(|ix| {
            if let Some(parent_rev) = nodes[ix].migration.revision().down_revision {
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

    pub fn forward_path(&self, current: Option<&str>, target: &str) -> Vec<Revision> {
        let start_ix = current.and_then(|rev| self.ix(rev));
        let target_ix = self.ix(target).expect("target revision must exist");

        std::iter::successors(start_ix, |&ix| self.child_ix(ix))
            .take_while(|&ix| ix != target_ix)
            .chain(std::iter::once(target_ix))
            .map(|ix| self.nodes[ix].revision.clone())
            .collect()
    }

    pub fn backward_path(&self, current: Option<&str>, stop: Option<&str>) -> Vec<Revision> {
        let stop_ix = stop.and_then(|r| self.ix(r));
        let start_ix = current.and_then(|r| self.ix(r));

        std::iter::successors(start_ix, |&ix| self.parent_ix(ix))
            .take_while(|&ix| Some(ix) != stop_ix)
            .map(|ix| self.nodes[ix].revision.clone())
            .collect()
    }

    pub fn get(&self, rev: &str) -> Option<(Option<Uuid>, &Arc<dyn MigrationTrait>)> {
        self.ix(rev).map(|ix| {
            let n = &self.nodes[ix];
            (n.id, &n.migration)
        })
    }
}

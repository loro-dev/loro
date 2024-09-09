use std::{cmp::Ordering, sync::Arc};

use loro_internal::{
    change::{Change, Lamport, Timestamp},
    id::ID,
    version::Frontiers,
};

/// `Change` is a grouped continuous operations that share the same id, timestamp, commit message.
///
/// - The id of the `Change` is the id of its first op.
/// - The second op's id is `{ peer: change.id.peer, counter: change.id.counter + 1 }`
///
/// The same applies on `Lamport`:
///
/// - The lamport of the `Change` is the lamport of its first op.
/// - The second op's lamport is `change.lamport + 1`
///
/// The length of the `Change` is how many operations it contains
#[derive(Debug, Clone)]
pub struct ChangeMeta {
    pub lamport: Lamport,
    pub id: ID,
    pub timestamp: Timestamp,
    pub message: Option<Arc<str>>,
    pub deps: Frontiers,
    pub len: usize,
}

impl PartialEq for ChangeMeta {
    fn eq(&self, other: &Self) -> bool {
        self.lamport == other.lamport && self.id == other.id
    }
}

impl Eq for ChangeMeta {}

impl PartialOrd for ChangeMeta {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ChangeMeta {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.lamport + self.len as Lamport)
            .cmp(&(other.lamport + other.len as Lamport))
            .then(self.id.peer.cmp(&other.id.peer))
    }
}

impl ChangeMeta {
    pub(super) fn from_change(c: &Change) -> Self {
        Self {
            id: c.id(),
            lamport: c.lamport(),
            timestamp: c.timestamp(),
            message: c.message().cloned(),
            deps: c.deps().clone(),
            len: c.len(),
        }
    }

    pub fn message(&self) -> &str {
        match self.message.as_ref() {
            Some(m) => m,
            None => "",
        }
    }
}

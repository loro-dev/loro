use bytes::Bytes;
use loro_common::{Counter, CounterSpan, IdLp, Lamport, LoroResult, PeerID, ID};
use num::iter::Range;
use std::{cmp::Ordering, collections::BTreeMap};
mod block_encode;
mod delta_rle_encode;
mod ops_encode;
use crate::{arena::SharedArena, change::Change, version::Frontiers};

#[derive(Debug)]
pub struct ChangeStore {
    kv: BTreeMap<Bytes, ChangesBlock>,
}

#[derive(Debug)]
pub struct ChangesBlock {
    arena: SharedArena,
    peer: PeerID,
    counter_range: (Counter, Counter),
    lamport_range: (Lamport, Lamport),
    content: ChangesBlockContent,
}

#[derive(thiserror::Error, Debug)]
pub enum ChangesBlockError {
    #[error("Invalid changes block bytes")]
    DecodeError,
}

impl ChangesBlock {
    pub fn from_bytes(bytes: Bytes, arena: SharedArena) -> Self {
        let bytes = ChangesBlockBytes::new(bytes);
        let peer = bytes.peer();
        let counter_range = bytes.counter_range();
        let lamport_range = bytes.lamport_range();
        let content = ChangesBlockContent::Bytes(bytes);
        Self {
            arena,
            peer,
            counter_range,
            lamport_range,
            content,
        }
    }

    pub fn cmp_id(&self, id: ID) -> Ordering {
        self.peer.cmp(&id.peer).then_with(|| {
            if self.counter_range.0 > id.counter {
                Ordering::Greater
            } else if self.counter_range.1 <= id.counter {
                Ordering::Less
            } else {
                Ordering::Equal
            }
        })
    }

    pub fn cmp_idlp(&self, idlp: (PeerID, Lamport)) -> Ordering {
        self.peer.cmp(&idlp.0).then_with(|| {
            if self.lamport_range.0 > idlp.1 {
                Ordering::Greater
            } else if self.lamport_range.1 <= idlp.1 {
                Ordering::Less
            } else {
                Ordering::Equal
            }
        })
    }
}

enum ChangesBlockContent {
    Changes(Vec<Change>),
    Bytes(ChangesBlockBytes),
    Both(Vec<Change>, ChangesBlockBytes),
}

impl ChangesBlockContent {
    pub fn changes(&mut self) -> Result<&Vec<Change>, ChangesBlockError> {
        match self {
            ChangesBlockContent::Changes(changes) => Ok(changes),
            ChangesBlockContent::Both(changes, _) => Ok(changes),
            ChangesBlockContent::Bytes(bytes) => {
                let changes = bytes.parse(&SharedArena::new())?;
                *self = ChangesBlockContent::Both(changes, bytes.clone());
                self.changes()
            }
        }
    }

    pub fn bytes(&mut self) -> &ChangesBlockBytes {
        match self {
            ChangesBlockContent::Bytes(bytes) => bytes,
            ChangesBlockContent::Both(_, bytes) => bytes,
            ChangesBlockContent::Changes(changes) => {
                let bytes = ChangesBlockBytes::serialize(changes, &SharedArena::new());
                *self = ChangesBlockContent::Both(std::mem::take(changes), bytes);
                self.bytes()
            }
        }
    }

    /// Note that this method will invalidate the stored bytes
    pub fn changes_mut(&mut self) -> Result<&mut Vec<Change>, ChangesBlockError> {
        match self {
            ChangesBlockContent::Changes(changes) => Ok(changes),
            ChangesBlockContent::Both(changes, _) => {
                *self = ChangesBlockContent::Changes(std::mem::take(changes));
                self.changes_mut()
            }
            ChangesBlockContent::Bytes(bytes) => {
                let changes = bytes.parse(&SharedArena::new())?;
                *self = ChangesBlockContent::Changes(changes);
                self.changes_mut()
            }
        }
    }
}

impl std::fmt::Debug for ChangesBlockContent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ChangesBlockContent::Changes(changes) => f
                .debug_tuple("ChangesBlockContent::Changes")
                .field(changes)
                .finish(),
            ChangesBlockContent::Bytes(_bytes) => {
                f.debug_tuple("ChangesBlockContent::Bytes").finish()
            }
            ChangesBlockContent::Both(changes, _bytes) => f
                .debug_tuple("ChangesBlockContent::Both")
                .field(changes)
                .finish(),
        }
    }
}

#[derive(Clone)]
struct ChangesBlockBytes {
    bytes: Bytes,
}

impl ChangesBlockBytes {
    fn new(bytes: Bytes) -> Self {
        Self { bytes }
    }

    fn parse(&self, a: &SharedArena) -> Result<Vec<Change>, ChangesBlockError> {
        unimplemented!()
    }

    fn serialize(changes: &[Change], a: &SharedArena) -> Self {
        unimplemented!()
    }

    fn peer(&self) -> PeerID {
        unimplemented!()
    }

    fn counter_range(&self) -> (Counter, Counter) {
        unimplemented!()
    }

    fn lamport_range(&self) -> (Lamport, Lamport) {
        unimplemented!()
    }

    /// Length of the changes
    fn len_changes(&self) -> usize {
        unimplemented!()
    }

    fn find_deps_for(&self, id: ID) -> Frontiers {
        unimplemented!()
    }
}

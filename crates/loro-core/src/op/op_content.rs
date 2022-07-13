use rle::{HasLength, RleVec, Sliceable};

use crate::{id::ID, id_span::IdSpan};

use super::InsertContent;

#[derive(Debug)]
pub enum OpContent {
    Insert {
        container: ID,
        content: Box<dyn InsertContent>,
    },
    Delete {
        target: RleVec<IdSpan>,
        lamport: usize,
    },
    Restore {
        target: RleVec<IdSpan>,
        lamport: usize,
    },
}

impl HasLength for OpContent {
    fn len(&self) -> usize {
        match self {
            OpContent::Insert { content, .. } => content.len(),
            OpContent::Delete { target, .. } => target.len(),
            OpContent::Restore { target, .. } => target.len(),
        }
    }
}

impl Clone for OpContent {
    fn clone(&self) -> Self {
        match self {
            OpContent::Insert { container, content } => OpContent::Insert {
                container: *container,
                content: content.clone_content(),
            },
            OpContent::Delete { target, lamport } => OpContent::Delete {
                target: target.clone(),
                lamport: *lamport,
            },
            OpContent::Restore { target, lamport } => OpContent::Restore {
                target: target.clone(),
                lamport: *lamport,
            },
        }
    }
}

impl Sliceable for OpContent {
    fn slice(&self, from: usize, to: usize) -> Self {
        match self {
            OpContent::Insert { container, content } => OpContent::Insert {
                container: *container,
                content: content.slice(from, to),
            },
            OpContent::Delete { target, lamport } => OpContent::Delete {
                target: target.slice(from, to),
                lamport: lamport + from,
            },
            OpContent::Restore { target, lamport } => OpContent::Restore {
                target: target.slice(from, to),
                lamport: lamport + from,
            },
        }
    }
}

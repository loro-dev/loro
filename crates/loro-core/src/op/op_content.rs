use rle::{HasLength, RleVec, Sliceable};

use crate::{id::ID, id_span::IdSpan};

use super::InsertContent;

#[derive(Debug, Clone)]
pub(crate) enum OpContent {
    Insert {
        container: ID,
        content: Box<InsertContent>,
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

impl Sliceable for OpContent {
    fn slice(&self, from: usize, to: usize) -> Self {
        match self {
            OpContent::Insert { container, content } => OpContent::Insert {
                container: *container,
                content: Box::new(content.slice(from, to)),
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

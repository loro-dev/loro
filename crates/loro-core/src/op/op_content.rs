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
    },
    Restore {
        target: RleVec<IdSpan>,
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
            OpContent::Delete { target } => OpContent::Delete {
                target: target.clone(),
            },
            OpContent::Restore { target } => OpContent::Restore {
                target: target.clone(),
            },
        }
    }
}

impl Sliceable for OpContent {
    fn slice(&self, from: usize, to: usize) -> Self {
        match self {
            OpContent::Insert { container, content } => OpContent::Insert {
                container: *container,
                content: content.slice_content(from, to),
            },
            OpContent::Delete { target } => OpContent::Delete {
                target: target.slice(from, to),
            },
            OpContent::Restore { target } => OpContent::Restore {
                target: target.slice(from, to),
            },
        }
    }
}

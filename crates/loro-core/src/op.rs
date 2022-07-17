use crate::{container::ContainerID, id::ID, id_span::IdSpan};
use rle::{HasLength, Mergable, RleVec, Sliceable};
mod insert_content;
mod op_content;
mod op_proxy;

pub use insert_content::*;
pub use op_content::*;
pub use op_proxy::*;

#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpType {
    Insert,
    Delete,
    Restore,
}

#[derive(Debug, Clone)]
/// Operation is a unit of change.
///
/// It has 3 types:
/// - Insert
/// - Delete
/// - Restore
///
/// A Op may have multiple atomic operations, since Op can be merged.
pub struct Op {
    pub(crate) id: ID,
    pub(crate) content: OpContent,
}

impl Op {
    pub fn new(id: ID, content: OpContent) -> Self {
        Op { id, content }
    }

    pub fn op_type(&self) -> OpType {
        match self.content {
            OpContent::Insert { .. } => OpType::Insert,
            OpContent::Delete { .. } => OpType::Delete,
            OpContent::Restore { .. } => OpType::Restore,
        }
    }

    #[allow(clippy::borrowed_box)]
    pub fn content(&self) -> &Box<dyn InsertContent> {
        match &self.content {
            OpContent::Insert { content, .. } => content,
            _ => unreachable!(),
        }
    }

    pub fn container(&self) -> &ContainerID {
        match &self.content {
            OpContent::Insert { container, .. } => container,
            _ => unreachable!(),
        }
    }
}

impl Mergable for Op {
    fn is_mergable(&self, other: &Self, cfg: &()) -> bool {
        match &self.content {
            OpContent::Insert { container, content } => match other.content {
                OpContent::Insert {
                    container: ref other_container,
                    content: ref other_content,
                } => container == other_container && content.is_mergable_content(&**other_content),
                _ => false,
            },
            OpContent::Delete { target } => match other.content {
                OpContent::Delete {
                    target: ref other_target,
                } => target.is_mergable(other_target, cfg),
                _ => false,
            },
            OpContent::Restore { target } => match other.content {
                OpContent::Restore {
                    target: ref other_target,
                } => target.is_mergable(other_target, cfg),
                _ => false,
            },
        }
    }

    fn merge(&mut self, other: &Self, cfg: &()) {
        match &mut self.content {
            OpContent::Insert { container, content } => match &other.content {
                OpContent::Insert {
                    container: other_container,
                    content: other_content,
                } => {
                    assert_eq!(container, other_container);
                    content.merge_content(&**other_content);
                }
                _ => unreachable!(),
            },
            OpContent::Delete { target, .. } => match &other.content {
                OpContent::Delete {
                    target: other_target,
                    ..
                } => target.merge(other_target, cfg),
                _ => unreachable!(),
            },
            OpContent::Restore { target, .. } => match &other.content {
                OpContent::Restore {
                    target: other_target,
                    ..
                } => target.merge(other_target, cfg),
                _ => unreachable!(),
            },
        }
    }
}

impl HasLength for Op {
    fn len(&self) -> usize {
        self.content.len()
    }
}

impl Sliceable for Op {
    fn slice(&self, from: usize, to: usize) -> Self {
        assert!(to > from);
        let content: OpContent = self.content.slice(from, to);
        Op {
            id: ID {
                client_id: self.id.client_id,
                counter: (self.id.counter + from as u32),
            },
            content,
        }
    }
}

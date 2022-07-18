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
    Normal,
    Undo,
    Redo,
}

/// Operation is a unit of change.
///
/// It has 3 types:
/// - Insert
/// - Delete
/// - Restore
///
/// A Op may have multiple atomic operations, since Op can be merged.
#[derive(Debug, Clone)]
pub struct Op {
    pub(crate) id: ID,
    pub(crate) content: OpContent,
}

impl Op {
    #[inline]
    pub fn new(id: ID, content: OpContent) -> Self {
        Op { id, content }
    }

    #[inline]
    pub fn new_insert_op(id: ID, container: ContainerID, content: Box<dyn InsertContent>) -> Self {
        Op::new(id, OpContent::Normal { container, content })
    }

    #[inline]
    pub fn new_delete_op(id: ID, container: ContainerID, target: RleVec<IdSpan>) -> Self {
        Op::new(id, OpContent::Undo { container, target })
    }

    pub fn op_type(&self) -> OpType {
        match self.content {
            OpContent::Normal { .. } => OpType::Normal,
            OpContent::Undo { .. } => OpType::Undo,
            OpContent::Redo { .. } => OpType::Redo,
        }
    }

    pub fn container(&self) -> &ContainerID {
        match &self.content {
            OpContent::Normal { container, .. } => container,
            _ => unreachable!(),
        }
    }

    #[allow(clippy::borrowed_box)]
    pub fn insert_content(&self) -> &Box<dyn InsertContent> {
        match &self.content {
            OpContent::Normal { content, .. } => content,
            _ => unreachable!(),
        }
    }
}

impl Mergable for Op {
    fn is_mergable(&self, other: &Self, cfg: &()) -> bool {
        self.id.is_connected_id(&other.id, self.len() as u32)
            && self.content.is_mergable(&other.content, cfg)
    }

    fn merge(&mut self, other: &Self, cfg: &()) {
        match &mut self.content {
            OpContent::Normal { container, content } => match &other.content {
                OpContent::Normal {
                    container: other_container,
                    content: other_content,
                } => {
                    assert_eq!(container, other_container);
                    content.merge_content(&**other_content);
                }
                _ => unreachable!(),
            },
            OpContent::Undo { target, .. } => match &other.content {
                OpContent::Undo {
                    target: other_target,
                    ..
                } => target.merge(other_target, cfg),
                _ => unreachable!(),
            },
            OpContent::Redo { target, .. } => match &other.content {
                OpContent::Redo {
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

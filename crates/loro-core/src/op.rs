use crate::{
    container::ContainerID,
    id::{Counter, ID},
    span::IdSpan,
};
use rle::{HasLength, Mergable, RleVec, Sliceable};
mod insert_content;
mod op_content;
mod op_proxy;

pub use insert_content::*;
pub use op_proxy::*;

pub(crate) use self::op_content::OpContent;

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
    pub(crate) container: ContainerID,
    pub(crate) content: OpContent,
}

impl Op {
    #[inline]
    pub(crate) fn new(id: ID, content: OpContent, container: ContainerID) -> Self {
        Op {
            id,
            content,
            container,
        }
    }

    #[inline]
    pub(crate) fn new_insert_op(id: ID, container: ContainerID, content: InsertContent) -> Self {
        Op::new(id, OpContent::Normal { content }, container)
    }

    #[inline]
    pub fn new_delete_op(id: ID, container: ContainerID, target: RleVec<IdSpan>) -> Self {
        Op::new(id, OpContent::Undo { target }, container)
    }

    pub fn op_type(&self) -> OpType {
        match self.content {
            OpContent::Normal { .. } => OpType::Normal,
            OpContent::Undo { .. } => OpType::Undo,
            OpContent::Redo { .. } => OpType::Redo,
        }
    }

    pub fn container(&self) -> &ContainerID {
        &self.container
    }
}

impl Mergable for Op {
    fn is_mergable(&self, other: &Self, cfg: &()) -> bool {
        self.id.is_connected_id(&other.id, self.len())
            && self.content.is_mergable(&other.content, cfg)
            && self.container == other.container
    }

    fn merge(&mut self, other: &Self, cfg: &()) {
        match &mut self.content {
            OpContent::Normal { content } => match &other.content {
                OpContent::Normal {
                    content: other_content,
                } => {
                    content.merge(other_content, cfg);
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
                counter: (self.id.counter + from as Counter),
            },
            content,
            container: self.container.clone(),
        }
    }
}

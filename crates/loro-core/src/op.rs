use crate::{
    change::{Lamport, Timestamp},
    container::ContainerID,
    id::{Counter, ID},
    span::HasId,
};
use rle::{HasIndex, HasLength, Mergable, Sliceable};
mod insert_content;
mod op_content;

pub use insert_content::*;

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
        self.id.is_connected_id(&other.id, self.content_len())
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
    fn content_len(&self) -> usize {
        self.content.content_len()
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

impl HasId for Op {
    fn id_start(&self) -> ID {
        self.id
    }
}

pub struct RichOp<'a> {
    pub op: &'a Op,
    pub lamport: Lamport,
    pub timestamp: Timestamp,
}

impl HasIndex for Op {
    type Int = Counter;

    fn get_start_index(&self) -> Self::Int {
        self.id.counter
    }
}

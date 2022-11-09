use crate::{
    change::{Lamport, Timestamp},
    container::ContainerID,
    id::{ContainerIdx, Counter, ID},
    span::HasCounter,
    LogStore,
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
    pub(crate) counter: Counter,
    pub(crate) container: ContainerIdx,
    pub(crate) content: OpContent,
}

#[derive(Debug, Clone)]
pub struct RemoteOp {
    pub(crate) counter: Counter,
    pub(crate) container: ContainerID,
    pub(crate) content: OpContent,
}

impl Op {
    #[inline]
    pub(crate) fn new(id: ID, content: OpContent, container: u32) -> Self {
        Op {
            counter: id.counter,
            content,
            container,
        }
    }

    #[inline]
    pub(crate) fn new_insert_op(id: ID, container: u32, content: InsertContent) -> Self {
        Op::new(id, OpContent::Normal { content }, container)
    }

    pub fn op_type(&self) -> OpType {
        match self.content {
            OpContent::Normal { .. } => OpType::Normal,
            OpContent::Undo { .. } => OpType::Undo,
            OpContent::Redo { .. } => OpType::Redo,
        }
    }

    pub(crate) fn convert(self, log: &LogStore) -> RemoteOp {
        let container = log.get_container_id(self.container).clone();
        RemoteOp {
            counter: self.counter,
            container,
            content: self.content,
        }
    }
}

impl RemoteOp {
    pub(crate) fn convert(self, log: &mut LogStore) -> Op {
        let container = log.get_or_create_container_idx(&self.container);
        let content = self.content;
        Op {
            counter: self.counter,
            container,
            content,
        }
    }
}

impl Mergable for Op {
    fn is_mergable(&self, other: &Self, cfg: &()) -> bool {
        self.counter + self.content_len() as Counter == other.counter
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
            counter: (self.counter + from as Counter),
            content,
            container: self.container,
        }
    }
}

impl Mergable for RemoteOp {
    fn is_mergable(&self, other: &Self, cfg: &()) -> bool {
        self.counter + self.content_len() as Counter == other.counter
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

impl HasLength for RemoteOp {
    fn content_len(&self) -> usize {
        self.content.content_len()
    }
}

impl Sliceable for RemoteOp {
    fn slice(&self, from: usize, to: usize) -> Self {
        assert!(to > from);
        let content: OpContent = self.content.slice(from, to);
        RemoteOp {
            counter: (self.counter + from as Counter),
            content,
            container: self.container.clone(),
        }
    }
}

pub struct RichOp<'a> {
    pub op: &'a Op,
    pub lamport: Lamport,
    pub timestamp: Timestamp,
    pub start: usize,
    pub end: usize,
}

impl<'a> RichOp<'a> {
    pub fn get_sliced(&self) -> Op {
        self.op.slice(self.start, self.end)
    }
}

impl HasIndex for Op {
    type Int = Counter;

    fn get_start_index(&self) -> Self::Int {
        self.counter
    }
}

impl HasIndex for RemoteOp {
    type Int = Counter;

    fn get_start_index(&self) -> Self::Int {
        self.counter
    }
}

impl HasCounter for Op {
    fn ctr_start(&self) -> Counter {
        self.counter
    }
}

impl HasCounter for RemoteOp {
    fn ctr_start(&self) -> Counter {
        self.counter
    }
}

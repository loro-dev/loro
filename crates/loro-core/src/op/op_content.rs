use rle::{HasLength, Mergable, RleVec, Sliceable};

use crate::{
    container::ContainerID,
    id::ID,
    span::{CounterSpan, IdSpan},
    OpType,
};

use super::{InsertContent, MergeableContent};

#[derive(Debug)]
pub enum OpContent {
    Normal { content: Box<dyn InsertContent> },
    Undo { target: RleVec<CounterSpan> },
    Redo { target: RleVec<CounterSpan> },
}

impl OpContent {
    pub fn op_type(&self) -> OpType {
        match self {
            OpContent::Normal { .. } => OpType::Normal,
            OpContent::Undo { .. } => OpType::Undo,
            OpContent::Redo { .. } => OpType::Redo,
        }
    }
}

impl HasLength for OpContent {
    fn len(&self) -> usize {
        match self {
            OpContent::Normal { content, .. } => content.len(),
            OpContent::Undo { target, .. } => target.len(),
            OpContent::Redo { target, .. } => target.len(),
        }
    }
}

impl Clone for OpContent {
    fn clone(&self) -> Self {
        match self {
            OpContent::Normal { content } => OpContent::Normal {
                content: content.clone_content(),
            },
            OpContent::Undo { target } => OpContent::Undo {
                target: target.clone(),
            },
            OpContent::Redo { target } => OpContent::Redo {
                target: target.clone(),
            },
        }
    }
}

impl Sliceable for OpContent {
    fn slice(&self, from: usize, to: usize) -> Self {
        match self {
            OpContent::Normal { content } => OpContent::Normal {
                content: content.slice_content(from, to),
            },
            OpContent::Undo { target } => OpContent::Undo {
                target: target.slice(from, to),
            },
            OpContent::Redo { target } => OpContent::Redo {
                target: target.slice(from, to),
            },
        }
    }
}

impl Mergable for OpContent {
    fn is_mergable(&self, other: &Self, cfg: &()) -> bool
    where
        Self: Sized,
    {
        match &self {
            OpContent::Normal { content } => match other {
                OpContent::Normal {
                    content: ref other_content,
                } => content.is_mergable_content(&**other_content),
                _ => false,
            },
            OpContent::Undo { target } => match other {
                OpContent::Undo {
                    target: ref other_target,
                } => target.is_mergable(other_target, cfg),
                _ => false,
            },
            OpContent::Redo { target } => match other {
                OpContent::Redo {
                    target: ref other_target,
                } => target.is_mergable(other_target, cfg),
                _ => false,
            },
        }
    }

    fn merge(&mut self, _other: &Self, _conf: &())
    where
        Self: Sized,
    {
    }
}

use enum_as_inner::EnumAsInner;
use rle::{HasLength, Mergable, RleVecWithIndex, Sliceable};

use crate::{span::IdSpan, OpType};

use super::InsertContent;

#[derive(Debug, EnumAsInner)]
pub(crate) enum OpContent {
    Normal { content: InsertContent },
    Undo { target: RleVecWithIndex<IdSpan> },
    Redo { target: RleVecWithIndex<IdSpan> },
}

impl OpContent {
    #[inline]
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
                content: content.clone(),
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
                content: content.slice(from, to),
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
                } => content.is_mergable(other_content, cfg),
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

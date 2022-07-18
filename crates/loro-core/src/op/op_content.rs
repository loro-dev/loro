use rle::{HasLength, Mergable, RleVec, Sliceable};

use crate::{container::ContainerID, id::ID, id_span::IdSpan, OpType};

use super::{InsertContent, MergeableContent};

#[derive(Debug)]
pub enum OpContent {
    Normal {
        container: ContainerID,
        content: Box<dyn InsertContent>,
    },
    Undo {
        container: ContainerID,
        target: RleVec<IdSpan>,
    },
    Redo {
        container: ContainerID,
        target: RleVec<IdSpan>,
    },
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
            OpContent::Normal { container, content } => OpContent::Normal {
                container: container.clone(),
                content: content.clone_content(),
            },
            OpContent::Undo { target, container } => OpContent::Undo {
                container: container.clone(),
                target: target.clone(),
            },
            OpContent::Redo { target, container } => OpContent::Redo {
                container: container.clone(),
                target: target.clone(),
            },
        }
    }
}

impl Sliceable for OpContent {
    fn slice(&self, from: usize, to: usize) -> Self {
        match self {
            OpContent::Normal { container, content } => OpContent::Normal {
                container: container.clone(),
                content: content.slice_content(from, to),
            },
            OpContent::Undo { target, container } => OpContent::Undo {
                container: container.clone(),
                target: target.slice(from, to),
            },
            OpContent::Redo { target, container } => OpContent::Redo {
                container: container.clone(),
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
            OpContent::Normal { container, content } => match other {
                OpContent::Normal {
                    container: ref other_container,
                    content: ref other_content,
                } => container == other_container && content.is_mergable_content(&**other_content),
                _ => false,
            },
            OpContent::Undo { target, container } => match other {
                OpContent::Undo {
                    target: ref other_target,
                    container: other_container,
                } => container == other_container && target.is_mergable(other_target, cfg),
                _ => false,
            },
            OpContent::Redo { target, container } => match other {
                OpContent::Redo {
                    target: ref other_target,
                    container: ref other_container,
                } => container == other_container && target.is_mergable(other_target, cfg),
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

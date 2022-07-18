use rle::{HasLength, Mergable, RleVec, Sliceable};

use crate::{container::ContainerID, id::ID, id_span::IdSpan, OpType};

use super::{InsertContent, MergeableContent};

#[derive(Debug)]
pub enum OpContent {
    Insert {
        container: ContainerID,
        content: Box<dyn InsertContent>,
    },
    Delete {
        container: ContainerID,
        target: RleVec<IdSpan>,
    },
    Restore {
        container: ContainerID,
        target: RleVec<IdSpan>,
    },
}

impl OpContent {
    pub fn op_type(&self) -> OpType {
        match self {
            OpContent::Insert { .. } => OpType::Insert,
            OpContent::Delete { .. } => OpType::Delete,
            OpContent::Restore { .. } => OpType::Restore,
        }
    }
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
                container: container.clone(),
                content: content.clone_content(),
            },
            OpContent::Delete { target, container } => OpContent::Delete {
                container: container.clone(),
                target: target.clone(),
            },
            OpContent::Restore { target, container } => OpContent::Restore {
                container: container.clone(),
                target: target.clone(),
            },
        }
    }
}

impl Sliceable for OpContent {
    fn slice(&self, from: usize, to: usize) -> Self {
        match self {
            OpContent::Insert { container, content } => OpContent::Insert {
                container: container.clone(),
                content: content.slice_content(from, to),
            },
            OpContent::Delete { target, container } => OpContent::Delete {
                container: container.clone(),
                target: target.slice(from, to),
            },
            OpContent::Restore { target, container } => OpContent::Restore {
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
            OpContent::Insert { container, content } => match other {
                OpContent::Insert {
                    container: ref other_container,
                    content: ref other_content,
                } => container == other_container && content.is_mergable_content(&**other_content),
                _ => false,
            },
            OpContent::Delete { target, container } => match other {
                OpContent::Delete {
                    target: ref other_target,
                    container: other_container,
                } => container == other_container && target.is_mergable(other_target, cfg),
                _ => false,
            },
            OpContent::Restore { target, container } => match other {
                OpContent::Restore {
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

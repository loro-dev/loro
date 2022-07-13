use crate::{id::ID, id_span::IdSpan};
use rle::{HasLength, Mergable, RleVec, Sliceable};
mod insert_content;
mod op_content;

pub use insert_content::*;
pub use op_content::*;

#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpType {
    Insert,
    Delete,
    Restore,
}

#[derive(Debug, Clone)]
pub struct Op {
    id: ID,
    content: OpContent,
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

    pub fn content(&self) -> &Box<dyn InsertContent> {
        match &self.content {
            OpContent::Insert { content, .. } => content,
            _ => unreachable!(),
        }
    }

    pub fn container(&self) -> &ID {
        match &self.content {
            OpContent::Insert { container, .. } => container,
            _ => unreachable!(),
        }
    }
}

impl Mergable for Op {
    fn is_mergable(&self, other: &Self) -> bool {
        match &self.content {
            OpContent::Insert { container, content } => match other.content {
                OpContent::Insert {
                    container: other_container,
                    content: ref other_content,
                } => container == &other_container && content.is_mergable(&**other_content),
                _ => false,
            },
            OpContent::Delete { target, lamport } => match other.content {
                OpContent::Delete {
                    target: ref other_target,
                    lamport: ref other_lamport,
                } => lamport + target.len() == *other_lamport && target.is_mergable(other_target),
                _ => false,
            },
            OpContent::Restore { target, lamport } => match other.content {
                OpContent::Restore {
                    target: ref other_target,
                    lamport: ref other_lamport,
                } => lamport + target.len() == *other_lamport && target.is_mergable(other_target),
                _ => false,
            },
        }
    }

    fn merge(&mut self, other: &Self) {
        match &mut self.content {
            OpContent::Insert { container, content } => match &other.content {
                OpContent::Insert {
                    container: other_container,
                    content: other_content,
                } => {
                    assert_eq!(container, other_container);
                    content.merge(&**other_content);
                }
                _ => unreachable!(),
            },
            OpContent::Delete { target, .. } => match &other.content {
                OpContent::Delete {
                    target: other_target,
                    ..
                } => target.merge(other_target),
                _ => unreachable!(),
            },
            OpContent::Restore { target, .. } => match &other.content {
                OpContent::Restore {
                    target: other_target,
                    ..
                } => target.merge(other_target),
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

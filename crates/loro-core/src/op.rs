use crate::{
    change::{Lamport, Timestamp},
    container::ContainerID,
    id::{ContainerIdx, Counter, ID},
    span::HasCounter,
    LogStore,
};
use rle::{HasIndex, HasLength, Mergable, RleVec, Sliceable};
mod insert_content;

pub use insert_content::*;
use smallvec::{smallvec, SmallVec};

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
    pub(crate) content: Content,
}

#[derive(Debug, Clone)]
pub struct RemoteOp {
    pub(crate) counter: Counter,
    pub(crate) container: ContainerID,
    pub(crate) contents: RleVec<[Content; 1]>,
}

impl Op {
    #[inline]
    pub(crate) fn new(id: ID, content: Content, container: u32) -> Self {
        Op {
            counter: id.counter,
            content,
            container,
        }
    }

    pub(crate) fn convert(self, log: &LogStore) -> RemoteOp {
        let container = log.reg.get_id(self.container).unwrap().clone();
        RemoteOp {
            counter: self.counter,
            container,
            contents: RleVec::from(smallvec![self.content]),
        }
    }
}

impl RemoteOp {
    pub(crate) fn convert(self, log: &mut LogStore) -> SmallVec<[Op; 1]> {
        let container = log.get_or_create_container_idx(&self.container);
        let mut counter = self.counter;
        self.contents
            .into_iter()
            .map(|content| {
                let ans = Op {
                    counter,
                    container,
                    content,
                };
                counter += ans.atom_len() as Counter;
                ans
            })
            .collect()
    }
}

impl Mergable for Op {
    fn is_mergable(&self, other: &Self, cfg: &()) -> bool {
        self.counter + self.content_len() as Counter == other.counter
            && self.container == other.container
            && self.content.is_mergable(&other.content, cfg)
    }

    fn merge(&mut self, other: &Self, cfg: &()) {
        self.content.merge(&other.content, cfg)
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
        let content: Content = self.content.slice(from, to);
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
            && other.contents.len() == 1
            && self
                .contents
                .last()
                .unwrap()
                .is_mergable(other.contents.first().unwrap(), cfg)
            && self.container == other.container
    }

    fn merge(&mut self, other: &Self, _: &()) {
        for content in other.contents.iter() {
            self.contents.push(content.clone())
        }
    }
}

impl HasLength for RemoteOp {
    fn content_len(&self) -> usize {
        self.contents.iter().map(|x| x.atom_len()).sum()
    }
}

impl Sliceable for RemoteOp {
    fn slice(&self, from: usize, to: usize) -> Self {
        assert!(to > from);
        RemoteOp {
            counter: (self.counter + from as Counter),
            contents: self.contents.slice(from, to),
            container: self.container.clone(),
        }
    }
}

/// RichOp includes lamport and timestamp info, which is used for conflict resolution.
///
/// `lamport` is the lamport of the returned op, to get the lamport of the sliced op, you need to use `lamport + start`
///
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

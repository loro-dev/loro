use crate::{
    change::{Change, Lamport, Timestamp},
    container::ContainerID,
    id::{ClientID, ContainerIdx, Counter, ID},
    span::{HasCounter, HasId, HasLamport},
    LogStore,
};
use rle::{HasIndex, HasLength, Mergable, RleVec, Sliceable};
mod content;

pub use content::*;
use smallvec::{smallvec, SmallVec};

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
    pub(crate) content: RemoteContent,
}

#[derive(Debug, Clone)]
pub struct RemoteOp {
    pub(crate) counter: Counter,
    pub(crate) container: ContainerID,
    pub(crate) contents: RleVec<[RemoteContent; 1]>,
}

/// RichOp includes lamport and timestamp info, which is used for conflict resolution.
#[derive(Debug, Clone)]
pub struct RichOp<'a> {
    op: &'a Op,
    client_id: ClientID,
    lamport: Lamport,
    timestamp: Timestamp,
    start: usize,
    end: usize,
}

impl Op {
    #[inline]
    pub(crate) fn new(id: ID, content: RemoteContent, container: u32) -> Self {
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
        let content: RemoteContent = self.content.slice(from, to);
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

impl<'a> HasId for RichOp<'a> {
    fn id_start(&self) -> ID {
        ID {
            client_id: self.client_id,
            counter: self.op.counter + self.start as Counter,
        }
    }
}

impl<'a> HasLength for RichOp<'a> {
    fn content_len(&self) -> usize {
        self.end - self.start
    }
}

impl<'a> HasLamport for RichOp<'a> {
    fn lamport(&self) -> Lamport {
        self.lamport + self.start as Lamport
    }
}

impl<'a> RichOp<'a> {
    pub fn new(op: &'a Op, client_id: ClientID, lamport: Lamport, timestamp: Timestamp) -> Self {
        RichOp {
            op,
            client_id,
            lamport,
            timestamp,
            start: 0,
            end: op.content_len(),
        }
    }

    pub fn new_by_change(change: &Change<Op>, op: &'a Op) -> Self {
        let diff = op.counter - change.id.counter;
        RichOp {
            op,
            client_id: change.id.client_id,
            lamport: change.lamport + diff as Lamport,
            timestamp: change.timestamp,
            start: 0,
            end: op.atom_len(),
        }
    }

    pub fn new_by_slice_on_change(change: &Change<Op>, op: &'a Op, start: i32, end: i32) -> Self {
        debug_assert!(end > start);
        let op_index_in_change = op.counter - change.id.counter;
        let op_slice_start = (start - op_index_in_change)
            .max(0)
            .min(op.atom_len() as i32);
        let op_slice_end = (end - op_index_in_change).max(0).min(op.atom_len() as i32);
        RichOp {
            op,
            client_id: change.id.client_id,
            lamport: change.lamport + op_index_in_change as Lamport,
            timestamp: change.timestamp,
            start: op_slice_start as usize,
            end: op_slice_end as usize,
        }
    }

    pub fn get_sliced(&self) -> Op {
        self.op.slice(self.start, self.end)
    }

    pub fn op(&self) -> &Op {
        self.op
    }

    pub fn client_id(&self) -> u64 {
        self.client_id
    }

    pub fn timestamp(&self) -> i64 {
        self.timestamp
    }

    pub fn start(&self) -> usize {
        self.start
    }

    pub fn end(&self) -> usize {
        self.end
    }
}

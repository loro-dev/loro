use crate::{
    change::{Change, Lamport, Timestamp},
    container::{
        registry::{ContainerIdx, ContainerInstance},
        ContainerID, ContainerTrait,
    },
    id::{Counter, PeerID, ID},
    span::{HasCounter, HasId, HasLamport},
};
use rle::{HasIndex, HasLength, Mergable, RleVec, Sliceable};
mod content;

pub use content::*;
use smallvec::SmallVec;

/// Operation is a unit of change.
///
/// A Op may have multiple atomic operations, since Op can be merged.
#[derive(Debug, Clone)]
pub struct Op {
    pub(crate) counter: Counter,
    pub(crate) container: ContainerIdx,
    pub(crate) content: InnerContent,
}

#[derive(Debug, Clone)]
pub struct RemoteOp<'a> {
    pub(crate) counter: Counter,
    pub(crate) container: ContainerID,
    pub(crate) contents: RleVec<[RawOpContent<'a>; 1]>,
}

/// This is used to propagate messages between inner module.
/// It's a temporary struct, and will be converted to Op when it's persisted.
#[derive(Debug, Clone)]
pub struct RawOp<'a> {
    pub id: ID,
    pub lamport: Lamport,
    pub container: ContainerIdx,
    pub content: RawOpContent<'a>,
}

/// RichOp includes lamport and timestamp info, which is used for conflict resolution.
#[derive(Debug, Clone)]
pub struct RichOp<'a> {
    op: &'a Op,
    client_id: PeerID,
    lamport: Lamport,
    timestamp: Timestamp,
    start: usize,
    end: usize,
}

/// RichOp includes lamport and timestamp info, which is used for conflict resolution.
#[derive(Debug, Clone)]
pub struct OwnedRichOp {
    pub op: Op,
    pub client_id: PeerID,
    pub lamport: Lamport,
    pub timestamp: Timestamp,
}

impl Op {
    #[inline]
    pub(crate) fn new(id: ID, content: InnerContent, container: ContainerIdx) -> Self {
        Op {
            counter: id.counter,
            content,
            container,
        }
    }

    pub(crate) fn convert(self, container: &mut ContainerInstance, gc: bool) -> RemoteOp {
        RemoteOp {
            counter: self.counter,
            container: container.id().clone(),
            contents: RleVec::from(container.to_export(self.content, gc)),
        }
    }
}

impl<'a> RemoteOp<'a> {
    pub(crate) fn convert(
        self,
        container: &mut ContainerInstance,
        container_idx: ContainerIdx,
    ) -> SmallVec<[Op; 1]> {
        let mut counter = self.counter;
        self.contents
            .into_iter()
            .map(|content| {
                let ans = Op {
                    counter,
                    container: container_idx,
                    content: container.to_import(content),
                };
                counter += ans.atom_len() as Counter;
                ans
            })
            .collect()
    }

    pub(crate) fn to_static(self) -> RemoteOp<'static> {
        RemoteOp {
            counter: self.counter,
            container: self.container,
            contents: self.contents.into_iter().map(|c| c.to_static()).collect(),
        }
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
        let content: InnerContent = self.content.slice(from, to);
        Op {
            counter: (self.counter + from as Counter),
            content,
            container: self.container,
        }
    }
}

impl<'a> Mergable for RemoteOp<'a> {
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

impl<'a> HasLength for RemoteOp<'a> {
    fn content_len(&self) -> usize {
        self.contents.iter().map(|x| x.atom_len()).sum()
    }
}

impl<'a> Sliceable for RemoteOp<'a> {
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

impl<'a> HasIndex for RemoteOp<'a> {
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

impl<'a> HasCounter for RemoteOp<'a> {
    fn ctr_start(&self) -> Counter {
        self.counter
    }
}

impl<'a> HasId for RichOp<'a> {
    fn id_start(&self) -> ID {
        ID {
            peer: self.client_id,
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
    pub fn new(op: &'a Op, client_id: PeerID, lamport: Lamport, timestamp: Timestamp) -> Self {
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
            client_id: change.id.peer,
            lamport: change.lamport + diff as Lamport,
            timestamp: change.timestamp,
            start: 0,
            end: op.atom_len(),
        }
    }

    /// we want the overlap part of the op and change[start..end]
    ///
    /// op is contained in the change, but it's not necessary overlap with change[start..end]
    pub fn new_by_slice_on_change(change: &Change<Op>, start: i32, end: i32, op: &'a Op) -> Self {
        debug_assert!(end > start);
        let op_index_in_change = op.counter - change.id.counter;
        let op_slice_start = (start - op_index_in_change).clamp(0, op.atom_len() as i32);
        let op_slice_end = (end - op_index_in_change).clamp(0, op.atom_len() as i32);
        RichOp {
            op,
            client_id: change.id.peer,
            lamport: change.lamport + op_index_in_change as Lamport,
            timestamp: change.timestamp,
            start: op_slice_start as usize,
            end: op_slice_end as usize,
        }
    }

    pub fn get_sliced(&self) -> Op {
        self.op.slice(self.start, self.end)
    }

    pub fn as_owned(&self) -> OwnedRichOp {
        OwnedRichOp {
            op: self.get_sliced(),
            client_id: self.client_id,
            lamport: self.lamport,
            timestamp: self.timestamp,
        }
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

impl OwnedRichOp {
    pub fn rich_op(&self) -> RichOp {
        RichOp {
            op: &self.op,
            client_id: self.client_id,
            lamport: self.lamport,
            timestamp: self.timestamp,
            start: 0,
            end: self.op.atom_len(),
        }
    }
}

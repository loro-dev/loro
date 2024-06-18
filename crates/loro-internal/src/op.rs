use crate::{
    change::{Change, Lamport, Timestamp},
    container::{idx::ContainerIdx, ContainerID},
    estimated_size::EstimatedSize,
    id::{Counter, PeerID, ID},
    oplog::BlockChangeRef,
    span::{HasCounter, HasId, HasLamport},
};
use crate::{delta::DeltaValue, LoroValue};
use enum_as_inner::EnumAsInner;
use loro_common::{CounterSpan, IdFull, IdLp, IdSpan};
use rle::{HasIndex, HasLength, Mergable, Sliceable};
use serde::{ser::SerializeSeq, Deserialize, Serialize};
use smallvec::SmallVec;
use std::{borrow::Cow, ops::Range};

mod content;
pub use content::*;

/// Operation is a unit of change.
///
/// A Op may have multiple atomic operations, since Op can be merged.
#[derive(Debug, Clone)]
pub struct Op {
    pub(crate) counter: Counter,
    pub(crate) container: ContainerIdx,
    pub(crate) content: InnerContent,
}

impl EstimatedSize for Op {
    fn estimate_storage_size(&self) -> usize {
        let counter_size = 4;
        let container_size = 2;
        let content_size = self
            .content
            .estimate_storage_size(self.container.get_type());
        counter_size + container_size + content_size
    }
}

#[derive(Debug, Clone)]
pub(crate) struct OpWithId {
    pub peer: PeerID,
    pub op: Op,
    pub lamport: Option<Lamport>,
}

impl OpWithId {
    pub fn id(&self) -> ID {
        ID {
            peer: self.peer,
            counter: self.op.counter,
        }
    }

    pub fn id_full(&self) -> IdFull {
        IdFull::new(
            self.peer,
            self.op.counter,
            self.lamport.expect("op should already be imported"),
        )
    }

    #[allow(unused)]
    pub fn id_span(&self) -> IdSpan {
        IdSpan::new(
            self.peer,
            self.op.counter,
            self.op.counter + self.op.atom_len() as Counter,
        )
    }
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "wasm", derive(Serialize, Deserialize))]
pub struct RemoteOp<'a> {
    pub(crate) counter: Counter,
    pub(crate) container: ContainerID,
    pub(crate) content: RawOpContent<'a>,
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

impl RawOp<'_> {
    pub(crate) fn id_full(&self) -> loro_common::IdFull {
        IdFull::new(self.id.peer, self.id.counter, self.lamport)
    }

    pub(crate) fn idlp(&self) -> loro_common::IdLp {
        IdLp::new(self.id.peer, self.lamport)
    }
}

/// RichOp includes lamport and timestamp info, which is used for conflict resolution.
#[derive(Debug, Clone)]
pub struct RichOp<'a> {
    op: Cow<'a, Op>,
    pub peer: PeerID,
    lamport: Lamport,
    pub timestamp: Timestamp,
    pub start: usize,
    pub end: usize,
}

impl Op {
    #[inline]
    #[allow(unused)]
    pub(crate) fn new(id: ID, content: InnerContent, container: ContainerIdx) -> Self {
        Op {
            counter: id.counter,
            content,
            container,
        }
    }
}

impl<'a> RemoteOp<'a> {
    #[allow(unused)]
    pub(crate) fn into_static(self) -> RemoteOp<'static> {
        RemoteOp {
            counter: self.counter,
            container: self.container,
            content: self.content.to_static(),
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
        assert!(to > from, "{to} should be greater than {from}");
        let content: InnerContent = self.content.slice(from, to);
        Op {
            counter: (self.counter + from as Counter),
            content,
            container: self.container,
        }
    }
}

impl<'a> Mergable for RemoteOp<'a> {
    fn is_mergable(&self, _other: &Self, _cfg: &()) -> bool {
        // don't merge remote op, because it's already merged.
        false
    }

    fn merge(&mut self, _other: &Self, _: &()) {
        unreachable!()
    }
}

impl<'a> HasLength for RemoteOp<'a> {
    fn content_len(&self) -> usize {
        self.content.atom_len()
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
            peer: self.peer,
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
    pub fn new_by_change(change: &Change<Op>, op: &'a Op) -> Self {
        let diff = op.counter - change.id.counter;
        RichOp {
            op: Cow::Borrowed(op),
            peer: change.id.peer,
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
            op: Cow::Borrowed(op),
            peer: change.id.peer,
            lamport: change.lamport + op_index_in_change as Lamport,
            timestamp: change.timestamp,
            start: op_slice_start as usize,
            end: op_slice_end as usize,
        }
    }

    pub fn new_by_cnt_range(change: &Change<Op>, span: CounterSpan, op: &'a Op) -> Option<Self> {
        let op_index_in_change = op.counter - change.id.counter;
        let op_slice_start = (span.start - op.counter).clamp(0, op.atom_len() as i32);
        let op_slice_end = (span.end - op.counter).clamp(0, op.atom_len() as i32);
        if op_slice_start == op_slice_end {
            return None;
        }
        Some(RichOp {
            op: Cow::Borrowed(op),
            peer: change.id.peer,
            lamport: change.lamport + op_index_in_change as Lamport,
            timestamp: change.timestamp,
            start: op_slice_start as usize,
            end: op_slice_end as usize,
        })
    }

    pub fn new_iter_by_cnt_range(change: BlockChangeRef, span: CounterSpan) -> RichOpBlockIter {
        RichOpBlockIter {
            change,
            span,
            op_index: 0,
        }
    }

    pub fn op(&self) -> Cow<'_, Op> {
        if self.start == 0 && self.end == self.op.content_len() {
            self.op.clone()
        } else {
            Cow::Owned(self.op.slice(self.start, self.end))
        }
    }

    pub fn raw_op(&self) -> &Op {
        &self.op
    }

    pub fn client_id(&self) -> u64 {
        self.peer
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

    #[allow(unused)]
    pub(crate) fn id(&self) -> ID {
        ID {
            peer: self.peer,
            counter: self.op.counter + self.start as Counter,
        }
    }

    pub(crate) fn id_full(&self) -> IdFull {
        IdFull::new(self.peer, self.op.counter, self.lamport)
    }
}

pub(crate) struct RichOpBlockIter {
    change: BlockChangeRef,
    span: CounterSpan,
    op_index: usize,
}

impl Iterator for RichOpBlockIter {
    type Item = RichOp<'static>;

    fn next(&mut self) -> Option<Self::Item> {
        let op = self.change.ops.get(self.op_index)?.clone();
        let op_offset_in_change = op.counter - self.change.id.counter;
        let op_slice_start = (self.span.start - op.counter).clamp(0, op.atom_len() as i32);
        let op_slice_end = (self.span.end - op.counter).clamp(0, op.atom_len() as i32);
        self.op_index += 1;
        if op_slice_start == op_slice_end {
            return self.next();
        }

        Some(RichOp {
            op: Cow::Owned(op),
            peer: self.change.id.peer,
            lamport: self.change.lamport + op_offset_in_change as Lamport,
            timestamp: self.change.timestamp,
            start: op_slice_start as usize,
            end: op_slice_end as usize,
        })
    }
}

// Note: It will be encoded into binary format, so the order of its fields should not be changed.
#[derive(PartialEq, Debug, EnumAsInner, Clone, Serialize, Deserialize)]
pub enum ListSlice<'a> {
    RawData(Cow<'a, [LoroValue]>),
    RawStr {
        str: Cow<'a, str>,
        unicode_len: usize,
    },
}

impl<'a> ListSlice<'a> {
    pub fn from_borrowed_str(str: &'a str) -> Self {
        Self::RawStr {
            str: Cow::Borrowed(str),
            unicode_len: str.chars().count(),
        }
    }
}

#[repr(transparent)]
#[derive(PartialEq, Eq, Debug, Clone, Serialize)]
pub struct SliceRange(pub Range<u32>);

const UNKNOWN_START: u32 = u32::MAX / 2;
impl SliceRange {
    #[inline(always)]
    pub fn is_unknown(&self) -> bool {
        self.0.start == UNKNOWN_START
    }

    #[inline(always)]
    pub fn new_unknown(size: u32) -> Self {
        Self(UNKNOWN_START..UNKNOWN_START + size)
    }

    #[inline(always)]
    pub fn new(range: Range<u32>) -> Self {
        Self(range)
    }

    #[inline(always)]
    pub fn to_range(&self) -> Range<usize> {
        self.0.start as usize..self.0.end as usize
    }
}

impl From<Range<u32>> for SliceRange {
    fn from(a: Range<u32>) -> Self {
        SliceRange(a)
    }
}

impl HasLength for SliceRange {
    fn content_len(&self) -> usize {
        self.0.len()
    }
}

impl Sliceable for SliceRange {
    fn slice(&self, from: usize, to: usize) -> Self {
        if self.is_unknown() {
            Self::new_unknown((to - from) as u32)
        } else {
            SliceRange(self.0.start + from as u32..self.0.start + to as u32)
        }
    }
}

impl Mergable for SliceRange {
    fn merge(&mut self, other: &Self, _: &()) {
        if self.is_unknown() {
            self.0.end += other.0.end - other.0.start;
        } else {
            self.0.end = other.0.end;
        }
    }

    fn is_mergable(&self, other: &Self, _conf: &()) -> bool
    where
        Self: Sized,
    {
        (self.is_unknown() && other.is_unknown()) || self.0.end == other.0.start
    }
}

impl<'a> ListSlice<'a> {
    #[inline(always)]
    pub fn unknown_range(len: usize) -> SliceRange {
        let start = UNKNOWN_START;
        let end = len as u32 + UNKNOWN_START;
        SliceRange(start..end)
    }

    #[inline(always)]
    pub fn is_unknown(range: &SliceRange) -> bool {
        range.is_unknown()
    }

    pub fn to_static(&self) -> ListSlice<'static> {
        match self {
            ListSlice::RawData(x) => ListSlice::RawData(Cow::Owned(x.to_vec())),
            ListSlice::RawStr { str, unicode_len } => ListSlice::RawStr {
                str: Cow::Owned(str.to_string()),
                unicode_len: *unicode_len,
            },
        }
    }
}

impl<'a> HasLength for ListSlice<'a> {
    fn content_len(&self) -> usize {
        match self {
            ListSlice::RawStr { unicode_len, .. } => *unicode_len,
            ListSlice::RawData(x) => x.len(),
        }
    }
}

impl<'a> Sliceable for ListSlice<'a> {
    fn slice(&self, from: usize, to: usize) -> Self {
        match self {
            ListSlice::RawStr {
                str,
                unicode_len: _,
            } => {
                let ans = str.chars().skip(from).take(to - from).collect::<String>();
                ListSlice::RawStr {
                    str: Cow::Owned(ans),
                    unicode_len: to - from,
                }
            }
            ListSlice::RawData(x) => match x {
                Cow::Borrowed(x) => ListSlice::RawData(Cow::Borrowed(&x[from..to])),
                Cow::Owned(x) => ListSlice::RawData(Cow::Owned(x[from..to].into())),
            },
        }
    }
}

impl<'a> Mergable for ListSlice<'a> {
    fn is_mergable(&self, _other: &Self, _: &()) -> bool {
        false
    }
}

#[derive(Debug, Clone)]
pub struct SliceRanges {
    pub ranges: SmallVec<[SliceRange; 2]>,
    pub id: IdFull,
}

impl Serialize for SliceRanges {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut s = serializer.serialize_seq(Some(self.ranges.len()))?;
        for item in self.ranges.iter() {
            s.serialize_element(item)?;
        }
        s.end()
    }
}

impl DeltaValue for SliceRanges {
    fn value_extend(&mut self, other: Self) -> Result<(), Self> {
        if self.id.peer != other.id.peer {
            return Err(other);
        }

        if self.id.counter + self.length() as Counter != other.id.counter {
            return Err(other);
        }

        if self.id.lamport + self.length() as Lamport != other.id.lamport {
            return Err(other);
        }

        self.ranges.extend(other.ranges);
        Ok(())
    }

    fn take(&mut self, target_len: usize) -> Self {
        let mut right = Self {
            ranges: Default::default(),
            id: self.id.inc(target_len as i32),
        };

        let right_target_len = self.length() - target_len;
        let mut right_len = 0;
        while right_len < right_target_len {
            let range = self.ranges.pop().unwrap();
            let range_len = range.content_len();
            if right_len + range_len <= target_len {
                right.ranges.push(range);
                right_len += range_len;
            } else {
                let new_range = range.slice(right_len * 2 - right_target_len, range_len);
                right.ranges.push(new_range);
                self.ranges
                    .push(range.slice(0, right_len * 2 - right_target_len));
                right_len = right_target_len;
            }
        }

        std::mem::swap(self, &mut right);
        let left = right;
        #[allow(clippy::let_and_return)]
        left
    }

    fn length(&self) -> usize {
        self.ranges.iter().fold(0, |acc, x| acc + x.atom_len())
    }
}

#[cfg(test)]
mod test {
    use crate::LoroValue;

    use super::ListSlice;

    #[test]
    fn fix_fields_order() {
        let list_slice = vec![
            ListSlice::RawData(vec![LoroValue::Bool(true)].into()),
            ListSlice::RawStr {
                str: "".into(),
                unicode_len: 0,
            },
        ];
        let list_slice_buf = vec![2, 0, 1, 1, 1, 1, 0, 0];
        assert_eq!(
            &postcard::to_allocvec(&list_slice).unwrap(),
            &list_slice_buf
        );
        assert_eq!(
            postcard::from_bytes::<Vec<ListSlice>>(&list_slice_buf).unwrap(),
            list_slice
        );
    }
}

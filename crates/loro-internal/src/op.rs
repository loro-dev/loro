use crate::{
    change::{Change, Lamport, Timestamp},
    container::{idx::ContainerIdx, ContainerID},
    id::{Counter, PeerID, ID},
    span::{HasCounter, HasId, HasLamport},
};
use crate::{delta::DeltaValue, LoroValue};
use enum_as_inner::EnumAsInner;
use rle::{HasIndex, HasLength, Mergable, Sliceable};
use serde::{ser::SerializeSeq, Deserialize, Serialize};
use smallvec::{smallvec, SmallVec};
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

#[derive(Debug, Clone, Serialize, Deserialize)]
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

/// RichOp includes lamport and timestamp info, which is used for conflict resolution.
#[derive(Debug, Clone)]
pub struct RichOp<'a> {
    pub op: &'a Op,
    pub peer: PeerID,
    pub lamport: Lamport,
    pub timestamp: Timestamp,
    pub start: usize,
    pub end: usize,
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

impl<'a> Sliceable for RemoteOp<'a> {
    fn slice(&self, from: usize, to: usize) -> Self {
        assert!(to > from);
        RemoteOp {
            counter: (self.counter + from as Counter),
            content: self.content.slice(from, to),
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
    pub fn new(op: &'a Op, client_id: PeerID, lamport: Lamport, timestamp: Timestamp) -> Self {
        RichOp {
            op,
            peer: client_id,
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
            op,
            peer: change.id.peer,
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
            client_id: self.peer,
            lamport: self.lamport,
            timestamp: self.timestamp,
        }
    }

    pub fn op(&self) -> &Op {
        self.op
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
}

impl OwnedRichOp {
    pub fn rich_op(&self) -> RichOp {
        RichOp {
            op: &self.op,
            peer: self.client_id,
            lamport: self.lamport,
            timestamp: self.timestamp,
            start: 0,
            end: self.op.atom_len(),
        }
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
pub struct SliceRanges(pub SmallVec<[SliceRange; 2]>);

impl Serialize for SliceRanges {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut s = serializer.serialize_seq(Some(self.0.len()))?;
        for item in self.0.iter() {
            s.serialize_element(item)?;
        }
        s.end()
    }
}

impl From<SliceRange> for SliceRanges {
    fn from(value: SliceRange) -> Self {
        Self(smallvec![value])
    }
}

impl DeltaValue for SliceRanges {
    fn value_extend(&mut self, other: Self) -> Result<(), Self> {
        self.0.extend(other.0);
        Ok(())
    }

    fn take(&mut self, target_len: usize) -> Self {
        let mut ret = SmallVec::new();
        let mut cur_len = 0;
        while cur_len < target_len {
            let range = self.0.pop().unwrap();
            let range_len = range.content_len();
            if cur_len + range_len <= target_len {
                ret.push(range);
                cur_len += range_len;
            } else {
                let new_range = range.slice(0, target_len - cur_len);
                ret.push(new_range);
                self.0.push(range.slice(target_len - cur_len, range_len));
                cur_len = target_len;
            }
        }
        SliceRanges(ret)
    }

    fn length(&self) -> usize {
        self.0.iter().fold(0, |acc, x| acc + x.atom_len())
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

use crate::{
    change::{Change, Lamport, Timestamp},
    container::{idx::ContainerIdx, ContainerID},
    estimated_size::EstimatedSize,
    id::{Counter, PeerID, ID},
    oplog::BlockChangeRef,
    span::{HasCounter, HasId, HasLamport},
};
use crate::{delta::DeltaValue, LoroValue};
use either::Either;
use enum_as_inner::EnumAsInner;
use loro_common::{CompactIdLp, ContainerType, CounterSpan, IdFull, IdLp, IdSpan};
use rle::{HasIndex, HasLength, Mergable, Sliceable};
use serde::{Deserialize, Serialize};
use std::{borrow::Cow, sync::Arc};

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
        // TODO: use benchmark to get the optimal estimated size for each container type
        self.content
            .estimate_storage_size(self.container.get_type())
    }
}

#[derive(Debug, Clone)]
pub(crate) struct OpWithId {
    pub peer: PeerID,
    pub op: Op,
}

impl OpWithId {
    #[allow(unused)]
    pub fn id_span(&self) -> IdSpan {
        IdSpan::new(
            self.peer,
            self.op.counter,
            self.op.counter + self.op.atom_len() as Counter,
        )
    }
}

#[derive(Debug, Clone, PartialEq)]
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

    /// If the estimated storage size of the content is greater than the given size,
    /// return the length of the content that makes the estimated storage size equal to the given size.
    /// Otherwise, return None.
    pub(crate) fn check_whether_slice_content_to_fit_in_size(&self, size: usize) -> Option<usize> {
        if self.estimate_storage_size() <= size {
            return None;
        }

        match &self.content {
            InnerContent::List(l) => match l {
                crate::container::list::list_op::InnerListOp::Insert { .. } => {
                    if matches!(self.container.get_type(), ContainerType::Text) {
                        Some(size.min(self.atom_len()))
                    } else {
                        Some((size / 4).min(self.atom_len()))
                    }
                }
                crate::container::list::list_op::InnerListOp::InsertText { .. } => {
                    Some(size.min(self.atom_len()))
                }
                _ => unreachable!(),
            },
            _ => unreachable!(),
        }
    }
}

impl RemoteOp<'_> {
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
        assert!(to <= self.atom_len());
        let content: InnerContent = self.content.slice(from, to);
        Op {
            counter: (self.counter + from as Counter),
            content,
            container: self.container,
        }
    }
}

impl Mergable for RemoteOp<'_> {
    fn is_mergable(&self, _other: &Self, _cfg: &()) -> bool {
        // don't merge remote op, because it's already merged.
        false
    }

    fn merge(&mut self, _other: &Self, _: &()) {
        unreachable!()
    }
}

impl HasLength for RemoteOp<'_> {
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

impl HasIndex for RemoteOp<'_> {
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

impl HasCounter for RemoteOp<'_> {
    fn ctr_start(&self) -> Counter {
        self.counter
    }
}

impl HasId for RichOp<'_> {
    fn id_start(&self) -> ID {
        ID {
            peer: self.peer,
            counter: self.op.counter + self.start as Counter,
        }
    }
}

impl HasLength for RichOp<'_> {
    fn content_len(&self) -> usize {
        self.end - self.start
    }
}

impl HasLamport for RichOp<'_> {
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

    pub(crate) fn new_iter_by_cnt_range(
        change: BlockChangeRef,
        span: CounterSpan,
    ) -> RichOpBlockIter {
        // Start at the first op that reaches into `span`.
        let op_index = change
            .ops
            .vec()
            .partition_point(|op| op.counter + op.atom_len() as Counter <= span.start);
        RichOpBlockIter {
            change,
            span,
            op_index,
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

    pub fn container(&self) -> ContainerIdx {
        self.op.container
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

    pub fn counter(&self) -> Counter {
        self.op.counter + self.start as Counter
    }

    #[allow(unused)]
    pub(crate) fn id(&self) -> ID {
        ID {
            peer: self.peer,
            counter: self.op.counter + self.start as Counter,
        }
    }

    pub(crate) fn id_full(&self) -> IdFull {
        IdFull::new(
            self.peer,
            self.op.counter + self.start as Counter,
            self.lamport + self.start as Lamport,
        )
    }

    pub fn idlp(&self) -> IdLp {
        IdLp {
            lamport: self.lamport + self.start as Lamport,
            peer: self.peer,
        }
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
        // Clone only the ops that are returned: cloning a list insert clones its
        // values' handle.
        let (op, op_slice_start, op_slice_end) = loop {
            let op = self.change.ops.get(self.op_index)?;
            self.op_index += 1;
            let op_slice_start = (self.span.start - op.counter).clamp(0, op.atom_len() as i32);
            let op_slice_end = (self.span.end - op.counter).clamp(0, op.atom_len() as i32);
            if op_slice_start != op_slice_end {
                break (op.clone(), op_slice_start, op_slice_end);
            }
        };
        let op_offset_in_change = op.counter - self.change.id.counter;

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

/// The values of a list insert op.
///
/// The op owns them. They used to be a range in the doc-wide values arena,
/// which never frees, so every parse of a change block appended its values to it
/// again (`context/text-arena-growth.md`).
#[derive(Clone)]
pub struct ValueSlice(Repr);

#[derive(Clone)]
enum Repr {
    /// A single value, as most local inserts have. It needs no allocation.
    One(LoroValue),
    Shared {
        values: Arc<Vec<LoroValue>>,
        start: u32,
        end: u32,
    },
}

impl ValueSlice {
    pub fn new(mut values: Vec<LoroValue>) -> Self {
        if values.len() == 1 {
            return Self(Repr::One(values.pop().unwrap()));
        }

        let end = u32::try_from(values.len()).expect("too many values in one list op");
        Self(Repr::Shared {
            values: Arc::new(values),
            start: 0,
            end,
        })
    }

    /// A slice of `buffer` that is not filled yet. See
    /// `DecodedBlockBuffers::push_values`.
    pub(crate) fn pending(buffer: &Arc<Vec<LoroValue>>, range: std::ops::Range<usize>) -> Self {
        let end = u32::try_from(range.end).expect("too many values in one change block");
        Self(Repr::Shared {
            values: buffer.clone(),
            start: range.start as u32,
            end,
        })
    }

    /// Points this slice at `to` if it points at `from`.
    pub(crate) fn replace_buffer(&mut self, from: &Arc<Vec<LoroValue>>, to: &Arc<Vec<LoroValue>>) {
        if let Repr::Shared { values, .. } = &mut self.0 {
            if Arc::ptr_eq(values, from) {
                *values = to.clone();
            }
        }
    }

    #[inline]
    pub fn as_slice(&self) -> &[LoroValue] {
        match &self.0 {
            Repr::One(value) => std::slice::from_ref(value),
            Repr::Shared { values, start, end } => &values[*start as usize..*end as usize],
        }
    }

    #[inline]
    pub fn len(&self) -> usize {
        match &self.0 {
            Repr::One(_) => 1,
            Repr::Shared { start, end, .. } => (end - start) as usize,
        }
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Extends this slice by `other` if it follows this slice in the same buffer.
    pub(crate) fn extend_if_adjacent(&mut self, other: &Self) -> bool {
        match (&mut self.0, &other.0) {
            (
                Repr::Shared { values, end, .. },
                Repr::Shared {
                    values: other_values,
                    start: other_start,
                    end: other_end,
                },
            ) if Arc::ptr_eq(values, other_values) && *end == *other_start => {
                *end = *other_end;
                true
            }
            _ => false,
        }
    }

    /// Whether this slice ends its buffer and is the buffer's only owner.
    fn can_append(&self) -> bool {
        match &self.0 {
            Repr::One(_) => false,
            Repr::Shared { values, end, .. } => {
                *end as usize == values.len()
                    && Arc::strong_count(values) == 1
                    && Arc::weak_count(values) == 0
            }
        }
    }
}

impl PartialEq for ValueSlice {
    fn eq(&self, other: &Self) -> bool {
        self.as_slice() == other.as_slice()
    }
}

impl Eq for ValueSlice {}

impl std::fmt::Debug for ValueSlice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("ValueSlice").field(&self.as_slice()).finish()
    }
}

impl HasLength for ValueSlice {
    fn content_len(&self) -> usize {
        self.len()
    }
}

impl Sliceable for ValueSlice {
    fn slice(&self, from: usize, to: usize) -> Self {
        assert!(from <= to && to <= self.len());
        match &self.0 {
            Repr::One(_) if from == 0 && to == 1 => self.clone(),
            Repr::One(_) => Self::new(Vec::new()),
            Repr::Shared { values, start, .. } => Self(Repr::Shared {
                values: values.clone(),
                start: start + from as u32,
                end: start + to as u32,
            }),
        }
    }
}

impl Mergable for ValueSlice {
    fn merge(&mut self, other: &Self, _: &()) {
        if self.extend_if_adjacent(other) {
            return;
        }

        if self.can_append() {
            let Repr::Shared { values, end, .. } = &mut self.0 else {
                unreachable!()
            };
            let values = Arc::get_mut(values).unwrap();
            values.extend_from_slice(other.as_slice());
            *end = u32::try_from(values.len()).expect("too many values in one list op");
            return;
        }

        // Copy only this slice, not the whole shared buffer. The merged slice
        // owns its buffer, so later merges append to it.
        let mut values = Vec::with_capacity((self.len() + other.len()).max(4));
        values.extend_from_slice(self.as_slice());
        values.extend_from_slice(other.as_slice());
        *self = Self::new(values);
    }

    /// Mergeable when the merge copies at most this slice's single value:
    /// `other` follows it in one buffer, or it can append to its buffer, or it
    /// is a single value.
    fn is_mergable(&self, other: &Self, _conf: &()) -> bool
    where
        Self: Sized,
    {
        matches!(self.0, Repr::One(_))
            || self.can_append()
            || matches!(
                (&self.0, &other.0),
                (
                    Repr::Shared { values, end, .. },
                    Repr::Shared { values: other_values, start, .. },
                ) if Arc::ptr_eq(values, other_values) && end == start
            )
    }
}

impl ListSlice<'_> {
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

impl HasLength for ListSlice<'_> {
    fn content_len(&self) -> usize {
        match self {
            ListSlice::RawStr { unicode_len, .. } => *unicode_len,
            ListSlice::RawData(x) => x.len(),
        }
    }
}

impl Sliceable for ListSlice<'_> {
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

impl Mergable for ListSlice<'_> {
    fn is_mergable(&self, _other: &Self, _: &()) -> bool {
        false
    }
}

#[derive(Debug, Clone)]
pub struct SliceWithId {
    pub values: Either<ValueSlice, LoroValue>,
    /// This field is no-none when diff calculating movable list that need to handle_unknown
    pub elem_id: Option<CompactIdLp>,
    pub id: IdFull,
}

impl DeltaValue for SliceWithId {
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

        match (&mut self.values, &other.values) {
            (Either::Left(left), Either::Left(right)) => {
                if left.extend_if_adjacent(right) {
                    Ok(())
                } else {
                    Err(other)
                }
            }
            _ => {
                // Values in different buffers, or a ValueSlice and a LoroValue
                Err(other)
            }
        }
    }

    fn take(&mut self, target_len: usize) -> Self {
        match &mut self.values {
            Either::Left(range) => {
                let ans = range.slice(0, target_len);
                let this = range.slice(target_len, range.atom_len());
                *range = this;
                let old_id = self.id;
                self.id = self.id.inc(target_len as i32);
                Self {
                    id: old_id,
                    values: Either::Left(ans),
                    elem_id: None,
                }
            }
            Either::Right(_) => unimplemented!(),
        }
    }

    fn length(&self) -> usize {
        match &self.values {
            Either::Left(r) => r.atom_len(),
            Either::Right(_) => 1,
        }
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

use std::ops::Range;

use enum_as_inner::EnumAsInner;
use rle::{rle_tree::tree_trait::CumulateTreeTrait, HasLength, Mergable, Sliceable};

use crate::{smstring::SmString, LoroValue};

#[derive(PartialEq, Debug, EnumAsInner, Clone)]
pub enum ListSlice {
    // TODO: use Box<[LoroValue]> ?
    RawData(Vec<LoroValue>),
    RawStr(SmString),
    Slice(SliceRange),
    Unknown(usize),
}

#[repr(transparent)]
#[derive(PartialEq, Eq, Debug, Clone)]
pub struct SliceRange(pub Range<u32>);

const UNKNOWN_START: u32 = u32::MAX / 2;
impl SliceRange {
    #[inline(always)]
    pub fn is_unknown(&self) -> bool {
        self.0.start == UNKNOWN_START
    }

    pub fn new_unknown(size: u32) -> Self {
        Self(UNKNOWN_START..UNKNOWN_START + size)
    }
}

impl Default for ListSlice {
    fn default() -> Self {
        ListSlice::Unknown(0)
    }
}

impl From<Range<u32>> for ListSlice {
    fn from(a: Range<u32>) -> Self {
        ListSlice::Slice(a.into())
    }
}

impl From<SliceRange> for ListSlice {
    fn from(a: SliceRange) -> Self {
        ListSlice::Slice(a)
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

impl ListSlice {
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
}

pub(super) fn new_unknown_text(len: usize) -> ListSlice {
    ListSlice::Unknown(len)
}

pub(super) fn is_unknown_text(a: &ListSlice) -> bool {
    a.as_unknown().is_some()
}

impl HasLength for ListSlice {
    fn content_len(&self) -> usize {
        match self {
            ListSlice::RawStr(s) => s.len(),
            ListSlice::Slice(x) => rle::HasLength::content_len(&x),
            ListSlice::Unknown(x) => *x,
            ListSlice::RawData(x) => x.len(),
        }
    }
}

impl Sliceable for ListSlice {
    fn slice(&self, from: usize, to: usize) -> Self {
        match self {
            ListSlice::RawStr(s) => ListSlice::RawStr(s.0[from..to].into()),
            ListSlice::Slice(x) => ListSlice::Slice(x.slice(from, to)),
            ListSlice::Unknown(_) => ListSlice::Unknown(to - from),
            ListSlice::RawData(x) => ListSlice::RawData(x[from..to].to_vec()),
        }
    }
}

impl Mergable for ListSlice {
    fn is_mergable(&self, other: &Self, _: &()) -> bool {
        match (self, other) {
            (ListSlice::Slice(x), ListSlice::Slice(y)) => x.is_mergable(y, &()),
            (ListSlice::Unknown(_), ListSlice::Unknown(_)) => true,
            (ListSlice::RawStr(a), ListSlice::RawStr(b)) => a.is_mergable(b, &()),
            _ => false,
        }
    }

    fn merge(&mut self, other: &Self, _: &()) {
        match (self, other) {
            (ListSlice::Slice(x), ListSlice::Slice(y)) => x.merge(y, &()),
            (ListSlice::Unknown(x), ListSlice::Unknown(y)) => {
                *x += y;
            }
            (ListSlice::RawStr(a), ListSlice::RawStr(b)) => a.merge(b, &()),
            _ => unreachable!(),
        }
    }
}

pub(super) type ListSliceTreeTrait = CumulateTreeTrait<ListSlice, 8>;

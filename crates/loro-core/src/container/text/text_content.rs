use std::ops::Range;

use enum_as_inner::EnumAsInner;
use rle::{rle_tree::tree_trait::CumulateTreeTrait, HasLength, Mergable, Sliceable};

use crate::{id::ID, smstring::SmString};

#[derive(PartialEq, Eq, Debug, EnumAsInner, Clone)]
pub enum ListSlice {
    RawStr(SmString),
    // TODO: Use small compact rle vec
    Slice(Range<usize>),
    Unknown(usize),
}

impl Default for ListSlice {
    fn default() -> Self {
        ListSlice::Unknown(0)
    }
}

impl ListSlice {
    #[inline(always)]
    pub fn from_range(range: Range<usize>) -> ListSlice {
        Self::Slice(range)
    }

    pub fn from_raw(str: SmString) -> ListSlice {
        Self::RawStr(str)
    }
}

#[derive(Debug, EnumAsInner)]
pub(super) enum TextOpContent {
    Insert { id: ID, text: ListSlice, pos: usize },
    Delete { id: ID, pos: usize, len: usize },
}

pub(super) fn new_unknown_text(len: usize) -> ListSlice {
    ListSlice::Unknown(len)
}

pub(super) fn is_unknown_text(a: &ListSlice) -> bool {
    a.as_unknown().is_some()
}

impl HasLength for ListSlice {
    fn len(&self) -> usize {
        match self {
            ListSlice::RawStr(s) => s.len(),
            ListSlice::Slice(x) => rle::HasLength::len(&x),
            ListSlice::Unknown(x) => *x,
        }
    }
}

impl Sliceable for ListSlice {
    fn slice(&self, from: usize, to: usize) -> Self {
        match self {
            ListSlice::RawStr(s) => ListSlice::RawStr(s.0[from..to].into()),
            ListSlice::Slice(x) => ListSlice::Slice(x.slice(from, to)),
            ListSlice::Unknown(_) => ListSlice::Unknown(to - from),
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

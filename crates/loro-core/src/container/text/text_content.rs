use std::ops::Range;

use enum_as_inner::EnumAsInner;
use rle::{HasLength, Mergable, Sliceable};

use crate::id::ID;

#[derive(PartialEq, Eq, Debug, EnumAsInner, Clone)]
pub(crate) enum ListSlice {
    Slice(Range<usize>),
    Unknown(usize),
}

impl ListSlice {
    #[inline(always)]
    pub fn new(range: Range<usize>) -> ListSlice {
        Self::Slice(range)
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
            ListSlice::Slice(x) => std::iter::ExactSizeIterator::len(x),
            ListSlice::Unknown(x) => *x,
        }
    }
}

impl Sliceable for ListSlice {
    fn slice(&self, from: usize, to: usize) -> Self {
        match self {
            ListSlice::Slice(x) => ListSlice::Slice(x.start + from..x.start + to),
            ListSlice::Unknown(_) => ListSlice::Unknown(to - from),
        }
    }
}

impl Mergable for ListSlice {
    fn is_mergable(&self, other: &Self, _: &()) -> bool {
        match (self, other) {
            (ListSlice::Slice(x), ListSlice::Slice(y)) => x.end == y.start,
            (ListSlice::Unknown(_), ListSlice::Unknown(_)) => true,
            _ => false,
        }
    }

    fn merge(&mut self, other: &Self, _: &()) {
        match (self, other) {
            (ListSlice::Slice(x), ListSlice::Slice(y)) => {
                *x = x.start..y.end;
            }
            (ListSlice::Unknown(x), ListSlice::Unknown(y)) => {
                *x += y;
            }
            _ => unreachable!(),
        }
    }
}

use std::ops::Range;

use enum_as_inner::EnumAsInner;
use rle::{rle_tree::tree_trait::CumulateTreeTrait, HasLength, Mergable, RleVec, Sliceable};

use crate::{id::ID, smstring::SmString};

#[derive(PartialEq, Eq, Debug, EnumAsInner, Clone)]
pub(crate) enum ListSlice {
    RawStr(SmString),
    // TODO: Use small compact rle vec
    Slice(RleVec<Range<usize>>),
    Unknown(usize),
}

impl ListSlice {
    #[inline(always)]
    pub fn from_range(range: Range<usize>) -> ListSlice {
        let mut v = RleVec::new();
        v.push(range);
        Self::Slice(v)
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
            ListSlice::Slice(x) => x.len(),
            ListSlice::Unknown(x) => *x,
        }
    }
}

impl Sliceable for ListSlice {
    fn slice(&self, from: usize, to: usize) -> Self {
        match self {
            ListSlice::RawStr(s) => {
                let mut new_s = SmString::new();
                new_s.push_str(&s[from..to]);
                ListSlice::RawStr(new_s)
            }
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
            _ => false,
        }
    }

    fn merge(&mut self, other: &Self, _: &()) {
        match (self, other) {
            (ListSlice::Slice(x), ListSlice::Slice(y)) => x.merge(y, &()),
            (ListSlice::Unknown(x), ListSlice::Unknown(y)) => {
                *x += y;
            }
            _ => unreachable!(),
        }
    }
}

pub(super) type ListSliceTreeTrait = CumulateTreeTrait<ListSlice, 8>;

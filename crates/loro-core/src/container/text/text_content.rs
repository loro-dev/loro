use std::ops::Range;

use enum_as_inner::EnumAsInner;
use rle::{HasLength, Mergable, Sliceable};

use crate::id::ID;

#[derive(PartialEq, Eq, Debug, EnumAsInner, Clone)]
pub(super) enum TextPointer {
    Slice(Range<usize>),
    Unknown(usize),
}

#[derive(Debug, EnumAsInner)]
pub(super) enum TextOpContent {
    Insert {
        id: ID,
        text: TextPointer,
        pos: usize,
    },
    Delete {
        id: ID,
        pos: usize,
        len: usize,
    },
}

pub(super) fn new_unknown_text(len: usize) -> TextPointer {
    TextPointer::Unknown(len)
}

pub(super) fn is_unknown_text(a: &TextPointer) -> bool {
    a.as_unknown().is_some()
}

impl HasLength for TextPointer {
    fn len(&self) -> usize {
        match self {
            TextPointer::Slice(x) => std::iter::ExactSizeIterator::len(x),
            TextPointer::Unknown(x) => *x,
        }
    }
}

impl Sliceable for TextPointer {
    fn slice(&self, from: usize, to: usize) -> Self {
        match self {
            TextPointer::Slice(x) => TextPointer::Slice(x.start + from..x.start + to),
            TextPointer::Unknown(_) => TextPointer::Unknown(to - from),
        }
    }
}

impl Mergable for TextPointer {
    fn is_mergable(&self, other: &Self, _: &()) -> bool {
        match (self, other) {
            (TextPointer::Slice(x), TextPointer::Slice(y)) => x.end == y.start,
            (TextPointer::Unknown(_), TextPointer::Unknown(_)) => true,
            _ => false,
        }
    }

    fn merge(&mut self, other: &Self, _: &()) {
        match (self, other) {
            (TextPointer::Slice(x), TextPointer::Slice(y)) => {
                *x = x.start..y.end;
            }
            (TextPointer::Unknown(x), TextPointer::Unknown(y)) => {
                *x += y;
            }
            _ => unreachable!(),
        }
    }
}

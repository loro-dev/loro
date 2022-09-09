use std::ops::Range;

use enum_as_inner::EnumAsInner;

use crate::id::ID;

pub(super) type TextPointer = Range<usize>;

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

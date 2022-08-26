use std::ops::Range;

use enum_as_inner::EnumAsInner;

use crate::id::ID;

#[derive(Debug, EnumAsInner)]
pub(super) enum TextContent {
    Insert {
        id: ID,
        text: Range<usize>,
        pos: usize,
    },
    Delete {
        id: ID,
        pos: usize,
        len: usize,
    },
}

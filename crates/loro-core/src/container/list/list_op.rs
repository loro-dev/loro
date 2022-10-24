use enum_as_inner::EnumAsInner;
use rle::{HasLength, Mergable, Sliceable};

use crate::container::text::text_content::ListSlice;

#[derive(EnumAsInner, Debug, Clone)]
pub(crate) enum ListOp {
    Insert { slice: ListSlice, pos: usize },
    Delete { pos: usize, len: usize },
}

impl Mergable for ListOp {
    fn is_mergable(&self, _other: &Self, _conf: &()) -> bool
    where
        Self: Sized,
    {
        match self {
            ListOp::Insert { pos, slice } => match _other {
                ListOp::Insert {
                    pos: other_pos,
                    slice: other_slice,
                } => pos + slice.content_len() == *other_pos && slice.is_mergable(other_slice, &()),
                _ => false,
            },
            // TODO: add support for reverse merge
            ListOp::Delete { pos, len: _ } => match _other {
                ListOp::Delete { pos: other_pos, .. } => *pos == *other_pos,
                _ => false,
            },
        }
    }

    fn merge(&mut self, _other: &Self, _conf: &())
    where
        Self: Sized,
    {
        match self {
            ListOp::Insert { slice, .. } => match _other {
                ListOp::Insert {
                    slice: other_slice, ..
                } => {
                    slice.merge(other_slice, &());
                }
                _ => unreachable!(),
            },
            ListOp::Delete { len, .. } => match _other {
                ListOp::Delete { len: other_len, .. } => {
                    *len += other_len;
                }
                _ => unreachable!(),
            },
        }
    }
}

impl HasLength for ListOp {
    fn content_len(&self) -> usize {
        match self {
            ListOp::Insert { slice, .. } => slice.content_len(),
            ListOp::Delete { len, .. } => *len,
        }
    }
}

impl Sliceable for ListOp {
    fn slice(&self, from: usize, to: usize) -> Self {
        match self {
            ListOp::Insert { slice, pos } => ListOp::Insert {
                slice: slice.slice(from, to),
                pos: *pos + from,
            },
            ListOp::Delete { pos, .. } => ListOp::Delete {
                // this looks weird but it's correct
                // because right now two adjacent delete can be merge
                // only when they delete at the same position.
                pos: *pos,
                len: to - from,
            },
        }
    }
}

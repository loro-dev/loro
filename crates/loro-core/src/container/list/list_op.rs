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
                } => pos + slice.len() == *other_pos && slice.is_mergable(other_slice, &()),
                _ => false,
            },
            ListOp::Delete { pos, len } => match _other {
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
    fn len(&self) -> usize {
        match self {
            ListOp::Insert { slice, .. } => slice.len(),
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
                pos: *pos + from,
                len: to - from,
            },
        }
    }
}

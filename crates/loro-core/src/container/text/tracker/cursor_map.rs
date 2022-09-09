use std::{fmt::Debug, ptr::NonNull};

use enum_as_inner::EnumAsInner;

use rle::{range_map::RangeMap, rle_tree::node::LeafNode, HasLength, Mergable, Sliceable};

use crate::span::IdSpan;

use super::y_span::{YSpan, YSpanTreeTrait};

#[non_exhaustive]
#[derive(Debug, Clone, EnumAsInner)]
pub(super) enum Marker {
    Insert {
        ptr: NonNull<LeafNode<'static, YSpan, YSpanTreeTrait>>,
        len: usize,
    },
    Delete(IdSpan),
    // TODO: REDO, UNDO
}

impl Sliceable for Marker {
    fn slice(&self, from: usize, to: usize) -> Self {
        match self {
            Marker::Insert { ptr, .. } => Marker::Insert {
                ptr: *ptr,
                len: to - from,
            },
            Marker::Delete(x) => Marker::Delete(x.slice(from, to)),
        }
    }
}

impl HasLength for Marker {
    fn len(&self) -> usize {
        match self {
            Marker::Insert { ptr, len } => *len,
            Marker::Delete(span) => span.len(),
        }
    }
}

impl Mergable for Marker {
    fn is_mergable(&self, other: &Self, _conf: &()) -> bool
    where
        Self: Sized,
    {
        match self {
            Marker::Insert { ptr: x, .. } => match other {
                Marker::Insert { ptr: y, .. } => x == y,
                _ => false,
            },
            Marker::Delete(x) => match other {
                Marker::Delete(y) => x.is_mergable(y, &()),
                _ => false,
            },
        }
    }

    fn merge(&mut self, other: &Self, _conf: &())
    where
        Self: Sized,
    {
        match self {
            Marker::Insert { ptr: _, len } => *len += other.as_insert().unwrap().1,
            Marker::Delete(x) => x.merge(other.as_delete().unwrap(), &()),
        }
    }
}

pub(super) type CursorMap = RangeMap<u128, Marker>;

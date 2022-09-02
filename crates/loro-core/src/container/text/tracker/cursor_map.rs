use std::{fmt::Debug, ptr::NonNull};

use enum_as_inner::EnumAsInner;

use rle::{range_map::RangeMap, rle_tree::node::LeafNode, HasLength, Mergable, Sliceable};

use crate::span::IdSpan;

use super::y_span::{YSpan, YSpanTreeTrait};

#[derive(Debug, Clone, EnumAsInner)]
pub(super) enum Marker {
    Insert(NonNull<LeafNode<'static, YSpan, YSpanTreeTrait>>),
    Delete(IdSpan),
}

impl Sliceable for Marker {
    fn slice(&self, from: usize, to: usize) -> Self {
        match self {
            Marker::Insert(x) => Marker::Insert(*x),
            Marker::Delete(x) => Marker::Delete(x.slice(from, to)),
        }
    }
}

impl HasLength for Marker {
    fn len(&self) -> usize {
        todo!()
    }
}

impl Mergable for Marker {
    fn is_mergable(&self, other: &Self, _conf: &()) -> bool
    where
        Self: Sized,
    {
        match self {
            Marker::Insert(x) => match other {
                Marker::Insert(y) => x == y,
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
        if let Marker::Delete(x) = self {
            x.merge(other.as_delete().unwrap(), &())
        }
    }
}

pub(super) type CursorMap = RangeMap<u128, Marker>;

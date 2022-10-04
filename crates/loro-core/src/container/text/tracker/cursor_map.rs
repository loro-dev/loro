use std::{fmt::Debug, ptr::NonNull};

use enum_as_inner::EnumAsInner;

use rle::{
    range_map::RangeMap,
    rle_tree::{node::LeafNode, Position, SafeCursor, SafeCursorMut},
    HasLength, Mergable, Sliceable,
};

use crate::{id::ID, span::IdSpan};

use super::y_span::{YSpan, YSpanTreeTrait};

// marker can only live while the bumpalo is alive. So we are safe to use 'static here
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

impl Marker {
    pub fn as_cursor(&self, id: ID) -> Option<SafeCursor<'_, 'static, YSpan, YSpanTreeTrait>> {
        match self {
            Marker::Insert { ptr, len: _ } => {
                // SAFETY: tree data is always valid
                let node = unsafe { ptr.as_ref() };
                debug_assert!(!node.is_deleted());
                let position = node.children().iter().position(|x| x.contain_id(id))?;
                // SAFETY: we just checked it is valid
                Some(unsafe {
                    SafeCursor::new(
                        *ptr,
                        position,
                        0,
                        rle::rle_tree::Position::Start,
                        self.len(),
                    )
                })
            }
            Marker::Delete(_) => None,
        }
    }

    pub fn as_cursor_mut(
        &mut self,
        id: ID,
    ) -> Option<SafeCursorMut<'_, 'static, YSpan, YSpanTreeTrait>> {
        match self {
            Marker::Insert { ptr, len: _ } => {
                // SAFETY: tree data is always valid
                let node = unsafe { ptr.as_ref() };
                debug_assert!(!node.is_deleted());
                let position = node.children().iter().position(|x| x.contain_id(id))?;
                // SAFETY: we just checked it is valid
                Some(unsafe { SafeCursorMut::new(*ptr, position, 0, Position::Start, self.len()) })
            }
            Marker::Delete(_) => None,
        }
    }
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
            Marker::Insert { ptr: _, len } => *len,
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

pub(super) fn make_notify(
    map: &mut CursorMap,
) -> impl for<'a> FnMut(&YSpan, *mut LeafNode<'a, YSpan, YSpanTreeTrait>) + '_ {
    |span, leaf| {
        map.set(
            span.id.into(),
            Marker::Insert {
                // SAFETY: marker can only live while the bumpalo is alive. so we are safe to change lifetime here
                ptr: unsafe {
                    NonNull::new_unchecked(leaf as usize as *mut LeafNode<'static, _, _>)
                },
                len: span.len(),
            },
        )
    }
}

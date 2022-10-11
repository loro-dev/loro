use std::{fmt::Debug, ptr::NonNull};

use enum_as_inner::EnumAsInner;

use rle::{
    range_map::RangeMap,
    rle_tree::{node::LeafNode, Position, SafeCursor, SafeCursorMut, UnsafeCursor},
    HasLength, Mergable, RleVec, Sliceable,
};

use crate::{id::ID, span::IdSpan};

use super::y_span::{YSpan, YSpanTreeTrait};

// marker can only live while the bumpalo is alive. So we are safe to use 'static here
#[non_exhaustive]
#[derive(Debug, Clone, EnumAsInner, PartialEq, Eq)]
pub(super) enum Marker {
    Insert {
        ptr: NonNull<LeafNode<'static, YSpan, YSpanTreeTrait>>,
        len: usize,
    },
    Delete(RleVec<IdSpan>),
    // TODO: REDO, UNDO
}

impl Marker {
    pub fn as_cursor(&self, id: ID) -> Option<SafeCursor<'static, YSpan, YSpanTreeTrait>> {
        match self {
            Marker::Insert { ptr, len: _ } => {
                // SAFETY: tree data is always valid
                let node = unsafe { ptr.as_ref() };
                if node.is_deleted() {
                    dbg!(&node);
                }
                debug_assert!(!node.is_deleted());
                let position = node.children().iter().position(|x| x.contain_id(id))?;
                let child = &node.children()[position];
                let start_counter = child.id.counter;
                let offset = id.counter - start_counter;
                // SAFETY: we just checked it is valid
                Some(unsafe {
                    SafeCursor::new(
                        *ptr,
                        position,
                        offset as usize,
                        Position::from_offset(offset as isize, child.content_len()),
                        0,
                    )
                })
            }
            Marker::Delete(_) => None,
        }
    }

    pub fn get_spans(&self, id_span: IdSpan) -> Vec<UnsafeCursor<'static, YSpan, YSpanTreeTrait>> {
        match self {
            Marker::Insert { ptr, len: _ } => {
                // SAFETY: tree data is always valid
                let node = unsafe { ptr.as_ref() };
                debug_assert!(!node.is_deleted());
                node.children()
                    .iter()
                    .enumerate()
                    .filter_map(|(i, child)| {
                        if child.overlap(id_span) {
                            let start_counter = child.id.counter;
                            let offset = std::cmp::max(id_span.counter.min() - start_counter, 0);
                            debug_assert!((offset as usize) < child.len);
                            let max_offset = std::cmp::min(
                                id_span.counter.max() - start_counter,
                                (child.len - 1) as i32,
                            );
                            let len = max_offset - offset + 1;
                            // SAFETY: we just checked it is valid
                            Some(unsafe {
                                std::mem::transmute(UnsafeCursor::new(
                                    *ptr,
                                    i,
                                    offset as usize,
                                    Position::from_offset(offset as isize, child.len),
                                    len as usize,
                                ))
                            })
                        } else {
                            None
                        }
                    })
                    .collect()
            }
            Marker::Delete(_) => unreachable!(),
        }
    }

    /// # Safety
    ///
    /// It's safe when you are sure that the returned cursor is the only reference to the target yspan tree
    pub unsafe fn as_cursor_mut(
        &mut self,
        id: ID,
    ) -> Option<SafeCursorMut<'static, YSpan, YSpanTreeTrait>> {
        match self {
            Marker::Insert { ptr, len: _ } => {
                let node = ptr.as_ref();
                debug_assert!(!node.is_deleted());
                let position = node.children().iter().position(|x| x.contain_id(id))?;
                let child = &node.children()[position];
                let start_counter = child.id.counter;
                let offset = id.counter - start_counter;
                Some(SafeCursorMut::new(
                    *ptr,
                    position,
                    offset as usize,
                    Position::from_offset(offset as isize, child.content_len()),
                    0,
                ))
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
                len: span.content_len(),
            },
        )
    }
}

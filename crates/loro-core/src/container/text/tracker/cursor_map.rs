use std::{
    fmt::Debug,
    ops::{Deref, DerefMut},
    ptr::NonNull,
};

use enum_as_inner::EnumAsInner;

use rle::{
    range_map::RangeMap,
    rle_tree::{node::LeafNode, Position, SafeCursor, SafeCursorMut, UnsafeCursor},
    HasLength, Mergable, RleVecWithLen, Sliceable, ZeroElement,
};

use crate::{
    debug_log,
    id::{Counter, ID},
    span::{HasId, HasIdSpan, IdSpan},
};

use super::y_span::{YSpan, YSpanTreeTrait};

// marker can only live while the bumpalo is alive. So we are safe to use 'static here
#[non_exhaustive]
#[derive(Debug, Clone, EnumAsInner, PartialEq, Eq)]
pub(super) enum Marker {
    Insert {
        ptr: NonNull<LeafNode<'static, YSpan, YSpanTreeTrait>>,
        len: usize,
    },
    Delete(Box<RleVecWithLen<[IdSpan; 2]>>),
    // FUTURE: REDO, UNDO
}

impl ZeroElement for Marker {
    fn zero_element() -> Self {
        Self::Insert {
            ptr: NonNull::dangling(),
            len: 0,
        }
    }
}

impl Marker {
    pub(super) fn as_cursor<'a, 'b>(
        &'a self,
        id: ID,
    ) -> Option<SafeCursor<'b, YSpan, YSpanTreeTrait>> {
        match self {
            Marker::Insert { ptr, len: _ } => {
                // SAFETY: tree data is always valid
                let node = unsafe { ptr.as_ref() };
                let position = node.children().iter().position(|x| x.contain_id(id))?;
                let child = &node.children()[position];
                let start_counter = child.id.counter;
                let offset = id.counter - start_counter;
                // SAFETY: we transform lifetime from SafeCursor<'static> to SafeCursor<'b> to suit the need.
                // Its safety is guaranteed by the caller, who has access to the underlying tree
                unsafe {
                    std::mem::transmute(Some(SafeCursor::from_leaf(
                        node,
                        position,
                        offset as usize,
                        Position::from_offset(offset as isize, child.atom_len()),
                        0,
                    )))
                }
            }
            Marker::Delete(_) => None,
        }
    }

    pub fn get_first_span(
        &self,
        id_span: IdSpan,
    ) -> Option<UnsafeCursor<'static, YSpan, YSpanTreeTrait>> {
        let mut ans = self.get_spans(id_span);
        // SAFETY: inner invariants ensures that the cursor is valid
        ans.sort_by_cached_key(|x| unsafe { x.as_ref() }.id.counter);
        ans.into_iter().next()
    }

    pub fn get_spans(&self, id_span: IdSpan) -> Vec<UnsafeCursor<'static, YSpan, YSpanTreeTrait>> {
        match self {
            Marker::Insert { ptr, len: _ } => {
                // SAFETY: tree data is always valid
                let node = unsafe { ptr.as_ref() };
                node.children()
                    .iter()
                    .enumerate()
                    .filter_map(|(i, child)| {
                        if child.overlap(id_span) {
                            let start_counter = child.id.counter;
                            let offset = std::cmp::max(id_span.counter.min() - start_counter, 0);
                            debug_assert!((offset as usize) < child.atom_len());
                            let max_offset = std::cmp::min(
                                id_span.counter.max() - start_counter,
                                (child.atom_len() - 1) as i32,
                            );
                            let len = max_offset - offset + 1;
                            // SAFETY: we just checked it is valid
                            Some(unsafe {
                                std::mem::transmute(UnsafeCursor::new(
                                    *ptr,
                                    i,
                                    offset as usize,
                                    Position::from_offset(offset as isize, child.atom_len()),
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
                let position = node.children().iter().position(|x| x.contain_id(id))?;
                let child = &node.children()[position];
                let start_counter = child.id.counter;
                let offset = id.counter - start_counter;
                Some(SafeCursorMut::new(
                    *ptr,
                    position,
                    offset as usize,
                    Position::from_offset(offset as isize, child.atom_len()),
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
            Marker::Delete(x) => Marker::Delete(Box::new(x.slice(from, to))),
        }
    }
}

impl HasLength for Marker {
    fn content_len(&self) -> usize {
        match self {
            Marker::Insert { ptr: _, len } => *len,
            Marker::Delete(span) => span.atom_len(),
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

#[derive(Debug, Default)]
pub(super) struct CursorMap(RangeMap<u128, Marker>);

impl Deref for CursorMap {
    type Target = RangeMap<u128, Marker>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl DerefMut for CursorMap {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

pub(super) fn make_notify(
    map: &mut CursorMap,
) -> impl for<'a> FnMut(&YSpan, *mut LeafNode<'a, YSpan, YSpanTreeTrait>) + '_ {
    |span, leaf| {
        map.set_small_range(
            span.id.into(),
            Marker::Insert {
                // SAFETY: marker can only live while the bumpalo is alive. so we are safe to change lifetime here
                ptr: unsafe {
                    NonNull::new_unchecked(leaf as usize as *mut LeafNode<'static, _, _>)
                },
                len: span.atom_len(),
            },
        );
    }
}

pub(super) struct IdSpanQueryResult {
    pub inserts: Vec<(ID, UnsafeCursor<'static, YSpan, YSpanTreeTrait>)>,
    pub deletes: Vec<(ID, RleVecWithLen<[IdSpan; 2]>)>,
}

#[derive(EnumAsInner, Debug)]
pub enum FirstCursorResult {
    // TODO: REMOVE id field?
    Ins(ID, UnsafeCursor<'static, YSpan, YSpanTreeTrait>),
    Del(ID, RleVecWithLen<[IdSpan; 2]>),
}

impl CursorMap {
    // FIXME:
    pub fn get_cursors_at_id_span(&self, span: IdSpan) -> IdSpanQueryResult {
        let mut inserts: Vec<(ID, UnsafeCursor<'static, YSpan, YSpanTreeTrait>)> =
            Vec::with_capacity(span.atom_len() / 10);
        let mut deletes: Vec<(ID, RleVecWithLen<[IdSpan; 2]>)> =
            Vec::with_capacity(span.atom_len() / 10);
        let mut inserted_set = fxhash::FxHashSet::default();
        for (id, marker) in self.get_range_with_index(span.min_id().into(), span.end_id().into()) {
            let id: ID = id.into();
            match marker {
                Marker::Insert { .. } => {
                    for cursor in marker.get_spans(span) {
                        // SAFETY: invariants
                        let sliced = unsafe { cursor.get_sliced() };
                        if cfg!(test) {
                            assert!(span.contains_id(sliced.id));
                            assert!(span.contains_id(sliced.last_id()));
                        }
                        if !inserted_set.contains(&cursor) {
                            inserted_set.insert(cursor.clone());
                            inserts.push((sliced.id, cursor));
                        }
                    }
                }
                Marker::Delete(del) => {
                    if span.intersect(&id.to_span(del.atom_len())) {
                        let from = (span.counter.min() - id.counter).max(0);
                        let to = (span.counter.end() - id.counter).min(del.atom_len() as Counter);
                        if to - from > 0 {
                            deletes.push((id.inc(from), del.slice(from as usize, to as usize)));
                        }
                    }
                }
            }
        }

        IdSpanQueryResult { inserts, deletes }
    }

    pub fn get_first_cursors_at_id_span(&self, span: IdSpan) -> Option<FirstCursorResult> {
        for (id, marker) in self.get_range_with_index(span.min_id().into(), span.end_id().into()) {
            let id: ID = id.into();
            match marker {
                Marker::Insert { .. } => {
                    if let Some(cursor) = marker.get_first_span(span) {
                        return Some(FirstCursorResult::Ins(span.id_start(), cursor));
                    }
                }
                Marker::Delete(del) => {
                    return Some(FirstCursorResult::Del(
                        span.id_start(),
                        del.slice(
                            (span.id_start().counter - id.counter).max(0) as usize,
                            del.atom_len(),
                        ),
                    ));
                }
            }
        }

        None
    }
}

use std::ops::{Deref, DerefMut};

use rle::{
    rle_tree::{node::LeafNode, Position, SafeCursor, SafeCursorMut, UnsafeCursor},
    HasLength, RleTree,
};

use crate::id::ID;

use super::y_span::{StatusChange, YSpan, YSpanTreeTrait};

/// It stores all the [YSpan] data, including the deleted/undo ones
///
/// Its internal state, acquired by traveling from begin to end, represents the **current** state of the tree
#[repr(transparent)]
#[derive(Debug, Default)]
pub(super) struct ContentMap(RleTree<YSpan, YSpanTreeTrait>);

struct CursorWithId<'tree> {
    id: ID,
    cursor: UnsafeCursor<'tree, 'static, YSpan, YSpanTreeTrait>,
}

impl ContentMap {
    #[inline]
    pub(super) fn insert_yspan_at_pos<F>(&mut self, id: ID, pos: usize, len: usize, notify: &mut F)
    where
        F: FnMut(&YSpan, *const LeafNode<'_, YSpan, YSpanTreeTrait>),
    {
        let (left, right) = self.get_sibling_at(pos);
        let yspan = YSpan {
            origin_left: left.as_ref().map(|x| x.id).unwrap_or_else(ID::null),
            origin_right: right.as_ref().map(|x| x.id).unwrap_or_else(ID::null),
            id,
            len,
            status: Default::default(),
        };

        // TODO: insert between left & right
    }

    /// When we insert a new [YSpan] at given position, we need to calculate its `originLeft` and `originRight`
    fn get_sibling_at(&self, pos: usize) -> (Option<CursorWithId<'_>>, Option<CursorWithId<'_>>) {
        self.with_tree(|tree| {
            if let Some(cursor) = tree.get(pos) {
                let cursor: SafeCursor<'_, 'static, YSpan, YSpanTreeTrait> =
                    // SAFETY: we only change the lifetime of the cursor; the returned lifetime is kinda wrong in this situation 
                    // because Bumpalo's lifetime is static due to the self-referential structure limitation; Maybe there is a better way?
                    unsafe { std::mem::transmute(cursor) };
                let (mut prev, mut next) = match cursor.pos() {
                    Position::Start => {
                        if cursor.as_ref().can_be_origin() {
                            let id = cursor.as_ref().id;
                            (
                                None,
                                Some(CursorWithId {
                                    id,
                                    cursor: cursor.unwrap(),
                                }),
                            )
                        } else {
                            (None, None)
                        }
                    }
                    Position::Middle => {
                        if cursor.as_ref().can_be_origin() {
                            let id = cursor.as_ref().id;
                            let offset = cursor.offset();
                            let mut prev_offset_cursor = cursor.unwrap();
                            prev_offset_cursor.offset -= 1;
                            (
                                Some(CursorWithId {
                                    id: id.inc(offset as i32 - 1),
                                    cursor: prev_offset_cursor,
                                }),
                                Some(CursorWithId {
                                    id: id.inc(offset as i32),
                                    cursor: cursor.unwrap(),
                                }),
                            )
                        } else {
                            (None, None)
                        }
                    }
                    Position::End => {
                        if cursor.as_ref().can_be_origin() {
                            let mut prev_offset_cursor = cursor.unwrap();
                            prev_offset_cursor.offset -= 1;
                            (
                                Some(CursorWithId {
                                    id: cursor.as_ref().last_id(),
                                    cursor: prev_offset_cursor,
                                }),
                                None,
                            )
                        } else {
                            (None, None)
                        }
                    }
                    _ => {
                        unreachable!()
                    }
                };

                if prev.is_none() {
                    let mut prev_cursor = cursor.prev();
                    while let Some(prev_inner) = prev_cursor {
                        if prev_inner.as_ref().status.is_activated() {
                            let cursor = prev_inner;
                            let offset = cursor.as_ref().len() - 1;
                            let mut cursor = cursor.unwrap();
                            cursor.offset = offset;
                            cursor.pos = Position::Middle;
                            prev = Some(CursorWithId {
                                id: prev_inner.as_ref().last_id(),
                                cursor,
                            });
                            break;
                        }
                        prev_cursor = prev_inner.prev();
                    }
                }

                if next.is_none() {
                    let mut next_cursor = cursor.next();
                    while let Some(next_inner) = next_cursor {
                        if next_inner.as_ref().status.is_activated() {
                            let mut cursor = next_inner.unwrap();
                            cursor.offset = 0;
                            cursor.pos = Position::Start;
                            next = Some(CursorWithId {
                                id: next_inner.as_ref().id,
                                cursor,
                            });
                            break;
                        }
                        next_cursor = next_inner.next();
                    }
                }

                (prev, next)
            } else {
                (None, None)
            }
        })
    }
}

impl Deref for ContentMap {
    type Target = RleTree<YSpan, YSpanTreeTrait>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for ContentMap {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

pub(super) fn change_status<'a, 'b: 'a>(
    cursor: &mut SafeCursorMut<'a, 'b, YSpan, YSpanTreeTrait>,
    change: StatusChange,
) {
    let value = cursor.as_mut();
    if value.status.apply(change) {
        cursor.update_cache_recursively();
    }
}

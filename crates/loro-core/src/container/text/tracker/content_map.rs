use std::ops::{Deref, DerefMut};

use rle::{
    rle_tree::{Position, SafeCursor, SafeCursorMut, UnsafeCursor},
    HasLength, RleTree, RleVec,
};

use crate::{
    id::{Counter, ID},
    span::IdSpan,
};

use super::y_span::{StatusChange, YSpan, YSpanTreeTrait};

/// It stores all the [YSpan] data, including the deleted/undo ones
///
/// Its internal state, acquired by traveling from begin to end, represents the **current** state of the tree
#[repr(transparent)]
#[derive(Debug, Default)]
pub(super) struct ContentMap(RleTree<YSpan, YSpanTreeTrait>);

struct CursorWithId<'tree> {
    id: ID,
    cursor: UnsafeCursor<'tree, YSpan, YSpanTreeTrait>,
}

impl ContentMap {
    #[inline]
    pub(super) fn get_yspan_at_pos(&mut self, id: ID, pos: usize, len: usize) -> YSpan {
        let (left, right) = self.get_sibling_at(pos);
        YSpan {
            origin_left: left.as_ref().map(|x| x.id),
            origin_right: right.as_ref().map(|x| x.id),
            id,
            len,
            status: Default::default(),
        }
    }

    /// When we insert a new [YSpan] at given position, we need to calculate its `originLeft` and `originRight`
    fn get_sibling_at(&self, pos: usize) -> (Option<CursorWithId<'_>>, Option<CursorWithId<'_>>) {
        if let Some(cursor) = self.get(pos) {
            let cursor: SafeCursor<'_, YSpan, YSpanTreeTrait> =
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
                let mut prev_cursor = cursor.prev_elem_end();
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
                    prev_cursor = prev_inner.prev_elem_end();
                }
            }

            if next.is_none() {
                let mut next_cursor = cursor.next_elem_start();
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
                    next_cursor = next_inner.next_elem_start();
                }
            }

            (prev, next)
        } else {
            (None, None)
        }
    }

    pub fn get_id_spans(&self, pos: usize, len: usize) -> RleVec<IdSpan> {
        let mut ans = RleVec::new();
        for cursor in self.iter_range(pos, Some(pos + len)) {
            ans.push(IdSpan::new(
                cursor.id.client_id,
                cursor.id.counter,
                cursor.id.counter + cursor.len as Counter,
            ));
        }

        ans
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

pub(super) fn change_status(
    cursor: &mut SafeCursorMut<'_, YSpan, YSpanTreeTrait>,
    change: StatusChange,
) {
    let value = cursor.as_mut();
    if value.status.apply(change) {
        cursor.update_cache_recursively();
    }
}

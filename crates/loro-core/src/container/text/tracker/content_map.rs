use std::ops::{Deref, DerefMut};

use rle::{
    rle_tree::{Position, SafeCursor, SafeCursorMut},
    HasLength, RleTree, RleVec,
};

use crate::{id::ID, span::IdSpan};

use super::y_span::{StatusChange, YSpan, YSpanTreeTrait};

/// It stores all the [YSpan] data, including the deleted/undo ones
///
/// Its internal state, acquired by traveling from begin to end, represents the **current** state of the tree
#[repr(transparent)]
#[derive(Debug, Default)]
pub(super) struct ContentMap(RleTree<YSpan, YSpanTreeTrait>);

impl ContentMap {
    #[inline]
    pub(super) fn get_yspan_at_pos(&self, id: ID, pos: usize, len: usize) -> YSpan {
        let (left, right) = self.get_sibling_at(pos);
        YSpan {
            origin_left: left,
            origin_right: right,
            id,
            len,
            status: Default::default(),
        }
    }

    fn get_sibling_at(&self, pos: usize) -> (Option<ID>, Option<ID>) {
        if let Some(cursor) = self.get(pos) {
            let mut cursor: SafeCursor<'_, YSpan, YSpanTreeTrait> =
                    // SAFETY: we only change the lifetime of the cursor; the returned lifetime is kinda wrong in this situation 
                    // because Bumpalo's lifetime is static due to the self-referential structure limitation; Maybe there is a better way?
                    unsafe { std::mem::transmute(cursor) };
            let mut prev = match cursor.pos() {
                Position::Start => None,
                Position::Middle => {
                    let id = cursor.as_ref().id;
                    let offset = cursor.offset();
                    let mut prev_offset_cursor = cursor.unwrap();
                    prev_offset_cursor.offset -= 1;
                    if cursor.as_ref().can_be_origin() {
                        return (Some(id.inc(offset as i32 - 1)), Some(id.inc(offset as i32)));
                    } else {
                        None
                    }
                }
                Position::End => {
                    if cursor.as_ref().can_be_origin() {
                        let mut prev_offset_cursor = cursor.unwrap();
                        prev_offset_cursor.offset -= 1;
                        prev_offset_cursor.pos = Position::Middle;
                        Some(cursor.as_ref().last_id())
                    } else {
                        None
                    }
                }
                _ => {
                    unreachable!()
                }
            };

            if prev.is_none() {
                let mut prev_cursor = cursor.prev_elem();
                while let Some(prev_inner) = prev_cursor {
                    cursor = prev_inner;
                    if prev_inner.as_ref().status.is_activated() {
                        let cursor = prev_inner;
                        let offset = cursor.as_ref().content_len() - 1;
                        let mut cursor = cursor.unwrap();
                        cursor.offset = offset;
                        cursor.pos = Position::Middle;
                        prev = Some(prev_inner.as_ref().last_id());
                        break;
                    }
                    prev_cursor = prev_inner.prev_elem();
                }
            }

            if prev.is_some() {
                let mut next_cursor = cursor.next_elem_start();
                let mut ans = None;
                while let Some(next_inner) = next_cursor {
                    if next_inner.as_ref().status.future {
                        let mut cursor = next_inner.unwrap();
                        cursor.offset = 0;
                        cursor.pos = Position::Start;
                        ans = Some(next_inner.as_ref().id);
                        break;
                    }

                    next_cursor = next_inner.next_elem_start();
                }

                (prev, ans)
            } else {
                while cursor.as_ref().status.future {
                    if let Some(next) = cursor.next_elem_start() {
                        cursor = next;
                    } else {
                        return (prev, None);
                    }
                }

                (prev, Some(cursor.as_ref().id))
            }
        } else {
            (None, None)
        }
    }

    pub fn get_id_spans(&self, pos: usize, len: usize) -> RleVec<IdSpan> {
        let mut ans = RleVec::new();
        for cursor in self.iter_range(pos, Some(pos + len)) {
            let id = cursor.as_ref().id;
            let cursor = cursor.unwrap();
            ans.push(IdSpan::new(
                id.client_id,
                id.counter + (cursor.offset as i32),
                id.counter + (cursor.offset + cursor.len) as i32,
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

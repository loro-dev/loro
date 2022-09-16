use std::ops::{Deref, DerefMut};

use rle::{
    rle_tree::{Position, SafeCursor, SafeCursorMut, UnsafeCursor},
    HasLength, RleTree,
};

use crate::{container::text::text_content::TextPointer, id::ID};

use super::y_span::{StatusChange, YSpan, YSpanTreeTrait};

#[repr(transparent)]
#[derive(Debug, Default)]
pub(super) struct ContentMap(RleTree<YSpan, YSpanTreeTrait>);

struct CursorWithId<'tree> {
    id: ID,
    cursor: UnsafeCursor<'tree, 'static, YSpan, YSpanTreeTrait>,
}

impl ContentMap {
    #[inline]
    pub fn new_yspan_at_pos(&mut self, id: ID, pos: usize, text: TextPointer) -> YSpan {
        let (left, right) = self.get_sibling_at(pos);
        YSpan {
            origin_left: left.map(|x| x.id).unwrap_or_else(ID::null),
            origin_right: right.map(|x| x.id).unwrap_or_else(ID::null),
            id,
            text,
            status: Default::default(),
        }
    }

    fn get_sibling_at(&self, pos: usize) -> (Option<CursorWithId<'_>>, Option<CursorWithId<'_>>) {
        self.with_tree(|tree| {
            if let Some(cursor) = tree.get(pos) {
                let cursor: SafeCursor<'_, 'static, YSpan, YSpanTreeTrait> =
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

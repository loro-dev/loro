use std::ops::{Deref, DerefMut};

use rle::{
    rle_tree::{Position, SafeCursor, SafeCursorMut, UnsafeCursor},
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

struct CursorWithId<'tree> {
    id: ID,
    cursor: UnsafeCursor<'tree, YSpan, YSpanTreeTrait>,
}

impl ContentMap {
    #[inline]
    pub(super) fn get_yspan_at_pos(&self, id: ID, pos: usize, len: usize) -> YSpan {
        let (left, right) = self.get_sibling_at_dumb(pos);
        YSpan {
            origin_left: left.as_ref().map(|x| x.id),
            origin_right: right.as_ref().map(|x| x.id),
            id,
            len,
            status: Default::default(),
        }
    }

    fn get_sibling_at_dumb(
        &self,
        pos: usize,
    ) -> (Option<CursorWithId<'_>>, Option<CursorWithId<'_>>) {
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
                        return (
                            Some(CursorWithId {
                                id: id.inc(offset as i32 - 1),
                                cursor: prev_offset_cursor,
                            }),
                            Some(CursorWithId {
                                id: id.inc(offset as i32),
                                cursor: cursor.unwrap(),
                            }),
                        );
                    } else {
                        None
                    }
                }
                Position::End => {
                    if cursor.as_ref().can_be_origin() {
                        let mut prev_offset_cursor = cursor.unwrap();
                        prev_offset_cursor.offset -= 1;
                        prev_offset_cursor.pos = Position::Middle;
                        Some(CursorWithId {
                            id: cursor.as_ref().last_id(),
                            cursor: prev_offset_cursor,
                        })
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
                        prev = Some(CursorWithId {
                            id: prev_inner.as_ref().last_id(),
                            cursor,
                        });
                        break;
                    }
                    prev_cursor = prev_inner.prev_elem();
                }
            }

            let next = if prev.is_some() {
                let mut next_cursor = cursor.next_elem_start();
                let mut ans = None;
                while let Some(next_inner) = next_cursor {
                    if next_inner.as_ref().status.is_activated() {
                        let mut cursor = next_inner.unwrap();
                        cursor.offset = 0;
                        cursor.pos = Position::Start;
                        ans = Some(CursorWithId {
                            id: next_inner.as_ref().id,
                            cursor,
                        });
                        break;
                    }

                    next_cursor = next_inner.next_elem_start();
                }

                ans
            } else {
                // if prev is none, next should be the first element in the tree
                let mut prev = cursor.prev_elem();
                while let Some(prev_inner) = prev {
                    cursor = prev_inner;
                    prev = prev_inner.prev_elem();
                }

                Some(CursorWithId {
                    id: cursor.as_ref().id,
                    cursor: cursor.unwrap(),
                })
            };

            (prev, next)
        } else {
            (None, None)
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
                        prev_offset_cursor.pos = Position::Middle;
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
                let mut prev_cursor = cursor.prev_elem();
                while let Some(prev_inner) = prev_cursor {
                    if prev_inner.as_ref().status.is_activated() {
                        let cursor = prev_inner;
                        let offset = cursor.as_ref().content_len() - 1;
                        let mut cursor = cursor.unwrap();
                        cursor.offset = offset;
                        cursor.pos = Position::Middle;
                        prev = Some(CursorWithId {
                            id: prev_inner.as_ref().last_id(),
                            cursor,
                        });
                        break;
                    }
                    prev_cursor = prev_inner.prev_elem();
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

#[cfg(test)]
mod test_get_yspan_at_pos {
    use crate::{
        container::text::tracker::y_span::{Status, YSpan},
        id::ID,
    };

    use super::ContentMap;

    fn insert(map: &mut ContentMap, id: ID, pos: usize, len: usize) {
        map.insert(
            pos,
            YSpan {
                id,
                len,
                status: Default::default(),
                origin_left: None,
                origin_right: None,
            },
        );
    }

    fn delete(map: &mut ContentMap, pos: usize, len: usize) {
        map.0.update_range(
            pos,
            Some(pos + len),
            &mut |v| v.status.delete_times = 1,
            &mut |_, _| {},
        )
    }

    fn insert_deleted(map: &mut ContentMap, id: ID, pos: usize, len: usize) {
        map.insert(
            pos,
            YSpan {
                id,
                len,
                status: Status {
                    delete_times: 1,
                    unapplied: false,
                    undo_times: 0,
                },
                origin_left: None,
                origin_right: None,
            },
        );
    }

    fn assert_at_pos(
        map: &ContentMap,
        pos: usize,
        origin_left: Option<ID>,
        origin_right: Option<ID>,
    ) {
        let ans = map.get_yspan_at_pos(ID::new(111, 11), pos, 1);
        assert_eq!(ans.origin_left, origin_left);
        assert_eq!(ans.origin_right, origin_right);
    }

    #[test]
    fn simple() {
        let mut map = ContentMap::default();
        insert(&mut map, ID::new(0, 0), 0, 10);
        assert_at_pos(&map, 0, None, Some(ID::new(0, 0)));
        assert_at_pos(&map, 10, Some(ID::new(0, 9)), None);
        assert_at_pos(&map, 3, Some(ID::new(0, 2)), Some(ID::new(0, 3)));
    }

    #[test]
    fn complicated() {
        let mut map = ContentMap::default();
        insert(&mut map, ID::new(0, 0), 0, 20);
        delete(&mut map, 10, 10);
        insert(&mut map, ID::new(1, 0), 10, 10);
        insert(&mut map, ID::new(2, 0), 20, 10);
        insert(&mut map, ID::new(3, 0), 30, 10);

        // dbg!(&map);
        assert_at_pos(&map, 10, Some(ID::new(0, 9)), Some(ID::new(1, 0)));
        assert_at_pos(&map, 11, Some(ID::new(1, 0)), Some(ID::new(1, 1)));

        assert_at_pos(&map, 20, Some(ID::new(1, 9)), Some(ID::new(2, 0)));
        assert_at_pos(&map, 21, Some(ID::new(2, 0)), Some(ID::new(2, 1)));
        delete(&mut map, 20, 1);
        assert_at_pos(&map, 20, Some(ID::new(1, 9)), Some(ID::new(2, 1)));
        assert_at_pos(&map, 21, Some(ID::new(2, 1)), Some(ID::new(2, 2)));

        delete(&mut map, 0, 10);
        assert_at_pos(&map, 0, None, Some(ID::new(0, 0)));
        assert_at_pos(&map, 29, Some(ID::new(3, 9)), None);
        delete(&mut map, 0, 28);
        assert_at_pos(&map, 1, Some(ID::new(3, 9)), None);
    }
}

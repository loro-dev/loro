use std::ops::ControlFlow;

use generic_btree::{
    rle::{HasLength as _, Sliceable},
    LeafIndex,
};
use loro_common::{Counter, HasId, HasIdSpan, IdFull, IdSpan, Lamport, PeerID, ID};
use rle::HasLength as _;
use tracing::instrument;

use crate::{cursor::AbsolutePosition, VersionVector};

use self::{crdt_rope::CrdtRope, id_to_cursor::IdToCursor};

use super::{
    fugue_span::{FugueSpan, Status},
    RichtextChunk,
};

mod crdt_rope;
mod id_to_cursor;
pub(crate) use crdt_rope::CrdtRopeDelta;

#[derive(Debug)]
pub(crate) struct Tracker {
    applied_vv: VersionVector,
    current_vv: VersionVector,
    rope: CrdtRope,
    id_to_cursor: IdToCursor,
}

impl Default for Tracker {
    fn default() -> Self {
        Self::new_with_unknown()
    }
}

pub(super) const UNKNOWN_PEER_ID: PeerID = u64::MAX;
impl Tracker {
    pub fn new_with_unknown() -> Self {
        let mut this = Self {
            rope: CrdtRope::new(),
            id_to_cursor: IdToCursor::default(),
            applied_vv: Default::default(),
            current_vv: Default::default(),
        };

        let result = this.rope.tree.push(FugueSpan {
            content: RichtextChunk::new_unknown(u32::MAX / 4),
            id: IdFull::new(UNKNOWN_PEER_ID, 0, 0),
            real_id: None,
            status: Status::default(),
            diff_status: None,
            origin_left: None,
            origin_right: None,
        });
        this.id_to_cursor.insert_without_split(
            ID::new(UNKNOWN_PEER_ID, 0),
            id_to_cursor::Cursor::new_insert(result.leaf, u32::MAX as usize / 4),
        );
        this
    }

    #[allow(unused)]
    fn new() -> Self {
        Self {
            rope: CrdtRope::new(),
            id_to_cursor: IdToCursor::default(),
            applied_vv: Default::default(),
            current_vv: Default::default(),
        }
    }

    #[inline]
    pub fn all_vv(&self) -> &VersionVector {
        &self.applied_vv
    }

    #[inline]
    pub fn current_vv(&self) -> &VersionVector {
        &self.current_vv
    }

    pub(crate) fn insert(&mut self, mut op_id: IdFull, mut pos: usize, mut content: RichtextChunk) {
        // trace!(
        //     "TrackerInsert op_id = {:#?}, pos = {:#?}, content = {:#?}",
        //     op_id,
        //     &pos,
        //     &content
        // );
        // tracing::span!(tracing::Level::INFO, "TrackerInsert");
        if let ControlFlow::Break(_) =
            self.skip_applied(op_id.id(), content.len(), |applied_counter_end| {
                // the op is partially included, need to slice the content
                let start = (applied_counter_end - op_id.counter) as usize;
                op_id.lamport += (applied_counter_end - op_id.counter) as Lamport;
                op_id.counter = applied_counter_end;
                pos += start;
                content = content.slice(start..);
            })
        {
            return;
        }

        // {
        //     tracing::span!(tracing::Level::INFO, "before insert {} pos={}", op_id, pos);
        //     debug_log::debug_dbg!(&self);
        // }
        self._insert(pos, content, op_id);
    }

    fn _insert(&mut self, pos: usize, content: RichtextChunk, op_id: IdFull) {
        let result = self.rope.insert(
            pos,
            FugueSpan {
                content,
                id: op_id,
                real_id: if op_id.peer == UNKNOWN_PEER_ID {
                    None
                } else {
                    Some(op_id.id().try_into().unwrap())
                },
                status: Status::default(),
                diff_status: None,
                origin_left: None,
                origin_right: None,
            },
            |id| self.id_to_cursor.get_insert(id).unwrap(),
        );
        self.id_to_cursor.insert(
            op_id.id(),
            id_to_cursor::Cursor::new_insert(result.leaf, content.len()),
        );

        self.update_insert_by_split(&result.splitted.arr);

        let end_id = op_id.inc(content.len() as Counter);
        self.current_vv.extend_to_include_end_id(end_id.id());
        self.applied_vv.extend_to_include_end_id(end_id.id());
    }

    fn update_insert_by_split(&mut self, split: &[LeafIndex]) {
        for &new_leaf_idx in split {
            let leaf = self.rope.tree().get_elem(new_leaf_idx).unwrap();

            self.id_to_cursor
                .update_insert(leaf.id_span(), new_leaf_idx)
        }
    }

    /// Delete the element from pos..pos+len
    ///
    /// If `reverse` is true, the deletion happens from the end of the range to the start.
    /// So the first op is the one that deletes element at `pos+len-1`, the last op
    /// is the one that deletes element at `pos`.
    ///
    /// - op_id: the first op that perform the deletion
    /// - target_start_id: in the target deleted span, it's the first id of the span
    /// - pos: the start pos of the deletion in the target span
    /// - len: the length of the deletion span
    /// - reverse: if true, the kth op delete the last kth element of the span
    pub(crate) fn delete(
        &mut self,
        mut op_id: ID,
        mut target_start_id: ID,
        pos: usize,
        mut len: usize,
        reverse: bool,
    ) {
        if let ControlFlow::Break(_) = self.skip_applied(op_id, len, |applied_counter_end: i32| {
            // the op is partially included, need to slice the op
            let start = (applied_counter_end - op_id.counter) as usize;
            op_id.counter = applied_counter_end;
            if !reverse {
                target_start_id = target_start_id.inc(start as i32);
            }
            // Okay, this looks pretty weird, but it's correct.
            // If it's reverse, we don't need to change the target_start_id, because target_start_id always pointing towards the
            // leftmost element of the span. After applying the initial part of the deletion, which starts from the right side,
            // the target_start_id will be still pointing towards the same leftmost element, thus no need to change.
            len -= start;
            // If reverse, don't need to change the pos, because it's deleting backwards.
            // If not reverse, we don't need to change the pos either, because the `start` chars after it are already deleted
        }) {
            return;
        }

        // tracing::info!("after forwarding pos={} len={}", pos, len);

        self._delete(target_start_id, pos, len, reverse, op_id);
    }

    fn _delete(&mut self, target_start_id: ID, pos: usize, len: usize, reverse: bool, op_id: ID) {
        let mut ans = Vec::new();
        let split = self
            .rope
            .delete(target_start_id, pos, len, reverse, &mut |span| {
                let mut id_span = span.id_span();
                if reverse {
                    id_span.reverse();
                }
                ans.push(id_span);
            });

        let mut cur_id = op_id;
        for id_span in ans {
            let len = id_span.atom_len();
            self.id_to_cursor
                .insert(cur_id, id_to_cursor::Cursor::Delete(id_span));
            cur_id = cur_id.inc(len as Counter);
        }

        debug_assert_eq!(cur_id.counter - op_id.counter, len as Counter);
        for s in split {
            self.update_insert_by_split(&s.arr);
        }

        let end_id = op_id.inc(len as Counter);
        self.current_vv.extend_to_include_end_id(end_id);
        self.applied_vv.extend_to_include_end_id(end_id);
    }

    fn skip_applied(
        &mut self,
        op_id: ID,
        len: usize,
        mut f: impl FnMut(Counter),
    ) -> ControlFlow<()> {
        let last_id = op_id.inc(len as Counter - 1);
        let applied_counter_end = self.applied_vv.get(&last_id.peer).copied().unwrap_or(0);
        if applied_counter_end > op_id.counter {
            if !self.current_vv.includes_id(last_id) {
                // PERF: may be slow
                let mut updates = Default::default();
                let cnt_start = self.current_vv.get(&op_id.peer).copied().unwrap_or(0);
                self.forward(
                    IdSpan::new(op_id.peer, cnt_start, op_id.counter + len as Counter),
                    &mut updates,
                );
                self.batch_update(updates, false);
            }

            if applied_counter_end > last_id.counter {
                self.current_vv.extend_to_include_last_id(last_id);
                return ControlFlow::Break(());
            }

            f(applied_counter_end);
        }
        ControlFlow::Continue(())
    }

    /// Internally it's delete at the src and insert at the dst.
    ///
    /// But it needs special behavior for id_to_cursor data structure
    ///
    #[instrument(skip(self))]
    pub(crate) fn move_item(
        &mut self,
        op_id: IdFull,
        deleted_id: ID,
        from_pos: usize,
        to_pos: usize,
    ) {
        if let ControlFlow::Break(_) = self.skip_applied(op_id.id(), 1, |_| unreachable!()) {
            return;
        }

        // We record the **fake** id of the deleted item, and store it in the `id_to_cursor`.
        // This is because when we retreat, we need to know the **fake** id of the deleted item,
        // so that we can look up the insert pos in `id_to_cursor`
        //
        // > `id_to_cursor` only stores the mappings from **fake** insert id to the leaf index.
        // > **Fake** means the id may be a temporary placeholder, created with UNKNOWN_PEER_ID.
        let mut fake_delete_id = None;
        let split = self.rope.delete(deleted_id, from_pos, 1, false, &mut |s| {
            debug_assert_eq!(s.rle_len(), 1);
            fake_delete_id = Some(s.id.id());
        });

        for s in split {
            self.update_insert_by_split(&s.arr);
        }

        let result = self.rope.insert(
            to_pos,
            FugueSpan {
                // we need to use the special move content to avoid
                // its merging with other [FugueSpan], which will make
                // id_to_cursor need to track its split.
                // It would be much harder to implement correctly
                content: RichtextChunk::new_move(),
                id: op_id,
                real_id: if op_id.peer == UNKNOWN_PEER_ID {
                    None
                } else {
                    Some(op_id.id().try_into().unwrap())
                },
                status: Status::default(),
                diff_status: None,
                origin_left: None,
                origin_right: None,
            },
            |id| self.id_to_cursor.get_insert(id).unwrap(),
        );
        self.update_insert_by_split(&result.splitted.arr);

        self.id_to_cursor.insert(
            op_id.id(),
            id_to_cursor::Cursor::new_move(result.leaf, fake_delete_id.unwrap()),
        );

        let end_id = op_id.inc(1);
        self.current_vv.extend_to_include_end_id(end_id.id());
        self.applied_vv.extend_to_include_end_id(end_id.id());
    }

    #[inline]
    pub(crate) fn checkout(&mut self, vv: &VersionVector) {
        self._checkout(vv, false);
    }

    fn _checkout(&mut self, vv: &VersionVector, on_diff_status: bool) {
        // tracing::info!("Checkout to {:?} from {:?}", vv, self.current_vv);
        if on_diff_status {
            self.rope.clear_diff_status();
        }

        let current_vv = std::mem::take(&mut self.current_vv);
        let (retreat, forward) = current_vv.diff_iter(vv);
        let mut updates = Vec::new();
        for span in retreat {
            for c in self.id_to_cursor.iter(span) {
                match c {
                    id_to_cursor::IterCursor::Insert { leaf, id_span } => {
                        updates.push(crdt_rope::LeafUpdate {
                            leaf,
                            id_span,
                            set_future: Some(true),
                            delete_times_diff: 0,
                        })
                    }
                    id_to_cursor::IterCursor::Delete(span) => {
                        for to_del in self.id_to_cursor.iter(span) {
                            match to_del {
                                id_to_cursor::IterCursor::Insert { leaf, id_span } => {
                                    updates.push(crdt_rope::LeafUpdate {
                                        leaf,
                                        id_span,
                                        set_future: None,
                                        delete_times_diff: -1,
                                    })
                                }
                                id_to_cursor::IterCursor::Move {
                                    from_id: _,
                                    to_leaf,
                                    new_op_id,
                                } => updates.push(crdt_rope::LeafUpdate {
                                    leaf: to_leaf,
                                    id_span: new_op_id.to_span(1),
                                    set_future: None,
                                    delete_times_diff: -1,
                                }),
                                _ => unreachable!(),
                            }
                        }
                    }
                    id_to_cursor::IterCursor::Move {
                        from_id: from,
                        to_leaf: to,
                        new_op_id: op_id,
                    } => {
                        let mut visited = false;
                        for to_del in self.id_to_cursor.iter(IdSpan::new(
                            from.peer,
                            from.counter,
                            from.counter + 1,
                        )) {
                            visited = true;

                            match to_del {
                                id_to_cursor::IterCursor::Move {
                                    from_id: _,
                                    to_leaf: to,
                                    new_op_id: op_id,
                                } => updates.push(crdt_rope::LeafUpdate {
                                    leaf: to,
                                    id_span: op_id.to_span(1),
                                    set_future: None,
                                    delete_times_diff: -1,
                                }),
                                // Un delete the from
                                id_to_cursor::IterCursor::Insert { leaf, id_span } => {
                                    debug_assert_eq!(id_span.atom_len(), 1);
                                    debug_assert_eq!(id_span.counter.start, from.counter);
                                    updates.push(crdt_rope::LeafUpdate {
                                        leaf,
                                        id_span,
                                        set_future: None,
                                        delete_times_diff: -1,
                                    })
                                }
                                _ => unreachable!(),
                            }
                        }
                        assert!(visited);
                        // insert the new
                        updates.push(crdt_rope::LeafUpdate {
                            leaf: to,
                            id_span: IdSpan::new(op_id.peer, op_id.counter, op_id.counter + 1),
                            set_future: Some(true),
                            delete_times_diff: 0,
                        });
                    }
                }
            }
        }

        for span in forward {
            self.forward(span, &mut updates);
        }

        if !on_diff_status {
            self.current_vv = vv.clone();
        } else {
            self.current_vv = current_vv;
        }

        self.batch_update(updates, on_diff_status);
    }

    fn batch_update(&mut self, updates: Vec<crdt_rope::LeafUpdate>, on_diff_status: bool) {
        let leaf_indexes = self.rope.update(updates, on_diff_status);
        self.update_insert_by_split(&leaf_indexes);
    }

    fn forward(&mut self, span: loro_common::IdSpan, updates: &mut Vec<crdt_rope::LeafUpdate>) {
        for c in self.id_to_cursor.iter(span) {
            match c {
                id_to_cursor::IterCursor::Insert { leaf, id_span } => {
                    updates.push(crdt_rope::LeafUpdate {
                        leaf,
                        id_span,
                        set_future: Some(false),
                        delete_times_diff: 0,
                    })
                }
                id_to_cursor::IterCursor::Delete(span) => {
                    for to_del in self.id_to_cursor.iter(span) {
                        match to_del {
                            id_to_cursor::IterCursor::Insert { leaf, id_span } => {
                                updates.push(crdt_rope::LeafUpdate {
                                    leaf,
                                    id_span,
                                    set_future: None,
                                    delete_times_diff: 1,
                                })
                            }
                            id_to_cursor::IterCursor::Move {
                                from_id: _,
                                to_leaf,
                                new_op_id,
                            } => updates.push(crdt_rope::LeafUpdate {
                                leaf: to_leaf,
                                id_span: new_op_id.to_span(1),
                                set_future: None,
                                delete_times_diff: 1,
                            }),
                            _ => unreachable!(),
                        }
                    }
                }
                id_to_cursor::IterCursor::Move {
                    from_id: from,
                    to_leaf: to,
                    new_op_id: op_id,
                } => {
                    for to_del in self.id_to_cursor.iter(IdSpan::new(
                        from.peer,
                        from.counter,
                        from.counter + 1,
                    )) {
                        match to_del {
                            id_to_cursor::IterCursor::Move {
                                from_id: _,
                                to_leaf: to,
                                new_op_id: op_id,
                            } => updates.push(crdt_rope::LeafUpdate {
                                leaf: to,
                                id_span: op_id.to_span(1),
                                set_future: None,
                                delete_times_diff: 1,
                            }),
                            id_to_cursor::IterCursor::Insert { leaf, id_span } => {
                                updates.push(crdt_rope::LeafUpdate {
                                    leaf,
                                    id_span,
                                    set_future: None,
                                    delete_times_diff: 1,
                                })
                            }
                            _ => unreachable!(),
                        }
                    }

                    updates.push(crdt_rope::LeafUpdate {
                        leaf: to,
                        id_span: IdSpan::new(op_id.peer, op_id.counter, op_id.counter + 1),
                        set_future: Some(false),
                        delete_times_diff: 0,
                    });
                }
            }
        }
    }

    #[allow(unused)]
    pub(crate) fn check(&self) {
        if !cfg!(debug_assertions) {
            return;
        }

        self.check_vv_correctness();
        self.check_id_to_cursor_insertions_correctness();
    }

    fn check_vv_correctness(&self) {
        if !cfg!(debug_assertions) {
            return;
        }

        for span in self.rope.tree().iter() {
            if span.id.peer == UNKNOWN_PEER_ID {
                continue;
            }

            let id_span = span.id_span();
            assert!(self.all_vv().includes_id(id_span.id_last()));
            if span.status.future {
                assert!(!self.current_vv.includes_id(id_span.id_start()));
            } else {
                assert!(self.current_vv.includes_id(id_span.id_last()));
            }
        }
    }

    // It can only check the correctness of insertions in id_to_cursor.
    // The deletions are not checked.
    fn check_id_to_cursor_insertions_correctness(&self) {
        if !cfg!(debug_assertions) {
            return;
        }

        for rope_elem in self.rope.tree().iter() {
            let id_span = rope_elem.id_span();
            let leaf_from_start = self.id_to_cursor.get_insert(id_span.id_start()).unwrap();
            let leaf_from_last = self.id_to_cursor.get_insert(id_span.id_last()).unwrap();
            assert_eq!(leaf_from_start, leaf_from_last);
            let elem_from_id_to_cursor_map = self.rope.tree().get_elem(leaf_from_last).unwrap();
            assert_eq!(rope_elem, elem_from_id_to_cursor_map);
        }

        for content in self.id_to_cursor.iter_all() {
            match content {
                id_to_cursor::IterCursor::Insert { leaf, id_span } => {
                    let leaf = self.rope.tree().get_elem(leaf).unwrap();
                    let span = leaf.id_span();
                    span.contains(id_span.id_start());
                    span.contains(id_span.id_last());
                }
                id_to_cursor::IterCursor::Delete(_) => {}
                id_to_cursor::IterCursor::Move { .. } => {}
            }
        }
    }

    pub(crate) fn get_target_id_latest_index_at_new_version(
        &self,
        id: ID,
    ) -> Option<AbsolutePosition> {
        // TODO: PERF this can be sped up from O(n) to O(log(n)) but I'm not sure if it's worth it
        let mut index = 0;
        for span in self.rope.tree.iter() {
            let is_activated = span.is_activated_in_diff();
            let span_id = span.real_id();
            let id_span = span_id.to_span(span.rle_len());
            if id_span.contains(id) {
                if is_activated {
                    index += (id.counter - id_span.counter.start) as usize;
                }

                return Some(AbsolutePosition {
                    pos: index,
                    side: if is_activated {
                        crate::cursor::Side::Middle
                    } else {
                        crate::cursor::Side::Left
                    },
                });
            }

            if is_activated {
                index += span.rle_len();
            }
        }

        None
    }

    // #[tracing::instrument(skip(self), level = "info")]
    pub(crate) fn diff(
        &mut self,
        from: &VersionVector,
        to: &VersionVector,
    ) -> impl Iterator<Item = CrdtRopeDelta> + '_ {
        // tracing::info!("Init: {:#?}, ", &self);
        self._checkout(from, false);
        self._checkout(to, true);
        // self.id_to_cursor.diagnose();
        // tracing::trace!("Trace::diff {:#?}, ", &self);

        self.rope.get_diff()
    }
}

#[cfg(test)]
mod test {
    use crate::{container::richtext::RichtextChunk, vv};
    use generic_btree::rle::HasLength;

    use super::*;

    #[test]
    fn test_len() {
        let mut t = Tracker::new();
        t.insert(IdFull::new(1, 0, 0), 0, RichtextChunk::new_text(0..2));
        assert_eq!(t.rope.len(), 2);
        t.checkout(&Default::default());
        assert_eq!(t.rope.len(), 0);
        t.insert(IdFull::new(2, 0, 0), 0, RichtextChunk::new_text(2..4));
        let v = vv!(1 => 2, 2 => 2);
        t.checkout(&v);
        assert_eq!(&t.applied_vv, &v);
        assert_eq!(t.rope.len(), 4);
    }

    #[test]
    fn test_retreat_and_forward_delete() {
        let mut t = Tracker::new();
        t.insert(IdFull::new(1, 0, 0), 0, RichtextChunk::new_text(0..10));
        t.delete(ID::new(2, 0), ID::NONE_ID, 0, 10, true);
        t.checkout(&vv!(1 => 10, 2=>5));
        assert_eq!(t.rope.len(), 5);
        t.checkout(&vv!(1 => 10, 2=>0));
        assert_eq!(t.rope.len(), 10);
        t.checkout(&vv!(1 => 10, 2=>10));
        assert_eq!(t.rope.len(), 0);
        t.checkout(&vv!(1 => 10, 2=>0));
        assert_eq!(t.rope.len(), 10);
    }

    #[test]
    fn test_checkout_in_doc_with_del_span() {
        let mut t = Tracker::new();
        t.insert(IdFull::new(1, 0, 0), 0, RichtextChunk::new_text(0..10));
        t.delete(ID::new(2, 0), ID::NONE_ID, 0, 10, false);
        t.checkout(&vv!(1 => 10, 2=>4));
        let v: Vec<FugueSpan> = t.rope.tree().iter().copied().collect();
        assert_eq!(v.len(), 2);
        assert!(!v[0].is_activated());
        assert_eq!(v[0].rle_len(), 4);
        assert!(v[1].is_activated());
        assert_eq!(v[1].rle_len(), 6);
    }
}

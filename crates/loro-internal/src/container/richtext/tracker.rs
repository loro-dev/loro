use generic_btree::{rle::Sliceable, LeafIndex};
use loro_common::{Counter, HasId, HasIdSpan, IdSpan, PeerID, ID};
use rle::HasLength;

use crate::VersionVector;

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

const UNKNOWN_PEER_ID: PeerID = u64::MAX;
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
            id: ID::new(UNKNOWN_PEER_ID, 0),
            status: Status::default(),
            diff_status: None,
            origin_left: None,
            origin_right: None,
        });
        this.id_to_cursor.push(
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

    pub(crate) fn insert(&mut self, mut op_id: ID, mut pos: usize, mut content: RichtextChunk) {
        // debug_log::debug_dbg!(&op_id, pos, content);
        // debug_log::debug_dbg!(&self);
        let last_id = op_id.inc(content.len() as Counter - 1);
        let applied_counter_end = self.applied_vv.get(&last_id.peer).copied().unwrap_or(0);
        if applied_counter_end > op_id.counter {
            if !self.current_vv.includes_id(last_id) {
                // PERF: may be slow
                let mut updates = Default::default();
                let cnt_start = self.current_vv.get(&op_id.peer).copied().unwrap_or(0);
                self.forward(
                    IdSpan::new(
                        op_id.peer,
                        cnt_start,
                        op_id.counter + content.len() as Counter,
                    ),
                    &mut updates,
                );
                self.batch_update(updates, false);
            }

            if applied_counter_end > last_id.counter {
                // the op is included in the applied vv
                self.current_vv.extend_to_include_last_id(last_id);
                // debug_log::debug_log!("Ops are already included {:#?}", &self);
                return;
            }

            // the op is partially included, need to slice the content
            let start = (applied_counter_end - op_id.counter) as usize;
            op_id.counter = applied_counter_end;
            pos += start;
            content = content.slice(start..);
        }

        // debug_log::group!("before insert {} pos={}", op_id, pos);
        // debug_log::debug_dbg!(&self);
        let result = self.rope.insert(
            pos,
            FugueSpan {
                content,
                id: op_id,
                status: Status::default(),
                diff_status: None,
                origin_left: None,
                origin_right: None,
            },
            |id| self.id_to_cursor.get_insert(id).unwrap(),
        );
        self.id_to_cursor.push(
            op_id,
            id_to_cursor::Cursor::new_insert(result.leaf, content.len()),
        );

        self.update_insert_by_split(&result.splitted.arr);

        let end_id = op_id.inc(content.len() as Counter);
        self.current_vv.extend_to_include_end_id(end_id);
        self.applied_vv.extend_to_include_end_id(end_id);
        // debug_log::debug_dbg!(&self);
        //
    }

    fn update_insert_by_split(&mut self, split: &[LeafIndex]) {
        for &new_leaf_idx in split {
            let leaf = self.rope.tree().get_elem(new_leaf_idx).unwrap();
            // debug_log::debug_dbg!(&leaf.id_span(), new_leaf_idx);
            self.id_to_cursor
                .update_insert(leaf.id_span(), new_leaf_idx)
        }
    }

    /// Delete the element from pos..pos+len
    ///
    /// If `reverse` is true, the deletion happens from the end of the range to the start.
    /// Then the first op_id is the one that deletes char at `pos+len-1`, the last op
    /// is the one that deletes char at `pos`.
    pub(crate) fn delete(&mut self, mut op_id: ID, pos: usize, mut len: usize, reverse: bool) {
        // debug_log::group!("Tracker Delete");
        // debug_log::debug_dbg!(&op_id, pos, len, reverse);
        // debug_log::debug_dbg!(&self);
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
                debug_log::debug_dbg!(&self);
                return;
            }

            // the op is partially included, need to slice the op
            let start = (applied_counter_end - op_id.counter) as usize;
            op_id.counter = applied_counter_end;
            len -= start;
            // If reverse, don't need to change the pos, because it's deleting backwards.
            // If not reverse, we don't need to change the pos either, because the `start` chars after it are already deleted
        }
        // debug_log::debug_dbg!(&op_id, pos, len, reverse);

        // debug_log::debug_log!("after forwarding pos={} len={}", pos, len);
        // debug_log::debug_dbg!(&self);
        let mut ans = Vec::new();
        let split = self.rope.delete(pos, len, |span| {
            let mut id_span = span.id_span();
            if reverse {
                id_span.reverse();
            }
            ans.push(id_span);
        });

        if reverse {
            ans.reverse();
        }

        let mut cur_id = op_id;
        for id_span in ans {
            let len = id_span.atom_len();
            self.id_to_cursor
                .push(cur_id, id_to_cursor::Cursor::Delete(id_span));
            cur_id = cur_id.inc(len as Counter);
        }

        debug_assert_eq!(cur_id.counter - op_id.counter, len as Counter);
        self.update_insert_by_split(&split.arr);

        let end_id = op_id.inc(len as Counter);
        self.current_vv.extend_to_include_end_id(end_id);
        self.applied_vv.extend_to_include_end_id(end_id);
        // debug_log::debug_dbg!(&self);
    }

    #[inline]
    pub(crate) fn checkout(&mut self, vv: &VersionVector) {
        self._checkout(vv, false);
    }

    fn _checkout(&mut self, vv: &VersionVector, on_diff_status: bool) {
        // debug_log::debug_log!("Checkout to {:?} from {:?}", vv, self.current_vv);
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
                                id_to_cursor::IterCursor::Delete(_) => unreachable!(),
                            }
                        }
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
                            id_to_cursor::IterCursor::Delete(_) => unreachable!(),
                        }
                    }
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
            }
        }
    }

    pub(crate) fn diff(
        &mut self,
        from: &VersionVector,
        to: &VersionVector,
    ) -> impl Iterator<Item = CrdtRopeDelta> + '_ {
        // debug_log::group!("From {:?} To {:?}", from, to);
        // debug_log::debug_log!("Init: {:#?}, ", &self);
        self._checkout(from, false);
        self._checkout(to, true);
        // self.id_to_cursor.diagnose();
        // debug_log::debug_dbg!(&self);
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
        t.insert(ID::new(1, 0), 0, RichtextChunk::new_text(0..2));
        assert_eq!(t.rope.len(), 2);
        t.checkout(&Default::default());
        assert_eq!(t.rope.len(), 0);
        t.insert(ID::new(2, 0), 0, RichtextChunk::new_text(2..4));
        let v = vv!(1 => 2, 2 => 2);
        t.checkout(&v);
        assert_eq!(&t.applied_vv, &v);
        assert_eq!(t.rope.len(), 4);
    }

    #[test]
    fn test_retreat_and_forward_delete() {
        let mut t = Tracker::new();
        t.insert(ID::new(1, 0), 0, RichtextChunk::new_text(0..10));
        t.delete(ID::new(2, 0), 0, 10, true);
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
        t.insert(ID::new(1, 0), 0, RichtextChunk::new_text(0..10));
        t.delete(ID::new(2, 0), 0, 10, false);
        t.checkout(&vv!(1 => 10, 2=>4));
        let v: Vec<FugueSpan> = t.rope.tree().iter().copied().collect();
        assert_eq!(v.len(), 2);
        assert!(!v[0].is_activated());
        assert_eq!(v[0].rle_len(), 4);
        assert!(v[1].is_activated());
        assert_eq!(v[1].rle_len(), 6);
    }

    #[test]
    fn test_checkout_in_doc_with_reversed_del_span() {
        let mut t = Tracker::new();
        t.insert(ID::new(1, 0), 0, RichtextChunk::new_text(0..10));
        t.delete(ID::new(2, 0), 0, 10, true);
        t.checkout(&vv!(1 => 10, 2=>4));
        let v: Vec<FugueSpan> = t.rope.tree().iter().copied().collect();
        assert_eq!(v.len(), 2);
        assert!(v[0].is_activated());
        assert_eq!(v[0].rle_len(), 6);
        assert!(!v[1].is_activated());
        assert_eq!(v[1].rle_len(), 4);
    }
}

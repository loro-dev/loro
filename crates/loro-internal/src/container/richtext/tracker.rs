use generic_btree::LeafIndex;
use loro_common::{Counter, PeerID, ID};

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

impl Tracker {
    pub fn new_with_unknown() -> Self {
        let mut this = Self {
            rope: CrdtRope::new(),
            id_to_cursor: IdToCursor::default(),
            applied_vv: Default::default(),
            current_vv: Default::default(),
        };

        this.insert(
            ID::new(PeerID::MAX, 0),
            0,
            RichtextChunk::new_unknown(u32::MAX / 2),
        );
        this
    }

    pub fn new() -> Self {
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

    pub(crate) fn insert(&mut self, op_id: ID, pos: usize, content: RichtextChunk) {
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
    }

    fn update_insert_by_split(&mut self, split: &[LeafIndex]) {
        for &new_leaf_idx in split {
            let leaf = self.rope.tree().get_elem(new_leaf_idx).unwrap();
            self.id_to_cursor
                .update_insert(leaf.id_span(), new_leaf_idx)
        }
    }

    pub(crate) fn delete(&mut self, op_id: ID, pos: usize, len: usize) {
        let mut cur_id = op_id;
        let splitted = self.rope.delete(pos, len, |span| {
            self.id_to_cursor
                .push(cur_id, id_to_cursor::Cursor::Delete(span.id_span()));
            cur_id = cur_id.inc(span.content.len() as Counter);
        });

        self.update_insert_by_split(&splitted.arr);

        let end_id = op_id.inc(len as Counter);
        self.current_vv.extend_to_include_end_id(end_id);
        self.applied_vv.extend_to_include_end_id(end_id);
    }

    #[inline]
    pub(crate) fn checkout(&mut self, vv: &VersionVector) {
        self._checkout(vv, false);
    }

    fn _checkout(&mut self, vv: &VersionVector, on_diff_status: bool) {
        if on_diff_status {
            self.rope.clear_diff_status();
        }

        let (retreat, forward) = self.current_vv.diff_iter(vv);
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

        self.current_vv = vv.clone();
        let leaf_indexes = self.rope.update(&updates, on_diff_status);
        self.update_insert_by_split(&leaf_indexes);
    }

    pub(crate) fn diff(
        &mut self,
        from: &VersionVector,
        to: &VersionVector,
    ) -> impl Iterator<Item = CrdtRopeDelta> + '_ {
        self._checkout(from, false);
        self._checkout(to, true);
        self.rope.get_diff()
    }
}

#[cfg(test)]
mod test {
    use crate::{container::richtext::RichtextChunk, vv};

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
        dbg!(&t);
    }

    #[test]
    fn test_retreat_and_forward_delete() {
        let mut t = Tracker::new();
        t.insert(ID::new(1, 0), 0, RichtextChunk::new_text(0..10));
        t.delete(ID::new(2, 0), 0, 10);
        t.checkout(&vv!(1 => 10, 2=>5));
        assert_eq!(t.rope.len(), 5);
        t.checkout(&vv!(1 => 10, 2=>0));
        assert_eq!(t.rope.len(), 10);
        t.checkout(&vv!(1 => 10, 2=>10));
        assert_eq!(t.rope.len(), 0);
        t.checkout(&vv!(1 => 10, 2=>0));
        assert_eq!(t.rope.len(), 10);
    }
}

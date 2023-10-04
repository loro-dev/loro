use generic_btree::LeafIndex;
use loro_common::{Counter, PeerID, ID};

use crate::VersionVector;

use self::{
    crdt_rope::CrdtRope,
    fugue_span::{Content, FugueSpan, Status},
    id_to_cursor::IdToCursor,
};

mod crdt_rope;
mod fugue_span;
mod id_to_cursor;

#[derive(Debug)]
pub(crate) struct Tracker {
    applied_vv: VersionVector,
    current_vv: VersionVector,
    rope: CrdtRope,
    id_to_cursor: IdToCursor,
}

impl Tracker {
    pub fn new() -> Self {
        let mut this = Self {
            rope: CrdtRope::new(),
            id_to_cursor: IdToCursor::default(),
            applied_vv: Default::default(),
            current_vv: Default::default(),
        };

        this.insert(
            ID::new(PeerID::MAX, 0),
            0,
            Content::new_unknown(u32::MAX / 2),
        );
        this
    }

    fn insert(&mut self, op_id: ID, pos: usize, content: Content) {
        let result = self.rope.insert(
            pos,
            FugueSpan {
                content,
                id: op_id,
                status: Status::default(),
                after_status: None,
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
    }

    fn update_insert_by_split(&mut self, split: &[LeafIndex]) {
        for &new_leaf_idx in split {
            let leaf = self.rope.tree().get_elem(new_leaf_idx).unwrap();
            self.id_to_cursor
                .update_insert(leaf.id_span(), new_leaf_idx)
        }
    }

    fn delete(&mut self, op_id: ID, pos: usize, len: usize) {
        let mut cur_id = op_id;
        let splitted = self.rope.delete(pos, len, |span| {
            self.id_to_cursor
                .push(cur_id, id_to_cursor::Cursor::Delete(span.id_span()));
            cur_id = cur_id.inc(span.content.len() as Counter);
        });

        self.update_insert_by_split(&splitted.arr)
    }

    fn checkout(&mut self, vv: &VersionVector) {
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

        let leaf_indexes = self.rope.update(&updates);
        self.update_insert_by_split(&leaf_indexes);
    }
}

use generic_btree::SplittedLeaves;
use loro_common::{Counter, IdSpan, PeerID, ID};

use self::{
    crdt_rope::CrdtRope,
    fugue_span::{Content, FugueSpan},
    id_to_cursor::IdToCursor,
};

mod crdt_rope;
mod fugue_span;
mod id_to_cursor;

#[derive(Debug)]
pub(crate) struct Tracker {
    rope: CrdtRope,
    id_to_cursor: IdToCursor,
}

impl Tracker {
    pub fn new() -> Self {
        let mut this = Self {
            rope: CrdtRope::new(),
            id_to_cursor: IdToCursor::default(),
        };

        this.insert(
            0,
            fugue_span::FugueSpan::new(ID::new(PeerID::MAX, 0), Content::new_unknown(u32::MAX / 2)),
        );
        this
    }

    fn insert(&mut self, pos: usize, span: FugueSpan) {
        let result = self
            .rope
            .insert(pos, span, |id| self.id_to_cursor.get_insert(id).unwrap());
        self.id_to_cursor.push(
            span.id,
            id_to_cursor::Cursor::new_insert(result.leaf, span.content.len()),
        );

        self.update_insert_by_split(result.splitted);
    }

    fn update_insert_by_split(&mut self, split: SplittedLeaves) {
        for new_leaf_idx in split.arr {
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

        self.update_insert_by_split(splitted)
    }
}

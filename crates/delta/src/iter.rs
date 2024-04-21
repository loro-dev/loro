use generic_btree::{rle::HasLength, Cursor, LeafIndex};

use crate::{
    delta_trait::{DeltaAttr, DeltaValue},
    DeltaItem, DeltaRope,
};

pub struct Iter<'a, V: DeltaValue, Attr: DeltaAttr> {
    delta: &'a DeltaRope<V, Attr>,
    cursor: Option<LeafIndex>,
    offset: usize,
}

pub struct SlicedDeltaItem<'a, V: DeltaValue, Attr: DeltaAttr> {
    pub item: &'a DeltaItem<V, Attr>,
    pub start_offset: usize,
}

impl<'a, V: DeltaValue, Attr: DeltaAttr> SlicedDeltaItem<'a, V, Attr> {
    pub fn len(&self) -> usize {
        match self.item {
            DeltaItem::Delete(len) => *len - self.start_offset,
            DeltaItem::Retain { len, .. } => *len - self.start_offset,
            DeltaItem::Insert { value, .. } => value.rle_len() - self.start_offset,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn to_delta(&self) -> DeltaItem<V, Attr> {
        match self.item {
            DeltaItem::Delete(_) => DeltaItem::Delete(self.len()),
            DeltaItem::Retain { attr, .. } => DeltaItem::Retain {
                len: self.len(),
                attr: attr.clone(),
            },
            DeltaItem::Insert { value, attr } => DeltaItem::Insert {
                value: value.slice(self.start_offset..),
                attr: attr.clone(),
            },
        }
    }
}

impl<'a, V: DeltaValue, Attr: DeltaAttr> Iter<'a, V, Attr> {
    pub fn new(delta: &'a DeltaRope<V, Attr>) -> Self {
        Self {
            delta,
            cursor: delta.tree.first_leaf(),
            offset: 0,
        }
    }

    pub fn peek(&self) -> Option<SlicedDeltaItem<V, Attr>> {
        self.cursor.and_then(|cursor| {
            self.delta.tree.get_elem(cursor).map(|x| SlicedDeltaItem {
                item: x,
                start_offset: self.offset,
            })
        })
    }

    pub fn next_with(&mut self, len: usize) {
        self.offset += len;
        while self.offset > 0 && self.cursor.is_some() {
            let cursor = self.cursor.unwrap();
            let elem = self.delta.tree.get_elem(cursor).unwrap();
            let elem_len = elem.delta_len();
            if self.offset < elem_len {
                break;
            }
            self.offset -= elem_len;
            self.cursor = self
                .delta
                .tree
                .next_elem(Cursor {
                    leaf: cursor,
                    offset: 0,
                })
                .map(|x| x.leaf);
        }
    }
}

impl<'a, V: DeltaValue, Attr: DeltaAttr> Iterator for Iter<'a, V, Attr> {
    type Item = (&'a DeltaItem<V, Attr>, usize);

    fn next(&mut self) -> Option<Self::Item> {
        let old_cursor = self.cursor?;
        let old_offset = self.offset;
        self.offset = 0;
        self.cursor = self
            .delta
            .tree
            .next_elem(Cursor {
                leaf: self.cursor.unwrap(),
                offset: 0,
            })
            .map(|x| x.leaf);
        self.delta
            .tree
            .get_elem(old_cursor)
            .map(|x| (x, old_offset))
    }
}

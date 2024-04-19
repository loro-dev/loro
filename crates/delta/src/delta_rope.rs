use std::{fmt::Debug, ops::Range};

use generic_btree::LengthFinder;

use crate::{
    delta_trait::{DeltaAttr, DeltaValue},
    DeltaItem, DeltaRope,
};

pub(crate) mod rle_tree;

impl<V: DeltaValue, Attr: DeltaAttr> DeltaRope<V, Attr> {
    pub fn new() -> Self {
        Self {
            tree: Default::default(),
        }
    }

    pub fn compose(&mut self, other: &Self) {
        let mut index = 0;
        for item in other.iter() {
            match item {
                item @ DeltaItem::Insert { value, .. } => {
                    self.insert_value(index, &[item.clone()]);
                    index += value.rle_len();
                }
                DeltaItem::Retain { len, attr } => {
                    self.update_range(index..index + len, attr);
                    index += len;
                }
                DeltaItem::Delete(len) => self.delete_range(index..index + len),
            }
        }
    }

    pub fn first(&self) -> Option<&DeltaItem<V, Attr>> {
        let leaf = self.tree.first_leaf()?;
        self.tree.get_elem(leaf)
    }

    pub fn iter(&self) -> impl Iterator<Item = &DeltaItem<V, Attr>> {
        self.tree.iter()
    }

    pub fn len(&self) -> usize {
        *self.tree.root_cache() as usize
    }

    pub fn is_empty(&self) -> bool {
        self.tree.is_empty()
    }

    pub fn insert(&mut self, v: V, attr: Attr) -> &mut Self {
        let Some(leaf) = self.tree.last_leaf() else {
            self.tree.push(DeltaItem::Insert { value: v, attr });
            return self;
        };
        let mut inserted = false;
        self.tree.update_leaf(leaf, |item| {
            if let DeltaItem::Insert { value, attr: a } = item {
                if value.can_merge(&v) && a == &attr {
                    value.merge_right(&v);
                    inserted = true;
                    return (true, None, None);
                }
            }
            (false, None, None)
        });

        if !inserted {
            self.tree.push(DeltaItem::Insert { value: v, attr });
        }

        self
    }

    pub fn retain(&mut self, retain: usize, attr: Attr) -> &mut Self {
        let leaf = self.tree.last_leaf().unwrap();
        let mut inserted = false;
        self.tree.update_leaf(leaf, |item| {
            if let DeltaItem::Retain { len, attr: a } = item {
                if a == &attr {
                    *len += retain;
                    inserted = true;
                    return (true, None, None);
                }
            }
            (false, None, None)
        });

        if !inserted {
            self.tree.push(DeltaItem::Retain { len: retain, attr });
        }

        self
    }

    pub fn delete(&mut self, len: usize) -> &mut Self {
        let leaf = self.tree.last_leaf().unwrap();
        let mut inserted = false;
        self.tree.update_leaf(leaf, |item| {
            if let DeltaItem::Delete(l) = item {
                *l += len;
                inserted = true;
                return (true, None, None);
            }
            (false, None, None)
        });

        if !inserted {
            self.tree.push(DeltaItem::Delete(len));
        }

        self
    }
}

impl<V: DeltaValue + Debug, Attr: DeltaAttr + Debug> Default for DeltaRope<V, Attr> {
    fn default() -> Self {
        Self::new()
    }
}

impl<V: DeltaValue, Attr: DeltaAttr> DeltaRope<V, Attr> {
    pub(crate) fn insert_value(&mut self, pos: usize, values: &[DeltaItem<V, Attr>]) {
        if self.is_empty() {
            for value in values {
                self.tree.push(value.clone());
            }
            return;
        }

        let pos = self.tree.query::<LengthFinder>(&pos).unwrap();
        // This would crash if values's number is large
        self.tree
            .insert_many_by_cursor(Some(pos.cursor), values.to_vec());
    }

    fn delete_range(&mut self, range: Range<usize>) {
        if range.start == range.end || self.is_empty() {
            return;
        }

        let from = self.tree.query::<LengthFinder>(&range.start).unwrap();
        let to = self.tree.query::<LengthFinder>(&range.end).unwrap();
        self.tree.drain(from..to);
    }

    fn update_range(&mut self, range: Range<usize>, attr: &Attr) {
        if range.start == range.end || self.is_empty() {
            return;
        }

        let from = self.tree.query::<LengthFinder>(&range.start).unwrap();
        let to = self.tree.query::<LengthFinder>(&range.end).unwrap();
        self.tree.update(from.cursor..to.cursor, &mut |item| {
            match item {
                DeltaItem::Delete(_) => {}
                DeltaItem::Retain { attr: a, .. } => {
                    a.merge(attr);
                }
                DeltaItem::Insert { attr: a, .. } => a.merge(attr),
            }

            None
        });
    }
}

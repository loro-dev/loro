use std::{fmt::Debug, ops::Range};

use generic_btree::{
    rle::{Mergeable, Sliceable},
    Cursor, LengthFinder,
};

use crate::{
    delta_trait::{DeltaAttr, DeltaValue},
    DeltaItem, DeltaRope, DeltaRopeBuilder,
};

use self::rle_tree::Len;

use super::iter::Iter;

pub(crate) mod rle_tree;

impl<V: DeltaValue, Attr: DeltaAttr> DeltaRope<V, Attr> {
    pub fn new() -> Self {
        Self {
            tree: Default::default(),
        }
    }

    pub fn compose(&mut self, other: &Self) {
        // TODO: Need to implement a slow mode that is guaranteed to be correct, then we can fuzz on it
        if self.is_empty() {
            *self = other.clone();
            return;
        }

        let mut index = 0;

        let mut push_rest = false;
        for item in other.iter() {
            if index >= self.len() {
                self.push_retain(index - self.len(), Attr::default());
                push_rest = true;
            }

            if push_rest {
                self.push(item.clone());
                continue;
            }

            match item {
                item @ DeltaItem::Insert { value, .. } => {
                    self.insert_values(index, [item.clone()]);
                    index += value.rle_len();
                }
                DeltaItem::Retain { len, attr } => {
                    if self.len() < index + len {
                        self.push_retain(index + len - self.len(), Default::default());
                    }
                    if !attr.attr_is_empty() {
                        self.update_range(index..index + len, attr);
                    }
                    index += len;
                }
                DeltaItem::Delete(len) => {
                    if *len == 0 {
                        continue;
                    }

                    assert!(index < self.len());
                    let range = index..(index + len).min(self.len());
                    let from = self.tree.query::<LengthFinder>(&range.start).unwrap();
                    let to = self.tree.query::<LengthFinder>(&range.end).unwrap();
                    if from.cursor.leaf == to.cursor.leaf {
                        self.tree.update_leaf(from.cursor.leaf, |item| match item {
                            DeltaItem::Delete(l) => {
                                assert!(!to.found);
                                *l += len;
                                (true, None, None)
                            }
                            DeltaItem::Retain {
                                len: retain_len,
                                attr,
                            } => {
                                let start = from.cursor.offset;
                                let end = to.cursor.offset;
                                let (l, r) = match (start == 0, end == *retain_len) {
                                    (true, true) => {
                                        *item = DeltaItem::Delete(*retain_len);
                                        (None, None)
                                    }
                                    (true, false) => {
                                        let right = item.slice(end..);
                                        *item = DeltaItem::Delete(end);
                                        (Some(right), None)
                                    }
                                    (false, true) => {
                                        *retain_len -= *len;
                                        (Some(DeltaItem::Delete(*len)), None)
                                    }
                                    (false, false) => {
                                        let right = DeltaItem::Retain {
                                            len: *retain_len - end,
                                            attr: attr.clone(),
                                        };
                                        *retain_len = start;
                                        (Some(DeltaItem::Delete(*len)), Some(right))
                                    }
                                };

                                (true, l, r)
                            }
                            DeltaItem::Insert { value, attr } => {
                                let start = from.cursor.offset;
                                let end = to.cursor.offset;
                                let new = match (start == 0, end == value.rle_len()) {
                                    (true, true) => {
                                        *item = DeltaItem::Delete(0);
                                        None
                                    }
                                    (true, false) => {
                                        value.slice_(end..);
                                        None
                                    }
                                    (false, true) => {
                                        value.slice_(..start);
                                        None
                                    }
                                    (false, false) => {
                                        let right = value.slice(end..);
                                        value.slice_(..start);
                                        let right = DeltaItem::Insert {
                                            value: right,
                                            attr: attr.clone(),
                                        };
                                        if item.can_merge(&right) {
                                            item.merge_right(&right);
                                            None
                                        } else {
                                            Some(right)
                                        }
                                    }
                                };

                                (true, new, None)
                            }
                        });
                    } else {
                        let mut left_len = *len;
                        self.tree.update(from.cursor..to.cursor, &mut |item| {
                            if left_len == 0 {
                                return None;
                            }

                            match item {
                                DeltaItem::Delete(_) => None,
                                DeltaItem::Retain { len, .. } => {
                                    assert!(*len <= left_len);
                                    left_len -= *len;
                                    let diff = -(*len as isize);
                                    *item = DeltaItem::Delete(*len);
                                    Some(Len {
                                        new_len: diff,
                                        old_len: diff,
                                    })
                                }
                                DeltaItem::Insert { value, .. } => {
                                    if left_len > value.rle_len() {
                                        let diff = value.rle_len() as isize;
                                        left_len -= value.rle_len();
                                        *item = DeltaItem::Delete(0);
                                        Some(Len {
                                            new_len: -diff,
                                            old_len: 0,
                                        })
                                    } else {
                                        let diff = left_len as isize;
                                        value.slice_(left_len..);
                                        left_len = 0;
                                        Some(Len {
                                            new_len: -diff,
                                            old_len: 0,
                                        })
                                    }
                                }
                            }
                        });

                        if left_len > 0 {
                            self.insert_values(index, [DeltaItem::Delete(left_len)]);
                        }
                    }
                }
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

    /// Returns the length of the delta rope (insertions + retains).
    pub fn len(&self) -> usize {
        self.tree.root_cache().new_len as usize
    }

    /// Returns the length of the delta rope (deletions + retains).
    pub fn old_len(&self) -> usize {
        self.tree.root_cache().old_len as usize
    }

    pub fn is_empty(&self) -> bool {
        self.tree.is_empty()
    }

    pub fn push_insert(&mut self, v: V, attr: Attr) -> &mut Self {
        if v.rle_len() == 0 {
            return self;
        }

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

    pub fn push_retain(&mut self, retain: usize, attr: Attr) -> &mut Self {
        if retain == 0 {
            return self;
        }

        let Some(leaf) = self.tree.last_leaf() else {
            self.tree.push(DeltaItem::Retain { len: retain, attr });
            return self;
        };

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

    pub fn push_delete(&mut self, len: usize) -> &mut Self {
        if len == 0 {
            return self;
        }

        let Some(leaf) = self.tree.last_leaf() else {
            self.tree.push(DeltaItem::Delete(len));
            return self;
        };

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

    pub fn push(&mut self, item: DeltaItem<V, Attr>) -> &mut Self {
        match item {
            DeltaItem::Insert { value, attr } => self.push_insert(value, attr),
            DeltaItem::Retain { len, attr } => self.push_retain(len, attr),
            DeltaItem::Delete(len) => self.push_delete(len),
        }
    }

    /// Returns an iterator that can iterate over the delta rope with a custom length.
    ///
    /// It's more controllable compared to the default iterator.
    ///
    /// - Iterating over the delta rope with a custom length.
    /// - You can peek the next item.
    ///
    /// It's useful to implement algorithms related to Delta
    pub fn iter_with_len(&self) -> Iter<V, Attr> {
        Iter::new(self)
    }

    pub fn chop(&mut self) {
        let mut last_leaf = self.tree.last_leaf();
        while let Some(last_leaf_idx) = last_leaf {
            let elem = self.tree.get_elem(last_leaf_idx).unwrap();
            match elem {
                DeltaItem::Retain { len: _, attr } if attr.attr_is_empty() => {
                    self.tree.remove_leaf(Cursor {
                        leaf: last_leaf_idx,
                        offset: 0,
                    });
                    last_leaf = self.tree.last_leaf();
                }
                _ => return,
            }
        }
    }
}

impl<V: DeltaValue + PartialEq, Attr: DeltaAttr + PartialEq> PartialEq for DeltaRope<V, Attr> {
    fn eq(&self, other: &Self) -> bool {
        if self.len() != other.len() {
            return false;
        }

        let mut a = self.iter_with_len();
        let mut b = other.iter_with_len();
        while let (Some(x), Some(y)) = (a.peek(), b.peek()) {
            let len = x.len().min(y.len());
            match (x.item, y.item) {
                (DeltaItem::Delete(_), DeltaItem::Delete(_)) => {
                    a.next_with(len);
                    b.next_with(len);
                }
                (DeltaItem::Retain { attr, .. }, DeltaItem::Retain { attr: b_attr, .. }) => {
                    if *attr == *b_attr {
                        a.next_with(len);
                        b.next_with(len);
                    } else {
                        return false;
                    }
                }
                (
                    DeltaItem::Insert { value, attr },
                    DeltaItem::Insert {
                        value: b_value,
                        attr: b_attr,
                    },
                ) => {
                    if attr != b_attr {
                        return false;
                    }

                    if value.slice(x.start_offset..x.start_offset + len)
                        != b_value.slice(y.start_offset..y.start_offset + len)
                    {
                        return false;
                    }

                    a.next_with(len);
                    b.next_with(len);
                }
                _ => return false,
            }
        }

        a.peek().is_none() && b.peek().is_none()
    }
}

impl<V: DeltaValue + Debug, Attr: DeltaAttr + Debug> Default for DeltaRope<V, Attr> {
    fn default() -> Self {
        Self::new()
    }
}

impl<V: DeltaValue, Attr: DeltaAttr> DeltaRope<V, Attr> {
    pub(crate) fn insert_values(
        &mut self,
        pos: usize,
        values: impl IntoIterator<Item = DeltaItem<V, Attr>>,
    ) {
        if self.is_empty() {
            for value in values {
                self.tree.push(value.clone());
            }
            return;
        }

        let pos = self.tree.query::<LengthFinder>(&pos).unwrap();
        // This would crash if values's number is large
        self.tree
            .insert_many_by_cursor(Some(pos.cursor), values.into_iter());
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
                    a.compose(attr);
                }
                DeltaItem::Insert { attr: a, .. } => a.compose(attr),
            }

            None
        });
    }
}

impl<V: DeltaValue, Attr: DeltaAttr> DeltaRopeBuilder<V, Attr> {
    pub fn new() -> Self {
        Self { items: vec![] }
    }

    pub fn insert(mut self, v: V, attr: Attr) -> Self {
        if v.rle_len() == 0 {
            return self;
        }

        if let Some(DeltaItem::Insert { value, attr: a }) = self.items.last_mut() {
            if value.can_merge(&v) && a == &attr {
                value.merge_right(&v);
                return self;
            }
        }

        self.items.push(DeltaItem::Insert { value: v, attr });
        self
    }

    pub fn retain(mut self, retain: usize, attr: Attr) -> Self {
        if retain == 0 {
            return self;
        }

        if let Some(DeltaItem::Retain { len, attr: a }) = self.items.last_mut() {
            if *a == attr {
                *len += retain;
                return self;
            }
        }

        self.items.push(DeltaItem::Retain { len: retain, attr });
        self
    }

    pub fn delete(mut self, len: usize) -> Self {
        if len == 0 {
            return self;
        }

        if let Some(DeltaItem::Delete(l)) = self.items.last_mut() {
            *l += len;
            return self;
        }

        self.items.push(DeltaItem::Delete(len));
        self
    }

    pub fn build(self) -> DeltaRope<V, Attr> {
        let mut rope = DeltaRope::new();
        for item in self.items {
            rope.tree.push(item);
        }

        rope
    }
}

impl<V: DeltaValue, Attr: DeltaAttr> Default for DeltaRopeBuilder<V, Attr> {
    fn default() -> Self {
        Self::new()
    }
}

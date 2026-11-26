use std::{fmt::Debug, ops::Range};

use generic_btree::{rle::Sliceable, Cursor, LeafIndex};

use crate::{
    delta_rope::rle_tree::LengthFinder,
    delta_trait::{DeltaAttr, DeltaValue},
    DeltaItem, DeltaRope, DeltaRopeBuilder,
};

use self::rle_tree::Len;

use super::iter::Iter;

mod compose;
pub(crate) mod rle_tree;

impl<V: DeltaValue, Attr: DeltaAttr> DeltaRope<V, Attr> {
    pub fn new() -> Self {
        Self {
            tree: Default::default(),
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
        self.tree.root_cache().data_len as usize
    }

    /// Returns the length of the delta rope (deletions + retains).
    pub fn old_len(&self) -> usize {
        self.tree.root_cache().delta_len as usize
    }

    pub fn is_empty(&self) -> bool {
        self.tree.is_empty()
    }

    pub fn node_len(&self) -> usize {
        self.tree.node_len()
    }

    pub fn push_insert(&mut self, v: V, attr: Attr) -> &mut Self {
        if v.rle_len() == 0 {
            return self;
        }

        let Some(leaf) = self.tree.last_leaf() else {
            self.tree.push(DeltaItem::Replace {
                value: v,
                attr,
                delete: 0,
            });
            return self;
        };
        let mut inserted = false;
        self.tree.update_leaf(leaf, |item| {
            if let DeltaItem::Replace {
                value,
                attr: a,
                delete: _,
            } = item
            {
                if value.can_merge(&v) && a == &attr {
                    value.merge_right(&v);
                    inserted = true;
                    return (true, None, None);
                }
            }
            (false, None, None)
        });

        if !inserted {
            self.tree.push(DeltaItem::Replace {
                value: v,
                attr,
                delete: 0,
            });
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

    pub fn push_replace(&mut self, value: V, attr: Attr, delete: usize) -> &mut Self {
        if value.rle_len() == 0 && delete == 0 {
            return self;
        }

        let Some(leaf) = self.tree.last_leaf() else {
            self.tree.push(DeltaItem::Replace {
                value,
                attr,
                delete,
            });
            return self;
        };

        let mut inserted = false;
        self.tree.update_leaf(leaf, |item| {
            if let DeltaItem::Replace {
                value: v,
                attr: a,
                delete: d,
            } = item
            {
                if a == &attr && v.can_merge(&value) {
                    v.merge_right(&value);
                    *d += delete;
                    inserted = true;
                    return (true, None, None);
                }
            }
            (false, None, None)
        });

        if !inserted {
            self.tree.push(DeltaItem::Replace {
                value,
                attr,
                delete,
            });
        }

        self
    }

    pub fn push_delete(&mut self, len: usize) -> &mut Self {
        if len == 0 {
            return self;
        }

        let Some(leaf) = self.tree.last_leaf() else {
            self.tree.push(DeltaItem::Replace {
                value: Default::default(),
                attr: Default::default(),
                delete: len,
            });
            return self;
        };

        let mut inserted = false;
        self.tree.update_leaf(leaf, |item| {
            if let DeltaItem::Replace {
                value: _,
                attr: _,
                delete,
            } = item
            {
                *delete += len;
                inserted = true;
                return (true, None, None);
            }
            (false, None, None)
        });

        if !inserted {
            self.tree.push(DeltaItem::new_delete(len));
        }

        self
    }

    pub fn push(&mut self, item: DeltaItem<V, Attr>) -> &mut Self {
        match item {
            DeltaItem::Retain { len, attr } => self.push_retain(len, attr),
            DeltaItem::Replace {
                value,
                attr,
                delete,
            } => self.push_replace(value, attr, delete),
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
    pub fn iter_with_len(&self) -> Iter<'_, V, Attr> {
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

    pub fn transform(&self, other: &Self, left_prior: bool) -> Self {
        let mut this_iter = CanonicalIter::new(self);
        let mut other_iter = CanonicalIter::new(other);
        let mut transformed_delta = DeltaRope::new();

        while this_iter.peek().is_some() || other_iter.peek().is_some() {
            match (this_iter.peek(), other_iter.peek()) {
                (Some(CanonicalOp::Insert(value, attr)), other_op)
                    if left_prior || !matches!(other_op, Some(CanonicalOp::Insert(..))) =>
                {
                    transformed_delta.push_insert((*value).clone(), (*attr).clone());
                    this_iter.consume_insert();
                }
                (_, Some(CanonicalOp::Insert(value, _))) => {
                    transformed_delta.push_retain(value.rle_len(), Default::default());
                    other_iter.consume_insert();
                }
                (this_op, other_op) => {
                    // It's now either retains or deletes
                    let this_len = this_op.map_or(usize::MAX, CanonicalOp::non_insert_len);
                    let other_len = other_op.map_or(usize::MAX, CanonicalOp::non_insert_len);
                    let length = this_len.min(other_len);
                    let other_is_delete = matches!(other_op, Some(CanonicalOp::Delete(_)));
                    let this_is_delete = matches!(this_op, Some(CanonicalOp::Delete(_)));
                    let this_retain_attr = match this_op {
                        Some(CanonicalOp::Retain(_, attr)) => Some((*attr).clone()),
                        _ => None,
                    };
                    this_iter.consume_len(length);
                    other_iter.consume_len(length);
                    if other_is_delete {
                        // It makes our deletes or retains redundant
                        continue;
                    } else if this_is_delete {
                        transformed_delta.push_delete(length);
                    } else {
                        transformed_delta.push_retain(length, this_retain_attr.unwrap_or_default());
                        // FIXME: transform the attributes
                    }
                }
            }
        }

        transformed_delta.chop();
        transformed_delta
    }

    /// Transforms operation `self` against another operation `other` in such a way that the
    /// impact of `other` is effectively included in `self`.
    pub fn transform_(&mut self, other: &Self, left_prior: bool) {
        *self = self.transform(other, left_prior);
    }

    /// Transforms the position based on self
    pub fn transform_pos(&self, mut pos: usize, left_prior: bool) -> usize {
        let mut index = 0;
        let mut iter = CanonicalIter::new(self);
        while let Some(op) = iter.peek() {
            match op {
                CanonicalOp::Retain(len, _) => {
                    let len = *len;
                    iter.consume_len(len);
                    index += len;
                    if index > pos || (index == pos && !left_prior) {
                        return pos;
                    }
                }
                CanonicalOp::Insert(value, _) => {
                    if index == pos && !left_prior {
                        return pos;
                    }

                    let insert_len = value.rle_len();
                    pos += insert_len;
                    index += insert_len;
                    iter.consume_insert();
                }
                CanonicalOp::Delete(delete) => {
                    let delete = *delete;
                    iter.consume_len(delete);
                    pos = pos.saturating_sub(delete);
                    if pos < index {
                        pos = index;
                        return pos;
                    }
                }
            }
        }

        pos
    }
}

enum CanonicalOp<'a, V, Attr> {
    Insert(&'a V, &'a Attr),
    Delete(usize),
    Retain(usize, &'a Attr),
}

impl<V, Attr> CanonicalOp<'_, V, Attr> {
    fn non_insert_len(&self) -> usize {
        match self {
            CanonicalOp::Delete(len) | CanonicalOp::Retain(len, _) => *len,
            CanonicalOp::Insert(..) => unreachable!(),
        }
    }
}

/// Iterates a [`DeltaRope`] in canonical Quill Delta order: within each contiguous
/// run of [`DeltaItem::Replace`] items (no [`DeltaItem::Retain`] between them), all
/// inserts are yielded first in their original order, followed by the combined
/// delete length.
///
/// This makes [`DeltaRope::transform`] and [`DeltaRope::transform_pos`] independent
/// of how adjacent inserts and deletes are split or ordered across leaves (for
/// example, `[Delete(1), Insert("s")]` vs `[Replace { value: "s", delete: 1 }]`).
struct CanonicalIter<'a, V: DeltaValue, Attr: DeltaAttr> {
    delta: &'a DeltaRope<V, Attr>,
    cursor: Option<LeafIndex>,
    pending_delete: usize,
    current: Option<CanonicalOp<'a, V, Attr>>,
}

impl<'a, V: DeltaValue, Attr: DeltaAttr> CanonicalIter<'a, V, Attr> {
    fn new(delta: &'a DeltaRope<V, Attr>) -> Self {
        let mut iter = Self {
            delta,
            cursor: delta.tree.first_leaf(),
            pending_delete: 0,
            current: None,
        };
        iter.load_next();
        iter
    }

    fn peek(&self) -> Option<&CanonicalOp<'a, V, Attr>> {
        self.current.as_ref()
    }

    fn consume_insert(&mut self) {
        debug_assert!(matches!(self.current, Some(CanonicalOp::Insert(..))));
        self.load_next();
    }

    fn consume_len(&mut self, len: usize) {
        match &mut self.current {
            Some(CanonicalOp::Delete(d)) => {
                debug_assert!(len <= *d);
                *d -= len;
                if *d == 0 {
                    self.load_next();
                }
            }
            Some(CanonicalOp::Retain(r, _)) => {
                debug_assert!(len <= *r);
                *r -= len;
                if *r == 0 {
                    self.load_next();
                }
            }
            Some(CanonicalOp::Insert(..)) => unreachable!("use consume_insert for inserts"),
            None => {}
        }
    }

    fn load_next(&mut self) {
        self.current = None;
        while let Some(leaf) = self.cursor {
            let elem = self.delta.tree.get_elem(leaf).unwrap();
            match elem {
                DeltaItem::Replace {
                    value,
                    attr,
                    delete,
                } => {
                    self.pending_delete += *delete;
                    self.cursor = self
                        .delta
                        .tree
                        .next_elem(Cursor { leaf, offset: 0 })
                        .map(|x| x.leaf);
                    if value.rle_len() > 0 {
                        self.current = Some(CanonicalOp::Insert(value, attr));
                        return;
                    }
                }
                DeltaItem::Retain { len, attr } => {
                    if *len == 0 {
                        self.cursor = self
                            .delta
                            .tree
                            .next_elem(Cursor { leaf, offset: 0 })
                            .map(|x| x.leaf);
                        continue;
                    }
                    if self.pending_delete > 0 {
                        let del = std::mem::take(&mut self.pending_delete);
                        self.current = Some(CanonicalOp::Delete(del));
                        return;
                    }
                    self.cursor = self
                        .delta
                        .tree
                        .next_elem(Cursor { leaf, offset: 0 })
                        .map(|x| x.leaf);
                    self.current = Some(CanonicalOp::Retain(*len, attr));
                    return;
                }
            }
        }
        if self.pending_delete > 0 {
            let del = std::mem::take(&mut self.pending_delete);
            self.current = Some(CanonicalOp::Delete(del));
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
            let len = x.delta_len().min(y.delta_len());
            match (x, y) {
                (
                    DeltaItem::Replace {
                        value: va,
                        attr: attr_a,
                        delete: d_a,
                    },
                    DeltaItem::Replace {
                        value: vb,
                        attr: attr_b,
                        delete: d_b,
                    },
                ) => {
                    if attr_a != attr_b {
                        return false;
                    }

                    let va_empty = va.rle_len() == 0;
                    let vb_empty = vb.rle_len() == 0;
                    if vb_empty || va_empty {
                        // both deletions
                        let min_del_len = (*d_a).min(*d_b);
                        if min_del_len == 0 {
                            return false;
                        }

                        a.next_with_del(min_del_len).unwrap();
                        b.next_with_del(min_del_len).unwrap();
                    } else {
                        let len = (va.rle_len()).min(vb.rle_len());
                        let va_slice = va.slice(..len);
                        let vb_slice = vb.slice(..len);
                        if va_slice != vb_slice {
                            return false;
                        }

                        a.next_with(len).unwrap();
                        b.next_with(len).unwrap();
                    }
                }
                (DeltaItem::Retain { attr, .. }, DeltaItem::Retain { attr: b_attr, .. })
                    if *attr == *b_attr =>
                {
                    a.next_with(len).unwrap();
                    b.next_with(len).unwrap();
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
    pub fn insert_values(
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

        let mut cursor = self.tree.query::<LengthFinder>(&pos).unwrap().cursor;
        let mut values = values.into_iter();
        let Some(first) = values.next() else {
            return;
        };
        let Some(second) = values.next() else {
            self.tree.insert_by_path(cursor, first);
            return;
        };

        // Published generic-btree 0.10.7 can merge a batch prefix into the
        // wrong leaf. Insert back to front at the returned start cursor so
        // published loro-delta does not rely on the workspace-only fix.
        let mut batch = vec![first, second];
        batch.extend(values);
        for value in batch.into_iter().rev() {
            cursor = self.tree.insert_by_path(cursor, value).0;
        }
    }

    pub fn insert_value(&mut self, pos: usize, value: V, attr: Attr) {
        if self.len() < pos {
            self.push_retain(pos - self.len(), Default::default());
        }

        if pos == self.len() {
            self.tree.push(DeltaItem::Replace {
                value,
                attr,
                delete: 0,
            });
            return;
        }

        let pos = self.tree.query::<LengthFinder>(&pos).unwrap();
        self.tree.insert_by_path(
            pos.cursor,
            DeltaItem::Replace {
                value,
                attr,
                delete: 0,
            },
        );
    }

    fn update_attr_in_range(&mut self, range: Range<usize>, attr: &Attr) {
        if range.start == range.end || self.is_empty() {
            return;
        }

        let from = self.tree.query::<LengthFinder>(&range.start).unwrap();
        let to = self.tree.query::<LengthFinder>(&range.end).unwrap();
        self.tree.update(from.cursor..to.cursor, &mut |item| {
            match item {
                DeltaItem::Retain { attr: a, .. } => {
                    a.compose(attr);
                }
                DeltaItem::Replace { attr: a, .. } => a.compose(attr),
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

        if let Some(DeltaItem::Replace { value, attr: a, .. }) = self.items.last_mut() {
            if value.can_merge(&v) && a == &attr {
                value.merge_right(&v);
                return self;
            }
        }

        self.items.push(DeltaItem::Replace {
            value: v,
            attr,
            delete: 0,
        });
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

        if let Some(DeltaItem::Replace { delete, .. }) = self.items.last_mut() {
            *delete += len;
            return self;
        }

        self.items.push(DeltaItem::new_delete(len));
        self
    }

    pub fn replace(mut self, value: V, attr: Attr, delete: usize) -> Self {
        if delete == 0 && value.rle_len() == 0 {
            return self;
        }

        if let Some(DeltaItem::Replace {
            value: last_value,
            attr: last_attr,
            delete: last_delete,
        }) = self.items.last_mut()
        {
            if last_value.can_merge(&value) && &attr == last_attr {
                last_value.merge_right(&value);
                *last_delete += delete;
                return self;
            }
        }

        self.items.push(DeltaItem::Replace {
            value,
            attr,
            delete,
        });
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

impl<V: DeltaValue, Attr: DeltaAttr> Debug for DeltaRope<V, Attr> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list().entries(self.iter()).finish()
    }
}

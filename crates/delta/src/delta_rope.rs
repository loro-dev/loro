use std::{fmt::Debug, ops::Range};

use generic_btree::{rle::Sliceable, Cursor};
use tracing::trace;

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

    /// Transforms operation `self` against another operation `other` in such a way that the
    /// impact of `other` is effectively included in `self`.
    pub fn transform(&mut self, other: &Self, left_priority: bool) {
        let mut this_iter = self.iter_with_len();
        let mut other_iter = other.iter_with_len();
        let mut transformed_delta = DeltaRope::new();

        while this_iter.peek().is_some() || other_iter.peek().is_some() {
            trace!(
                "this_iter: {:?}, other_iter: {:?}",
                this_iter.peek(),
                other_iter.peek()
            );
            if this_iter.peek_is_insert() && (left_priority || !other_iter.peek_is_insert()) {
                let insert_length;
                match this_iter.peek().unwrap() {
                    DeltaItem::Replace { value, attr, .. } => {
                        insert_length = value.rle_len();
                        transformed_delta.push_insert(value.clone(), attr.clone());
                    }
                    DeltaItem::Retain { .. } => unreachable!(),
                }
                this_iter.next_with(insert_length).unwrap();
            } else if other_iter.peek_is_insert() {
                let insert_length = other_iter.peek_insert_length();
                transformed_delta.push_retain(insert_length, Default::default());
                other_iter.next_with(insert_length).unwrap();
            } else {
                // It's now either retains or deletes
                let length = this_iter.peek_length().min(other_iter.peek_length());
                let this_op_peek = this_iter.peek().cloned();
                let other_op_peek = other_iter.peek().cloned();
                let _ = this_iter.next_with(length);
                let _ = other_iter.next_with(length);
                if other_op_peek.map(|x| x.is_delete()).unwrap_or(false) {
                    // It makes our deletes or retains redundant
                    continue;
                } else if this_op_peek
                    .as_ref()
                    .map(|x| x.is_delete())
                    .unwrap_or(false)
                {
                    transformed_delta.push_delete(length);
                } else {
                    transformed_delta.push_retain(
                        length,
                        this_op_peek
                            .map(|x| x.into_retain().unwrap().1)
                            .unwrap_or_default(),
                    );
                    // FIXME: transform the attributes
                }
            }
        }

        transformed_delta.chop();
        *self = transformed_delta;
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
                (DeltaItem::Retain { attr, .. }, DeltaItem::Retain { attr: b_attr, .. }) => {
                    if *attr == *b_attr {
                        a.next_with(len).unwrap();
                        b.next_with(len).unwrap();
                    } else {
                        return false;
                    }
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

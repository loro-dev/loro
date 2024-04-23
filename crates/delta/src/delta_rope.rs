use std::{fmt::Debug, ops::Range};

use generic_btree::{rle::Sliceable, Cursor};

use crate::{
    delta_rope::rle_tree::LengthFinder,
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
                DeltaItem::Retain { len, attr } => {
                    if self.len() < index + len {
                        self.push_retain(index + len - self.len(), Default::default());
                    }
                    if !attr.attr_is_empty() {
                        self.update_range(index..index + len, attr);
                    }
                    index += len;
                }
                DeltaItem::Replace {
                    value: this_value,
                    attr: this_attr,
                    delete,
                } => {
                    let mut should_insert = this_value.rle_len() > 0;
                    let mut left_del_len = *delete;
                    if *delete > 0 {
                        assert!(index < self.len());
                        let range = index..(index + left_del_len).min(self.len());
                        let from = self.tree.query::<LengthFinder>(&range.start).unwrap();
                        let to = self.tree.query::<LengthFinder>(&range.end).unwrap();
                        if from.cursor.leaf == to.cursor.leaf {
                            should_insert = false;
                            self.tree.update_leaf(from.cursor.leaf, |item| match item {
                                DeltaItem::Retain {
                                    len: retain_len,
                                    attr,
                                } => {
                                    let start = from.cursor.offset;
                                    let end = to.cursor.offset;
                                    let (l, r) = match (start == 0, end >= *retain_len) {
                                        (true, true) => {
                                            *item = DeltaItem::Replace {
                                                delete: left_del_len,
                                                value: this_value.clone(),
                                                attr: this_attr.clone(),
                                            };
                                            (None, None)
                                        }
                                        (true, false) => {
                                            let right = item.slice(end..);
                                            *item = DeltaItem::Replace {
                                                delete: left_del_len,
                                                value: this_value.clone(),
                                                attr: this_attr.clone(),
                                            };
                                            (Some(right), None)
                                        }
                                        (false, true) => {
                                            *retain_len -= *delete;
                                            (
                                                Some(DeltaItem::Replace {
                                                    value: this_value.clone(),
                                                    attr: this_attr.clone(),
                                                    delete: left_del_len,
                                                }),
                                                None,
                                            )
                                        }
                                        (false, false) => {
                                            let right = DeltaItem::Retain {
                                                len: *retain_len - end,
                                                attr: attr.clone(),
                                            };
                                            *retain_len = start;
                                            (
                                                Some(DeltaItem::Replace {
                                                    value: this_value.clone(),
                                                    attr: this_attr.clone(),
                                                    delete: left_del_len,
                                                }),
                                                Some(right),
                                            )
                                        }
                                    };

                                    left_del_len = 0;
                                    (true, l, r)
                                }
                                DeltaItem::Replace {
                                    value,
                                    attr,
                                    delete,
                                } => {
                                    let start = from.cursor.offset;
                                    let end = to.cursor.offset;
                                    let value_len = value.rle_len();
                                    {
                                        left_del_len =
                                            left_del_len.saturating_sub(value_len.min(end) - start);
                                        *delete += left_del_len;
                                        left_del_len = 0;
                                    }

                                    let mut right = value.split(start);
                                    right.slice_(value_len.min(end) - start..);
                                    if this_value.rle_len() > 0 {
                                        if attr != this_attr || !value.can_merge(this_value) {
                                            let right = if right.rle_len() > 0 {
                                                Some(DeltaItem::Replace {
                                                    value: right,
                                                    attr: attr.clone(),
                                                    delete: 0,
                                                })
                                            } else {
                                                None
                                            };

                                            return (
                                                true,
                                                Some(DeltaItem::Replace {
                                                    value: this_value.clone(),
                                                    attr: this_attr.clone(),
                                                    delete: 0,
                                                }),
                                                right,
                                            );
                                        } else {
                                            value.merge_right(this_value);
                                        }
                                    }

                                    if value.can_merge(&right) {
                                        value.merge_right(&right);
                                        (true, None, None)
                                    } else {
                                        let right = if right.rle_len() > 0 {
                                            Some(DeltaItem::Replace {
                                                value: right,
                                                attr: attr.clone(),
                                                delete: 0,
                                            })
                                        } else {
                                            None
                                        };
                                        (true, right, None)
                                    }
                                }
                            });
                        } else {
                            self.tree.update(from.cursor..to.cursor, &mut |item| {
                                if left_del_len == 0 {
                                    return None;
                                }

                                match item {
                                    DeltaItem::Retain { len, .. } => {
                                        assert!(*len <= left_del_len);
                                        left_del_len -= *len;
                                        let diff = -(*len as isize);
                                        *item = DeltaItem::Replace {
                                            delete: *len,
                                            value: Default::default(),
                                            attr: Default::default(),
                                        };
                                        Some(Len {
                                            data_len: diff,
                                            delta_len: 0,
                                        })
                                    }
                                    DeltaItem::Replace { value, attr, .. } => {
                                        if left_del_len >= value.rle_len() {
                                            let diff = value.rle_len() as isize;
                                            left_del_len -= value.rle_len();
                                            *value = Default::default();
                                            *attr = Default::default();
                                            Some(Len {
                                                data_len: -diff,
                                                delta_len: -diff,
                                            })
                                        } else {
                                            unreachable!()
                                        }
                                    }
                                }
                            });
                        }
                    }

                    if left_del_len > 0 || should_insert {
                        self.insert_values(
                            index,
                            [DeltaItem::Replace {
                                value: if should_insert {
                                    this_value.clone()
                                } else {
                                    Default::default()
                                },
                                attr: if should_insert {
                                    this_attr.clone()
                                } else {
                                    Default::default()
                                },
                                delete: left_del_len,
                            }],
                        );
                    }

                    index += this_value.rle_len();
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
                delete,
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
                value,
                attr,
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

    fn update_range(&mut self, range: Range<usize>, attr: &Attr) {
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

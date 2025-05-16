use std::ops::RangeBounds;

use super::*;
use generic_btree::rle::{CanRemove, TryInsert};

impl<V: DeltaValue, Attr> DeltaItem<V, Attr> {
    /// Including the delete length
    pub fn delta_len(&self) -> usize {
        match self {
            DeltaItem::Retain { len, .. } => *len,
            DeltaItem::Replace {
                value,
                attr: _,
                delete,
            } => value.rle_len() + delete,
        }
    }

    /// The real length of the item in the delta, excluding the delete length
    pub fn data_len(&self) -> usize {
        match self {
            DeltaItem::Retain { len, .. } => *len,
            DeltaItem::Replace { value, .. } => value.rle_len(),
        }
    }

    pub fn new_insert(value: V, attr: Attr) -> Self {
        DeltaItem::Replace {
            value,
            attr,
            delete: 0,
        }
    }
}

impl<V: DeltaValue, Attr: Default> DeltaItem<V, Attr> {
    pub fn new_delete(len: usize) -> Self {
        DeltaItem::Replace {
            value: Default::default(),
            attr: Default::default(),
            delete: len,
        }
    }
}

impl<V: DeltaValue, Attr> HasLength for DeltaItem<V, Attr> {
    /// This would treat the len of the Delete as 0
    fn rle_len(&self) -> usize {
        self.delta_len()
    }
}

impl<V: Mergeable, Attr: PartialEq> Mergeable for DeltaItem<V, Attr> {
    fn can_merge(&self, rhs: &Self) -> bool {
        match (self, rhs) {
            (DeltaItem::Retain { attr: attr1, .. }, DeltaItem::Retain { attr: attr2, .. }) => {
                attr1 == attr2
            }
            (
                DeltaItem::Replace {
                    value: value1,
                    attr: attr1,
                    delete: _del1,
                },
                DeltaItem::Replace {
                    value: value2,
                    attr: attr2,
                    delete: _del2,
                },
            ) => value1.can_merge(value2) && attr1 == attr2,
            _ => false,
        }
    }

    fn merge_right(&mut self, rhs: &Self) {
        match (self, rhs) {
            (DeltaItem::Retain { len: len1, .. }, DeltaItem::Retain { len: len2, .. }) => {
                *len1 += len2
            }
            (
                DeltaItem::Replace {
                    value: value1,
                    delete: del1,
                    ..
                },
                DeltaItem::Replace {
                    value: value2,
                    delete: del2,
                    ..
                },
            ) => {
                value1.merge_right(value2);
                *del1 += *del2;
            }
            _ => unreachable!(),
        }
    }

    fn merge_left(&mut self, left: &Self) {
        match (self, left) {
            (DeltaItem::Retain { len: len1, .. }, DeltaItem::Retain { len: len2, .. }) => {
                *len1 += len2
            }
            (
                DeltaItem::Replace {
                    value: value1,
                    delete: del1,
                    ..
                },
                DeltaItem::Replace {
                    value: value2,
                    delete: del2,
                    ..
                },
            ) => {
                value1.merge_left(value2);
                *del1 += del2;
            }
            _ => unreachable!(),
        }
    }
}

impl<V: DeltaValue, Attr: DeltaAttr> Sliceable for DeltaItem<V, Attr> {
    fn _slice(&self, range: std::ops::Range<usize>) -> Self {
        assert!(range.end <= self.rle_len());
        match self {
            DeltaItem::Retain { len, attr } => {
                assert!(range.end <= *len);
                DeltaItem::Retain {
                    len: range.len(),
                    attr: attr.clone(),
                }
            }
            DeltaItem::Replace {
                value,
                attr,
                delete,
            } => {
                if range.end <= value.rle_len() {
                    let value = value._slice(range.clone());
                    DeltaItem::Replace {
                        value,
                        attr: attr.clone(),
                        delete: 0,
                    }
                } else if range.start >= value.rle_len() {
                    debug_assert!(range.end <= delete + value.rle_len());
                    debug_assert!(range.len() <= *delete);
                    DeltaItem::new_delete(range.len())
                } else {
                    let delete_len = range.end - value.rle_len();
                    debug_assert!(delete_len <= *delete);
                    let value = value._slice(range.start..value.rle_len());
                    DeltaItem::Replace {
                        delete: delete_len,
                        value,
                        attr: attr.clone(),
                    }
                }
            }
        }
    }

    /// slice in-place
    #[inline(always)]
    fn slice_(&mut self, range: impl RangeBounds<usize>) {
        *self = self.slice(range);
    }

    fn split(&mut self, pos: usize) -> Self {
        match self {
            DeltaItem::Retain { len, attr } => {
                let right_len = *len - pos;
                *len = pos;
                DeltaItem::Retain {
                    len: right_len,
                    attr: attr.clone(),
                }
            }
            DeltaItem::Replace {
                value,
                attr,
                delete,
            } => {
                if pos < value.rle_len() {
                    let right = value.split(pos);
                    let right_delete = *delete;
                    *delete = 0;
                    DeltaItem::Replace {
                        value: right,
                        attr: attr.clone(),
                        delete: right_delete,
                    }
                } else {
                    let right_len = value.rle_len() + *delete - pos;
                    let right = DeltaItem::new_delete(right_len);
                    *delete -= right_len;
                    right
                }
            }
        }
    }

    /// Update the slice in the given range.
    /// This method may split `self` into two or three parts.
    /// If so, it will make `self` the leftmost part and return the next split parts.
    ///
    /// # Example
    ///
    /// If `self.rle_len() == 10`, `self.update(1..5)` will split self into three parts and update the middle part.
    /// It returns the middle and the right part.
    fn update_with_split(
        &mut self,
        range: impl RangeBounds<usize>,
        f: impl FnOnce(&mut Self),
    ) -> (Option<Self>, Option<Self>) {
        let start = match range.start_bound() {
            std::ops::Bound::Included(x) => *x,
            std::ops::Bound::Excluded(x) => x + 1,
            std::ops::Bound::Unbounded => 0,
        };

        let end = match range.end_bound() {
            std::ops::Bound::Included(x) => x + 1,
            std::ops::Bound::Excluded(x) => *x,
            std::ops::Bound::Unbounded => self.rle_len(),
        };

        match (start == 0, end == self.rle_len()) {
            (true, true) => {
                f(self);
                (None, None)
            }
            (true, false) => {
                let right = self.split(end);
                f(self);
                if self.can_merge(&right) {
                    self.merge_right(&right);
                    (None, None)
                } else {
                    (Some(right), None)
                }
            }
            (false, true) => {
                let mut right = self.split(start);
                f(&mut right);
                if self.can_merge(&right) {
                    self.merge_right(&right);
                    (None, None)
                } else {
                    (Some(right), None)
                }
            }
            (false, false) => {
                let right = self.split(end);
                let mut middle = self.split(start);
                f(&mut middle);
                if middle.can_remove() {
                    if self.can_merge(&right) {
                        self.merge_right(&right);
                        (None, None)
                    } else {
                        (Some(right), None)
                    }
                } else if middle.can_merge(&right) {
                    middle.merge_right(&right);
                    if self.can_merge(&middle) {
                        self.merge_right(&middle);
                        (None, None)
                    } else {
                        (Some(middle), None)
                    }
                } else if self.can_merge(&middle) {
                    self.merge_right(&middle);
                    (Some(right), None)
                } else {
                    (Some(middle), Some(right))
                }
            }
        }
    }
}

impl<V: DeltaValue, Attr: Clone + PartialEq + Debug> TryInsert for DeltaItem<V, Attr> {
    fn try_insert(&mut self, pos: usize, elem: Self) -> Result<(), Self>
    where
        Self: Sized,
    {
        match (self, elem) {
            (
                DeltaItem::Retain { len, attr },
                DeltaItem::Retain {
                    len: len_b,
                    attr: attr_b,
                },
            ) => {
                if attr == &attr_b {
                    *len += len_b;
                    Ok(())
                } else {
                    Err(DeltaItem::Retain {
                        len: len_b,
                        attr: attr_b,
                    })
                }
            }
            (
                DeltaItem::Replace {
                    value: l_value,
                    attr: l_attr,
                    delete: l_delete,
                },
                DeltaItem::Replace {
                    value: r_value,
                    attr: r_attr,
                    delete: r_delete,
                },
            ) => {
                if l_value.rle_len() == 0 && r_value.rle_len() == 0 {
                    *l_delete += r_delete;
                    return Ok(());
                }

                if l_attr == &r_attr {
                    match l_value.try_insert(pos, r_value) {
                        Ok(_) => {
                            *l_delete += r_delete;
                            return Ok(());
                        }
                        Err(r_value) => {
                            return Err(DeltaItem::Replace {
                                value: r_value,
                                attr: r_attr,
                                delete: r_delete,
                            })
                        }
                    }
                }

                Err(DeltaItem::Replace {
                    value: r_value,
                    attr: r_attr,
                    delete: r_delete,
                })
            }
            (_, a) => Err(a),
        }
    }
}

impl<V: DeltaValue, Attr: Clone> CanRemove for DeltaItem<V, Attr> {
    fn can_remove(&self) -> bool {
        match self {
            DeltaItem::Retain { len, .. } => *len == 0,
            DeltaItem::Replace {
                value,
                attr: _,
                delete,
            } => value.rle_len() == 0 && *delete == 0,
        }
    }
}

impl<V: DeltaValue, Attr: DeltaAttr> Default for DeltaItem<V, Attr> {
    fn default() -> Self {
        DeltaItem::Retain {
            len: 0,
            attr: Default::default(),
        }
    }
}

impl<V: DeltaValue, Attr: DeltaAttr> DeltaValue for DeltaItem<V, Attr> {}

use generic_btree::rle::{CanRemove, TryInsert};

use super::*;

impl<V: DeltaValue, Attr: DeltaAttr> DeltaItem<V, Attr> {
    /// The real length of the item in the delta
    pub fn delta_len(&self) -> usize {
        match self {
            DeltaItem::Retain { len, .. } => *len,
            DeltaItem::Replace {
                value,
                attr,
                delete,
            } => value.rle_len() + delete,
        }
    }

    pub fn new_delete(len: usize) -> Self {
        DeltaItem::Replace {
            value: Default::default(),
            attr: Default::default(),
            delete: len,
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

impl<V: DeltaValue, Attr> HasLength for DeltaItem<V, Attr> {
    /// This would treat the len of the Delete as 0
    fn rle_len(&self) -> usize {
        match self {
            DeltaItem::Retain { len, .. } => *len,
            DeltaItem::Replace { value, delete, .. } => value.rle_len(),
        }
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
                    delete: del1,
                },
                DeltaItem::Replace {
                    value: value2,
                    attr: attr2,
                    delete: del2,
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

impl<V: DeltaValue, Attr: Clone> Sliceable for DeltaItem<V, Attr> {
    fn _slice(&self, range: std::ops::Range<usize>) -> Self {
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
                let value = value._slice(range.clone());
                DeltaItem::Replace {
                    value,
                    attr: attr.clone(),
                    delete: *delete,
                }
            }
        }
    }
}

impl<V: DeltaValue, Attr: Clone + PartialEq> TryInsert for DeltaItem<V, Attr> {
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
                if l_attr == &r_attr {
                    match l_value.try_insert(pos, r_value) {
                        Ok(_) => return Ok(()),
                        Err(v) => {
                            return Err(DeltaItem::Replace {
                                value: v,
                                attr: l_attr.clone(),
                                delete: *l_delete + r_delete,
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
                attr,
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

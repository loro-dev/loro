use generic_btree::rle::{CanRemove, TryInsert};

use super::*;

impl<V: DeltaValue, Attr> HasLength for DeltaItem<V, Attr> {
    fn rle_len(&self) -> usize {
        match self {
            DeltaItem::Delete(_) => 0,
            DeltaItem::Retain { len, .. } => *len,
            DeltaItem::Insert { value, .. } => value.rle_len(),
        }
    }
}

impl<V: Mergeable, Attr: PartialEq> Mergeable for DeltaItem<V, Attr> {
    fn can_merge(&self, rhs: &Self) -> bool {
        match (self, rhs) {
            (DeltaItem::Delete(_), DeltaItem::Delete(_)) => true,
            (DeltaItem::Retain { attr: attr1, .. }, DeltaItem::Retain { attr: attr2, .. }) => {
                attr1 == attr2
            }
            (
                DeltaItem::Insert {
                    value: value1,
                    attr: attr1,
                },
                DeltaItem::Insert {
                    value: value2,
                    attr: attr2,
                },
            ) => value1.can_merge(value2) && attr1 == attr2,
            _ => false,
        }
    }

    fn merge_right(&mut self, rhs: &Self) {
        match (self, rhs) {
            (DeltaItem::Delete(a), DeltaItem::Delete(b)) => {
                *a += *b;
            }
            (DeltaItem::Retain { len: len1, .. }, DeltaItem::Retain { len: len2, .. }) => {
                *len1 += len2
            }
            (DeltaItem::Insert { value: value1, .. }, DeltaItem::Insert { value: value2, .. }) => {
                value1.merge_right(value2);
            }
            _ => unreachable!(),
        }
    }

    fn merge_left(&mut self, left: &Self) {
        match (self, left) {
            (DeltaItem::Delete(a), DeltaItem::Delete(b)) => {
                *a += *b;
            }
            (DeltaItem::Retain { len: len1, .. }, DeltaItem::Retain { len: len2, .. }) => {
                *len1 += len2
            }
            (DeltaItem::Insert { value: value1, .. }, DeltaItem::Insert { value: value2, .. }) => {
                value1.merge_left(value2);
            }
            _ => unreachable!(),
        }
    }
}

impl<V: DeltaValue, Attr: Clone> Sliceable for DeltaItem<V, Attr> {
    fn _slice(&self, range: std::ops::Range<usize>) -> Self {
        match self {
            DeltaItem::Delete(d) => {
                assert!(range.end < *d);
                DeltaItem::Delete(range.len())
            }
            DeltaItem::Retain { len, attr } => {
                assert!(range.end < *len);
                DeltaItem::Retain {
                    len: range.len(),
                    attr: attr.clone(),
                }
            }
            DeltaItem::Insert { value, attr } => {
                let value = value._slice(range.clone());
                DeltaItem::Insert {
                    value,
                    attr: attr.clone(),
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
            (DeltaItem::Delete(a), DeltaItem::Delete(b)) => {
                *a += b;
                Ok(())
            }
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
                DeltaItem::Insert {
                    value: l_value,
                    attr: l_attr,
                },
                DeltaItem::Insert {
                    value: r_value,
                    attr: r_attr,
                },
            ) => {
                if l_attr == &r_attr {
                    match l_value.try_insert(pos, r_value) {
                        Ok(_) => return Ok(()),
                        Err(v) => {
                            return Err(DeltaItem::Insert {
                                value: v,
                                attr: l_attr.clone(),
                            })
                        }
                    }
                }

                Err(DeltaItem::Insert {
                    value: r_value,
                    attr: r_attr,
                })
            }
            (_, a) => Err(a),
        }
    }
}

impl<V: DeltaValue, Attr: Clone> CanRemove for DeltaItem<V, Attr> {
    fn can_remove(&self) -> bool {
        match self {
            DeltaItem::Delete(len) => *len == 0,
            DeltaItem::Retain { len, .. } => *len == 0,
            DeltaItem::Insert { value, .. } => value.rle_len() == 0,
        }
    }
}

impl<V: DeltaValue, Attr: DeltaAttr> DeltaValue for DeltaItem<V, Attr> {}

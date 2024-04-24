use tracing::trace;

use super::*;

struct DeltaReplace<'a, V, Attr> {
    value: &'a V,
    attr: &'a Attr,
    delete: usize,
}

impl<V: DeltaValue, Attr: DeltaAttr> DeltaRope<V, Attr> {
    pub fn compose(&mut self, other: &Self) {
        // TODO: Need to implement a slow mode that is guaranteed to be correct, then we can fuzz on it
        if self.is_empty() {
            *self = other.clone();
            return;
        }

        // trace!("Composing {:#?}\n{:#?}", &self, &other);
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
                        self.update_attr_in_range(index..index + len, attr);
                    }
                    index += len;
                }
                DeltaItem::Replace {
                    value: this_value,
                    attr: this_attr,
                    delete,
                } => {
                    self._compose_replace(
                        DeltaReplace {
                            value: this_value,
                            attr: this_attr,
                            delete: *delete,
                        },
                        &mut index,
                    );
                }
            }
        }

        // trace!("Composed {:#?}", &self);
    }

    fn _compose_replace(
        &mut self,
        delta_replace_item @ DeltaReplace {
            value: this_value,
            attr: this_attr,
            delete,
        }: DeltaReplace<V, Attr>,
        index: &mut usize,
    ) {
        let mut should_insert = this_value.rle_len() > 0;
        let mut left_del_len = delete;
        if delete > 0 {
            assert!(*index < self.len());
            let range = *index..(*index + left_del_len).min(self.len());
            let from = self.tree.query::<LengthFinder>(&range.start).unwrap();
            let to = self.tree.query::<LengthFinder>(&range.end).unwrap();
            if from.cursor.leaf == to.cursor.leaf {
                self._replace_on_single_leaf(from, to, left_del_len, delta_replace_item);
                should_insert = false;
                left_del_len = 0;
            } else {
                self._replace_batch_leaves(from, to, &mut left_del_len);
            }
        }

        if left_del_len > 0 || should_insert {
            let replace = DeltaItem::Replace {
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
            };
            self.insert_values(*index, [replace]);
        }

        *index += this_value.rle_len();
    }

    fn _replace_batch_leaves(
        &mut self,
        from: generic_btree::QueryResult,
        to: generic_btree::QueryResult,
        left_del_len: &mut usize,
    ) {
        self.tree.update(from.cursor..to.cursor, &mut |item| {
            // This method will split the leaf node before calling this closure.
            // So it's guaranteed that the item is contained in the range.
            if *left_del_len == 0 {
                return None;
            }

            match item {
                DeltaItem::Retain { len, .. } => {
                    assert!(*len <= *left_del_len);
                    *left_del_len -= *len;
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
                    if *left_del_len >= value.rle_len() {
                        let diff = value.rle_len() as isize;
                        *left_del_len -= value.rle_len();
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

    fn _replace_on_single_leaf(
        &mut self,
        from: generic_btree::QueryResult,
        to: generic_btree::QueryResult,
        left_del_len: usize,
        DeltaReplace {
            value: this_value,
            attr: this_attr,
            delete: _,
        }: DeltaReplace<V, Attr>,
    ) {
        self.tree.update_leaf(from.cursor.leaf, |item| match item {
            DeltaItem::Retain {
                len: retain_len, ..
            } => {
                let start = from.cursor.offset;
                let end = to.cursor.offset;
                debug_assert!(end <= *retain_len);
                let (l, r) = item.update_with_split(start..end, |item| {
                    *item = DeltaItem::Replace {
                        delete: left_del_len,
                        value: this_value.clone(),
                        attr: this_attr.clone(),
                    };
                });

                (true, l, r)
            }
            DeltaItem::Replace { value, delete, .. } => {
                let start = from.cursor.offset;
                let end = to.cursor.offset;
                let value_len = value.rle_len();
                let value_start = start.min(value_len);
                let value_end = value_len.min(end);
                {
                    // We need to remove the part of value that is between start and end.
                    // If the range is out of the bounds of the value, we record extra deletions
                    // on the `delete` field of this item.
                    let left = left_del_len.saturating_sub(value_end - value_start);
                    *delete += left;
                }

                let (l, r) = item.update_with_split(value_start..value_end, |item| {
                    *item = DeltaItem::Replace {
                        value: this_value.clone(),
                        attr: this_attr.clone(),
                        delete: 0,
                    };
                });

                (true, l, r)
            }
        });
    }
}

use super::*;

struct DeltaReplace<'a, V, Attr> {
    value: &'a V,
    attr: &'a Attr,
    delete: usize,
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use generic_btree::rle::{HasLength, Mergeable, Sliceable, TryInsert};

    use super::*;

    thread_local! {
        static LENGTH_READS: Cell<usize> = const { Cell::new(0) };
    }

    #[derive(Debug, Clone, Default)]
    struct MeasuredLength(usize);

    impl HasLength for MeasuredLength {
        fn rle_len(&self) -> usize {
            LENGTH_READS.with(|reads| reads.set(reads.get() + 1));
            self.0
        }
    }

    impl Sliceable for MeasuredLength {
        fn _slice(&self, range: std::ops::Range<usize>) -> Self {
            Self(range.len())
        }
    }

    impl Mergeable for MeasuredLength {
        fn can_merge(&self, _: &Self) -> bool {
            true
        }

        fn merge_right(&mut self, other: &Self) {
            self.0 += other.0;
        }

        fn merge_left(&mut self, other: &Self) {
            self.0 += other.0;
        }
    }

    impl TryInsert for MeasuredLength {
        fn try_insert(&mut self, _: usize, other: Self) -> Result<(), Self> {
            self.0 += other.0;
            Ok(())
        }
    }

    impl DeltaValue for MeasuredLength {}

    #[test]
    fn repeated_backward_deletes_do_not_rescan_deleted_suffixes() {
        let count = 4096;
        let mut delta = DeltaRope::<MeasuredLength, ()>::new();
        LENGTH_READS.with(|reads| reads.set(0));
        for position in (0..count).rev() {
            let next = DeltaRopeBuilder::new()
                .retain(position, ())
                .delete(1)
                .build();
            delta.compose(&next);
        }
        let reads = LENGTH_READS.with(Cell::get);
        assert!(
            reads < count * 256,
            "{reads} length reads for {count} deletes"
        );
        assert_eq!(delta.len(), 0);
        assert_eq!(delta.old_len(), count);
    }

    #[test]
    fn repeated_fixed_offset_deletes_before_suffix_coalesce_fragments() {
        let count = 4096;
        let mut delta = DeltaRopeBuilder::<MeasuredLength, ()>::new()
            .retain(count + 16, ())
            .build();
        for _ in 0..count {
            let next = DeltaRopeBuilder::new().retain(5, ()).delete(1).build();
            delta.compose(&next);
        }
        assert!(
            delta.node_len() <= 4,
            "expected coalesced leaves, got {} leaves",
            delta.node_len()
        );
        assert_eq!(delta.len(), 16);
        assert_eq!(delta.old_len(), count + 16);
    }
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

    pub fn delete(&mut self, mut index: usize, len: usize) {
        self._compose_replace(
            DeltaReplace {
                value: &Default::default(),
                attr: &Default::default(),
                delete: len,
            },
            &mut index,
        );
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
        if *index > self.len() {
            self.push_retain(*index - self.len(), Attr::default());
        }

        if delete > 0 && *index != self.len() {
            let range = *index..(*index + left_del_len).min(self.len());
            let from = self.tree.query::<LengthFinder>(&range.start).unwrap();
            let mut to = self.tree.query::<LengthFinder>(&(range.end - 1)).unwrap();
            to.cursor.offset += 1;
            if from.cursor.leaf == to.cursor.leaf {
                self._replace_on_single_leaf(from, to, left_del_len, delta_replace_item);
                should_insert = false;
                left_del_len = 0;
            } else if !should_insert {
                self._delete_batch_leaves(from, to, &mut left_del_len);
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

    fn _delete_batch_leaves(
        &mut self,
        from: generic_btree::QueryResult,
        to: generic_btree::QueryResult,
        left_del_len: &mut usize,
    ) {
        let absorb_into_prev = if from.cursor.offset == 0 {
            self.tree.prev_elem(from.cursor).and_then(|prev| {
                matches!(
                    self.tree.get_elem(prev.leaf),
                    Some(DeltaItem::Replace { .. })
                )
                .then_some(prev.leaf)
            })
        } else {
            matches!(
                self.tree.get_elem(from.cursor.leaf),
                Some(DeltaItem::Replace { .. })
            )
            .then_some(from.cursor.leaf)
        };

        let mut kept_first = false;
        let mut total_input_deletes = 0usize;
        self.tree
            .update(from.cursor..to.cursor, &mut |item| match item {
                DeltaItem::Retain { len, .. } => {
                    let consumed = *len;
                    assert!(consumed <= *left_del_len);
                    *left_del_len -= consumed;
                    total_input_deletes += consumed;
                    if absorb_into_prev.is_none() && !kept_first {
                        kept_first = true;
                        *item = DeltaItem::Replace {
                            delete: 1,
                            value: Default::default(),
                            attr: Default::default(),
                        };
                        Some(Len {
                            data_len: -(consumed as isize),
                            delta_len: 1 - (consumed as isize),
                        })
                    } else {
                        *len = 0;
                        Some(Len {
                            data_len: -(consumed as isize),
                            delta_len: -(consumed as isize),
                        })
                    }
                }
                DeltaItem::Replace {
                    value,
                    attr,
                    delete,
                } => {
                    let vlen = value.rle_len();
                    assert!(*left_del_len >= vlen);
                    *left_del_len -= vlen;
                    let old_del = *delete;
                    total_input_deletes += old_del;
                    *value = Default::default();
                    *attr = Default::default();
                    if absorb_into_prev.is_none() && !kept_first {
                        kept_first = true;
                        *delete = 1;
                        Some(Len {
                            data_len: -(vlen as isize),
                            delta_len: 1 - ((vlen + old_del) as isize),
                        })
                    } else {
                        *delete = 0;
                        Some(Len {
                            data_len: -(vlen as isize),
                            delta_len: -((vlen + old_del) as isize),
                        })
                    }
                }
            });

        let final_delete = total_input_deletes + std::mem::take(left_del_len);
        if let Some(prev_leaf) = absorb_into_prev {
            if final_delete > 0 {
                self.tree.update_leaf(prev_leaf, |item| {
                    let DeltaItem::Replace { delete, .. } = item else {
                        unreachable!()
                    };
                    *delete += final_delete;
                    (true, None, None)
                });
            }
        } else if kept_first {
            let target_leaf = if from.cursor.offset == 0 {
                from.cursor.leaf
            } else {
                self.tree
                    .next_elem(generic_btree::Cursor {
                        leaf: from.cursor.leaf,
                        offset: 0,
                    })
                    .expect("kept placeholder leaf follows split prefix")
                    .leaf
            };
            self.tree.update_leaf(target_leaf, |item| {
                let DeltaItem::Replace { delete, .. } = item else {
                    unreachable!()
                };
                *delete = final_delete;
                (true, None, None)
            });
            if final_delete > 0 {
                if let Some(next) = self.tree.next_elem(generic_btree::Cursor {
                    leaf: target_leaf,
                    offset: 0,
                }) {
                    if let Some(DeltaItem::Replace {
                        value,
                        delete: next_del,
                        ..
                    }) = self.tree.get_elem(next.leaf)
                    {
                        if value.rle_len() == 0 {
                            let extra = *next_del;
                            self.tree.update_leaf(target_leaf, |item| {
                                let DeltaItem::Replace { delete, .. } = item else {
                                    unreachable!()
                                };
                                *delete += extra;
                                (true, None, None)
                            });
                            self.tree.update_leaf(next.leaf, |item| {
                                let DeltaItem::Replace { delete, .. } = item else {
                                    unreachable!()
                                };
                                *delete = 0;
                                (true, None, None)
                            });
                        }
                    }
                }
            }
        }
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
        if this_value.rle_len() == 0 && from.cursor.offset == 0 {
            if let Some(prev) = self.tree.prev_elem(from.cursor) {
                if matches!(
                    self.tree.get_elem(prev.leaf),
                    Some(DeltaItem::Replace { .. })
                ) {
                    let end = to.cursor.offset;
                    let mut absorbed_del = None;
                    self.tree.update_leaf(from.cursor.leaf, |item| match item {
                        DeltaItem::Retain {
                            len: retain_len, ..
                        } => {
                            debug_assert!(end <= *retain_len);
                            *retain_len -= end;
                            absorbed_del = Some(left_del_len);
                            (true, None, None)
                        }
                        DeltaItem::Replace { value, delete, .. } => {
                            let value_len = value.rle_len();
                            let value_end = value_len.min(end);
                            let left = left_del_len.saturating_sub(value_end);
                            if value_end == value_len {
                                absorbed_del = Some(*delete + left);
                                *value = Default::default();
                                *delete = 0;
                                (true, None, None)
                            } else {
                                let right = value.split(value_end);
                                *value = right;
                                absorbed_del = Some(left);
                                (true, None, None)
                            }
                        }
                    });
                    if let Some(extra) = absorbed_del.filter(|&d| d > 0) {
                        self.tree.update_leaf(prev.leaf, |item| {
                            let DeltaItem::Replace { delete, .. } = item else {
                                unreachable!()
                            };
                            *delete += extra;
                            (true, None, None)
                        });
                    }
                    return;
                }
            }
        }
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

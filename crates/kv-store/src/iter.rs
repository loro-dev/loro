use std::fmt::Debug;

use bytes::Bytes;

/// When you need peek next key and value and next back key and value, use this trait.
pub trait KvIterator: Debug + DoubleEndedIterator<Item = (Bytes, Bytes)> {
    fn next_key(&self) -> Option<Bytes>;
    fn next_value(&self) -> Option<Bytes>;
    fn next_(&mut self);
    fn has_next(&self) -> bool;
    fn next_back_key(&self) -> Option<Bytes>;
    fn next_back_value(&self) -> Option<Bytes>;
    fn next_back_(&mut self);
    fn has_next_back(&self) -> bool;
}

/// Merge multiple iterators into one.
///
/// The iterators are merged in the order they are provided.
/// If two iterators have the same key, the value from the iterator with the smallest index is used.
///
/// Note: This implementation is not optimized for lots of iterators.
/// You should only use this when you have a small number of iterators.
#[derive(Debug)]
pub struct MergeIterator<T: KvIterator> {
    iters: Vec<T>,
}

impl<T: KvIterator> MergeIterator<T> {
    pub fn new(iters: Vec<T>) -> Self {
        Self { iters }
    }
}

impl<T: KvIterator> Iterator for MergeIterator<T> {
    type Item = (Bytes, Bytes);

    fn next(&mut self) -> Option<Self::Item> {
        let mut min_key = None;
        let mut min_index = None;
        let mut has_to_remove = false;
        for (i, iter) in self.iters.iter_mut().enumerate() {
            if let Some(key) = iter.next_key() {
                if let Some(this_min_key) = &min_key {
                    match key.cmp(this_min_key) {
                        std::cmp::Ordering::Less => {
                            min_key = Some(key);
                            min_index = Some(i);
                        }
                        std::cmp::Ordering::Equal => {
                            // the same key, skip it
                            iter.next();
                        }
                        std::cmp::Ordering::Greater => {}
                    }
                } else {
                    min_key = Some(key);
                    min_index = Some(i);
                }
            } else {
                has_to_remove = true;
            }
        }

        let ans = if let Some(idx) = min_index {
            self.iters[idx].next()
        } else {
            None
        };

        if has_to_remove {
            self.iters.retain(|x| x.has_next());
        }
        ans
    }
}

impl<T: KvIterator> DoubleEndedIterator for MergeIterator<T> {
    fn next_back(&mut self) -> Option<Self::Item> {
        let mut max_key = None;
        let mut max_index = None;
        let mut has_to_remove = false;
        for (i, iter) in self.iters.iter_mut().enumerate() {
            if let Some(key) = iter.next_back_key() {
                if let Some(this_max_key) = &max_key {
                    match key.cmp(this_max_key) {
                        std::cmp::Ordering::Less => {}
                        std::cmp::Ordering::Equal => {
                            // the same key, skip it
                            iter.next_back();
                        }
                        std::cmp::Ordering::Greater => {
                            max_key = Some(key);
                            max_index = Some(i);
                        }
                    }
                } else {
                    max_key = Some(key);
                    max_index = Some(i);
                }
            } else {
                has_to_remove = true;
            }
        }
        let ans = if let Some(idx) = max_index {
            self.iters[idx].next_back()
        } else {
            None
        };

        if has_to_remove {
            self.iters.retain(|x| x.has_next_back());
        }
        ans
    }
}

#[cfg(test)]
mod tests {
    use std::ops::Bound;

    use super::*;
    use crate::{compress::CompressionType, sstable};
    use bytes::Bytes;
    #[test]
    fn test_merge_iterator() {
        let a = Bytes::from("a");
        let b = Bytes::from("b");
        let c = Bytes::from("c");
        let d = Bytes::from("d");

        let mut sstable1 = sstable::SsTableBuilder::new(10, CompressionType::LZ4);
        sstable1.add(a.clone(), a.clone());
        sstable1.add(c.clone(), c.clone());
        let sstable1 = sstable1.build();
        let iter1 = sstable::SsTableIter::new_scan(&sstable1, Bound::Unbounded, Bound::Unbounded);

        let mut sstable2 = sstable::SsTableBuilder::new(10, CompressionType::LZ4);
        sstable2.add(b.clone(), b.clone());
        sstable2.add(d.clone(), d.clone());
        let sstable2 = sstable2.build();
        let iter2 = sstable::SsTableIter::new_scan(&sstable2, Bound::Unbounded, Bound::Unbounded);

        let merged_iter = MergeIterator::new(vec![iter1.clone(), iter2.clone()]);
        let ans = merged_iter.collect::<Vec<_>>();
        assert_eq!(
            ans,
            vec![
                (a.clone(), a.clone()),
                (b.clone(), b.clone()),
                (c.clone(), c.clone()),
                (d.clone(), d.clone())
            ]
        );

        let merged_iter = MergeIterator::new(vec![iter1.clone(), iter2.clone()]);
        let ans2 = merged_iter.rev().collect::<Vec<_>>();
        assert_eq!(ans2, ans.iter().rev().cloned().collect::<Vec<_>>());
    }

    #[test]
    fn same_key() {
        let a = Bytes::from("a");
        let a2 = Bytes::from("a2");
        let c = Bytes::from("c");
        let d = Bytes::from("d");

        let mut sstable1 = sstable::SsTableBuilder::new(10, CompressionType::LZ4);
        sstable1.add(a.clone(), a.clone());
        sstable1.add(c.clone(), c.clone());
        let sstable1 = sstable1.build();
        let iter1 = sstable::SsTableIter::new_scan(&sstable1, Bound::Unbounded, Bound::Unbounded);

        let mut sstable2 = sstable::SsTableBuilder::new(10, CompressionType::LZ4);
        sstable2.add(a.clone(), a2.clone());
        sstable2.add(d.clone(), d.clone());
        let sstable2 = sstable2.build();
        let iter2 = sstable::SsTableIter::new_scan(&sstable2, Bound::Unbounded, Bound::Unbounded);

        let merged_iter = MergeIterator::new(vec![iter1.clone(), iter2.clone()]);
        let ans = merged_iter.collect::<Vec<_>>();
        assert_eq!(
            ans,
            vec![
                (a.clone(), a.clone()),
                (c.clone(), c.clone()),
                (d.clone(), d.clone())
            ]
        );
    }
}

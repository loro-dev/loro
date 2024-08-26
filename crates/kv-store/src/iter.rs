use std::{
    cell::RefCell,
    collections::{binary_heap::PeekMut, BinaryHeap},
    fmt::Debug,
    rc::Rc,
};

use bytes::Bytes;

pub trait KvIterator: Debug {
    fn next_key(&self) -> Bytes;
    fn next_value(&self) -> Bytes;
    fn find_next(&mut self);
    fn has_next(&self) -> bool;
    fn next_back_key(&self) -> Bytes;
    fn next_back_value(&self) -> Bytes;
    fn find_next_back(&mut self);
    fn has_next_back(&self) -> bool;
}

#[derive(Debug)]
struct HeapIterWrapper<T> {
    pub idx: usize,
    pub iter: Rc<RefCell<T>>,
    pub f2b: bool,
}

impl<T: KvIterator> Ord for HeapIterWrapper<T> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        if self.f2b {
            self.iter
                .borrow()
                .next_key()
                .cmp(&other.iter.borrow().next_key())
                .then(self.idx.cmp(&other.idx))
                .reverse()
        } else {
            self.iter
                .borrow()
                .next_back_key()
                .cmp(&other.iter.borrow().next_back_key())
                .then(self.idx.cmp(&other.idx).reverse())
        }
    }
}

impl<T: KvIterator> PartialOrd for HeapIterWrapper<T> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl<T: KvIterator> PartialEq for HeapIterWrapper<T> {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == std::cmp::Ordering::Equal
    }
}

impl<T: KvIterator> Eq for HeapIterWrapper<T> {}

#[derive(Debug)]
pub struct MergeIterator<T: KvIterator> {
    next_iters: BinaryHeap<HeapIterWrapper<T>>,
    next_current: Option<HeapIterWrapper<T>>,
    back_iters: BinaryHeap<HeapIterWrapper<T>>,
    back_current: Option<HeapIterWrapper<T>>,
}

impl<T: KvIterator> MergeIterator<T> {
    pub fn new(iters: Vec<T>) -> Self {
        if iters.is_empty() {
            return Self {
                next_iters: BinaryHeap::new(),
                next_current: None,
                back_iters: BinaryHeap::new(),
                back_current: None,
            };
        }
        let mut next_heap = BinaryHeap::new();
        let mut back_heap = BinaryHeap::new();
        let shared_iters: Vec<Rc<RefCell<T>>> = iters
            .into_iter()
            .map(|iter| Rc::new(RefCell::new(iter)))
            .collect();

        let next_current = if shared_iters.iter().all(|x| !x.borrow().has_next()) {
            Some(HeapIterWrapper {
                idx: 0,
                iter: Rc::clone(shared_iters.last().unwrap()),
                f2b: true,
            })
        } else {
            for (idx, iter) in shared_iters.iter().enumerate() {
                if iter.borrow().has_next() {
                    next_heap.push(HeapIterWrapper {
                        idx,
                        iter: Rc::clone(iter),
                        f2b: true,
                    });
                }
            }
            next_heap.pop()
        };

        let back_current = if shared_iters.iter().all(|x| !x.borrow().has_next_back()) {
            Some(HeapIterWrapper {
                idx: 0,
                iter: Rc::clone(shared_iters.first().unwrap()),
                f2b: false,
            })
        } else {
            for (idx, iter) in shared_iters.iter().enumerate() {
                if iter.borrow().has_next_back() {
                    back_heap.push(HeapIterWrapper {
                        idx,
                        iter: Rc::clone(iter),
                        f2b: false,
                    });
                }
            }
            back_heap.pop()
        };

        Self {
            next_iters: next_heap,
            next_current,
            back_iters: back_heap,
            back_current,
        }
    }
}

impl<T: KvIterator> KvIterator for MergeIterator<T> {
    fn next_key(&self) -> Bytes {
        self.next_current.as_ref().unwrap().iter.borrow().next_key()
    }

    fn next_value(&self) -> Bytes {
        self.next_current
            .as_ref()
            .unwrap()
            .iter
            .borrow()
            .next_value()
    }

    fn find_next(&mut self) {
        let next_current = self.next_current.as_mut().unwrap();
        while let Some(inner_iter) = self.next_iters.peek_mut() {
            debug_assert!(
                inner_iter.iter.borrow().next_key() >= next_current.iter.borrow().next_key()
            );
            if inner_iter.iter.borrow().next_key() == next_current.iter.borrow().next_key() {
                inner_iter.iter.borrow_mut().find_next();
                if !inner_iter.iter.borrow().has_next() {
                    PeekMut::pop(inner_iter);
                }
            } else {
                break;
            }
        }
        next_current.iter.borrow_mut().find_next();
        if !next_current.iter.borrow().has_next() {
            if let Some(iter) = self.next_iters.pop() {
                *next_current = iter;
            }
            return;
        }

        if let Some(mut iter) = self.next_iters.peek_mut() {
            if *next_current < *iter {
                std::mem::swap(&mut *iter, next_current)
            }
        }
    }

    fn has_next(&self) -> bool {
        self.next_current
            .as_ref()
            .map(|x| x.iter.borrow().has_next())
            .unwrap_or(false)
    }

    fn next_back_key(&self) -> Bytes {
        self.back_current
            .as_ref()
            .unwrap()
            .iter
            .borrow()
            .next_back_key()
    }

    fn next_back_value(&self) -> Bytes {
        self.back_current
            .as_ref()
            .unwrap()
            .iter
            .borrow()
            .next_back_value()
    }

    fn find_next_back(&mut self) {
        let back_current = self.back_current.as_mut().unwrap();
        while let Some(inner_iter) = self.back_iters.peek_mut() {
            debug_assert!(
                inner_iter.iter.borrow().next_back_key()
                    <= back_current.iter.borrow().next_back_key()
            );
            if inner_iter.iter.borrow().next_back_key()
                == back_current.iter.borrow().next_back_key()
            {
                inner_iter.iter.borrow_mut().find_next_back();
                if !inner_iter.iter.borrow().has_next_back() {
                    PeekMut::pop(inner_iter);
                }
            } else {
                break;
            }
        }
        back_current.iter.borrow_mut().find_next_back();
        if !back_current.iter.borrow().has_next_back() {
            if let Some(iter) = self.back_iters.pop() {
                *back_current = iter;
            }
            return;
        }

        if let Some(mut iter) = self.back_iters.peek_mut() {
            if *back_current < *iter {
                std::mem::swap(&mut *iter, back_current)
            }
        }
    }

    fn has_next_back(&self) -> bool {
        self.back_current
            .as_ref()
            .map(|x| x.iter.borrow().has_next_back())
            .unwrap_or(false)
    }
}

#[derive(Debug)]
pub struct FilterEmptyIter<T: KvIterator> {
    iter: T,
}

impl<T: KvIterator> FilterEmptyIter<T> {
    pub fn new(mut iter: T) -> Self {
        while iter.has_next() && iter.next_value().is_empty() {
            iter.find_next();
        }
        while iter.has_next_back() && iter.next_back_value().is_empty() {
            iter.find_next_back();
        }

        Self { iter }
    }
}

impl<T: KvIterator> KvIterator for FilterEmptyIter<T> {
    fn next_key(&self) -> Bytes {
        self.iter.next_key()
    }

    fn next_value(&self) -> Bytes {
        self.iter.next_value()
    }

    fn find_next(&mut self) {
        self.iter.find_next();
        while self.has_next() && self.iter.next_value().is_empty() {
            self.iter.find_next();
        }
    }

    fn has_next(&self) -> bool {
        self.iter.has_next()
    }

    fn next_back_key(&self) -> Bytes {
        self.iter.next_back_key()
    }

    fn next_back_value(&self) -> Bytes {
        self.iter.next_back_value()
    }

    fn find_next_back(&mut self) {
        self.iter.find_next_back();
        while self.has_next_back() && self.iter.next_back_value().is_empty() {
            self.iter.find_next_back();
        }
    }

    fn has_next_back(&self) -> bool {
        self.iter.has_next_back()
    }
}

impl<T: KvIterator> Iterator for MergeIterator<T> {
    type Item = (Bytes, Bytes);

    fn next(&mut self) -> Option<Self::Item> {
        if !self.has_next() {
            return None;
        }
        let key = self.next_key();
        let value = self.next_value();
        KvIterator::find_next(self);
        Some((key, value))
    }
}

impl<T: KvIterator> DoubleEndedIterator for MergeIterator<T> {
    fn next_back(&mut self) -> Option<Self::Item> {
        if !self.has_next_back() {
            return None;
        }
        let key = self.next_back_key();
        let value = self.next_back_value();
        KvIterator::find_next_back(self);
        Some((key, value))
    }
}

impl<T: KvIterator> Iterator for FilterEmptyIter<T> {
    type Item = (Bytes, Bytes);

    fn next(&mut self) -> Option<Self::Item> {
        if !self.has_next() {
            return None;
        }
        let key = self.next_key();
        let value = self.next_value();
        KvIterator::find_next(self);
        Some((key, value))
    }
}

impl<T: KvIterator> DoubleEndedIterator for FilterEmptyIter<T> {
    fn next_back(&mut self) -> Option<Self::Item> {
        if !self.has_next_back() {
            return None;
        }
        let key = self.next_back_key();
        let value = self.next_back_value();
        KvIterator::find_next_back(self);
        Some((key, value))
    }
}

#[cfg(test)]
mod tests {
    use std::ops::Bound;

    use super::*;
    use crate::sstable;
    use bytes::Bytes;
    #[test]
    fn test_merge_iterator() {
        let a = Bytes::from("a");
        let b = Bytes::from("b");
        let c = Bytes::from("c");
        let d = Bytes::from("d");

        let mut sstable1 = sstable::SsTableBuilder::new(10);
        sstable1.add(a.clone(), a.clone());
        sstable1.add(c.clone(), c.clone());
        let sstable1 = sstable1.build();
        let iter1 = sstable::SsTableIter::new_scan(&sstable1, Bound::Unbounded, Bound::Unbounded);

        let mut sstable2 = sstable::SsTableBuilder::new(10);
        sstable2.add(b.clone(), b.clone());
        sstable2.add(d.clone(), d.clone());
        let sstable2 = sstable2.build();
        let iter2 = sstable::SsTableIter::new_scan(&sstable2, Bound::Unbounded, Bound::Unbounded);

        let merged_iter = MergeIterator::new(vec![iter1.clone(), iter2.clone()]);
        assert_eq!(merged_iter.next_key(), a.clone());
        assert_eq!(merged_iter.next_value(), a.clone());
        assert_eq!(merged_iter.next_back_key(), d.clone());
        assert_eq!(merged_iter.next_back_value(), d.clone());
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
        assert_eq!(merged_iter.next_key(), a.clone());
        assert_eq!(merged_iter.next_value(), a.clone());
        assert_eq!(merged_iter.next_back_key(), d.clone());
        assert_eq!(merged_iter.next_back_value(), d.clone());
        let ans2 = merged_iter.rev().collect::<Vec<_>>();
        assert_eq!(ans2, ans.iter().rev().cloned().collect::<Vec<_>>());
    }

    #[test]
    fn same_key() {
        let a = Bytes::from("a");
        let a2 = Bytes::from("a2");
        let c = Bytes::from("c");
        let d = Bytes::from("d");

        let mut sstable1 = sstable::SsTableBuilder::new(10);
        sstable1.add(a.clone(), a.clone());
        sstable1.add(c.clone(), c.clone());
        let sstable1 = sstable1.build();
        let iter1 = sstable::SsTableIter::new_scan(&sstable1, Bound::Unbounded, Bound::Unbounded);

        let mut sstable2 = sstable::SsTableBuilder::new(10);
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

    #[test]
    fn empty_iter() {
        let a = Bytes::from("a");
        let b = Bytes::from("b");
        let c = Bytes::from("c");
        let d = Bytes::from("d");

        let mut sstable1 = sstable::SsTableBuilder::new(10);
        sstable1.add(a.clone(), Bytes::new());
        sstable1.add(c.clone(), c.clone());
        let sstable1 = sstable1.build();
        let iter1 = sstable::SsTableIter::new_scan(&sstable1, Bound::Unbounded, Bound::Unbounded);

        let mut sstable2 = sstable::SsTableBuilder::new(0);
        sstable2.add(a.clone(), a.clone());
        sstable2.add(b.clone(), Bytes::new());
        sstable2.add(d.clone(), d.clone());
        let sstable2 = sstable2.build();
        let iter2 = sstable::SsTableIter::new_scan(&sstable2, Bound::Unbounded, Bound::Unbounded);

        let merged_iter = MergeIterator::new(vec![iter1.clone(), iter2.clone()]);
        let ans = merged_iter.collect::<Vec<_>>();
        assert_eq!(
            ans,
            vec![
                (a.clone(), Bytes::new()),
                (b.clone(), Bytes::new()),
                (c.clone(), c.clone()),
                (d.clone(), d.clone())
            ]
        );

        let merged_iter = MergeIterator::new(vec![iter1.clone(), iter2.clone()]);
        let ans = merged_iter.rev().collect::<Vec<_>>();
        assert_eq!(
            ans,
            vec![
                (d.clone(), d.clone()),
                (c.clone(), c.clone()),
                (b.clone(), Bytes::new()),
                (a.clone(), Bytes::new())
            ]
        );

        let filtered_iter =
            FilterEmptyIter::new(MergeIterator::new(vec![iter1.clone(), iter2.clone()]));
        let ans = filtered_iter.collect::<Vec<_>>();
        assert_eq!(ans, vec![(c.clone(), c.clone()), (d.clone(), d.clone())]);
    }
}

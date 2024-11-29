// TODO: use boolrle to encode the has_effects array
#![allow(dead_code)]
use std::io::{Read, Write};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BoolRleVec {
    /// For the number of the nth item (0-indexed):
    /// - if n is odd, the item is the run length of consecutive `true` values
    /// - if n is even, the item is the run length of consecutive `false` values
    rle_vec: Vec<u32>,
    len: usize,
}

impl BoolRleVec {
    pub fn push(&mut self, value: bool) {
        self.len += 1;
        if self.rle_vec.is_empty() {
            self.rle_vec.push(0);
        }

        let last_index = self.rle_vec.len() - 1;
        let is_last_run_true = last_index % 2 == 1;

        if (value && is_last_run_true) || (!value && !is_last_run_true) {
            // If the new value matches the last run, increment its length
            self.rle_vec[last_index] += 1;
        } else {
            // If the new value doesn't match, start a new run
            self.rle_vec.push(1);
        }
    }

    pub fn pop(&mut self) -> Option<bool> {
        while let Some(last) = self.rle_vec.last() {
            if *last == 0 {
                self.rle_vec.pop();
            } else {
                break;
            }
        }

        if self.rle_vec.is_empty() {
            return None;
        }

        self.len -= 1;
        let last_index = self.rle_vec.len() - 1;
        let is_last_run_true = last_index % 2 == 1;
        self.rle_vec[last_index] -= 1;
        if self.rle_vec[last_index] == 0 {
            self.rle_vec.pop();
        }

        Some(is_last_run_true)
    }

    pub fn drop_last_n(&mut self, n: usize) {
        if n > self.len {
            panic!("Attempted to drop more elements than exist in the vector");
        }

        let mut remaining = n;
        while remaining > 0 {
            if let Some(last) = self.rle_vec.last_mut() {
                if *last <= remaining as u32 {
                    remaining -= *last as usize;
                    self.rle_vec.pop();
                } else {
                    *last -= remaining as u32;
                    remaining = 0;
                }
            } else {
                break;
            }
        }

        self.len -= n;

        // Remove any trailing zero-length runs
        while self.rle_vec.last().map_or(false, |&x| x == 0) {
            self.rle_vec.pop();
        }
    }

    pub fn merge(&mut self, other: &BoolRleVec) {
        if self.is_empty() {
            self.rle_vec = other.rle_vec.clone();
            self.len = other.len;
            return;
        }

        if other.is_empty() {
            return;
        }

        // Align the end of self with the start of other
        let self_last_run_true = self.rle_vec.len() % 2 == 0;
        if self_last_run_true {
            self.rle_vec.extend_from_slice(&other.rle_vec);
        } else {
            *self.rle_vec.last_mut().unwrap() += other.rle_vec[0];
            self.rle_vec.extend_from_slice(&other.rle_vec[1..]);
        }

        self.len += other.len;
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn iter(&self) -> BoolRleVecIter {
        BoolRleVecIter {
            rle_vec: self,
            index: 0,
            offset: 0,
        }
    }

    pub fn new() -> Self {
        Self {
            rle_vec: Default::default(),
            len: 0,
        }
    }

    pub fn encode<W: Write>(&self, writer: &mut W) -> Result<(), std::io::Error> {
        leb128::write::unsigned(writer, self.rle_vec.len() as u64)?;
        for item in &self.rle_vec {
            leb128::write::unsigned(writer, *item as u64)?;
        }

        Ok(())
    }

    pub fn decode<R: Read>(reader: &mut R) -> Result<Self, std::io::Error> {
        let len = leb128::read::unsigned(reader)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?
            as usize;
        let mut rle_vec = Vec::with_capacity(len);
        let mut total_len = 0;
        for _ in 0..len {
            let v = leb128::read::unsigned(reader)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?
                as u32;
            rle_vec.push(v);
            total_len += v;
        }

        Ok(Self {
            rle_vec,
            len: total_len as usize,
        })
    }
}

pub(crate) struct BoolRleVecIter<'a> {
    rle_vec: &'a BoolRleVec,
    index: usize,
    offset: u32,
}

impl Iterator for BoolRleVecIter<'_> {
    type Item = bool;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index >= self.rle_vec.rle_vec.len() {
            return None;
        }

        let run_length = self.rle_vec.rle_vec[self.index];
        if run_length == 0 {
            self.index += 1;
            self.offset = 0;
            return self.next();
        }

        let value = self.index % 2 == 1;
        self.offset += 1;
        if self.offset >= run_length {
            self.index += 1;
            self.offset = 0;
        }

        Some(value)
    }

    fn nth(&mut self, n: usize) -> Option<Self::Item> {
        self.offset += n as u32;
        while self.index < self.rle_vec.rle_vec.len()
            && self.offset >= self.rle_vec.rle_vec[self.index]
        {
            self.offset -= self.rle_vec.rle_vec[self.index];
            self.index += 1;
        }

        self.next()
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_bool_rle_vec() {
        let truth = [true, true, false, false, true, true];
        let mut rle_vec = BoolRleVec::new();
        for t in truth.iter() {
            rle_vec.push(*t);
        }

        let iter = rle_vec.iter();
        for (a, b) in truth.iter().zip(iter) {
            assert_eq!(*a, b);
        }

        let mut encoded = Vec::new();
        rle_vec.encode(&mut encoded).unwrap();
        let decoded = BoolRleVec::decode(&mut encoded.as_slice()).unwrap();
        assert_eq!(rle_vec, decoded);
    }

    #[test]
    fn test_bool_rle_vec_skip() {
        let truth = [true, true, false, false, true, true];
        let mut rle_vec = BoolRleVec::new();
        for t in truth.iter() {
            rle_vec.push(*t);
        }

        let iter = rle_vec.iter();
        for (a, b) in truth.iter().skip(3).zip(iter.skip(3)) {
            assert_eq!(*a, b);
        }
    }

    #[test]
    fn test_bool_rle_vec_empty() {
        let rle_vec = BoolRleVec::new();
        assert_eq!(rle_vec.len(), 0);
        assert!(rle_vec.iter().next().is_none());
    }

    #[test]
    fn test_bool_rle_vec_single_element() {
        let mut rle_vec = BoolRleVec::new();
        rle_vec.push(true);
        assert_eq!(rle_vec.len(), 1);
        assert_eq!(rle_vec.iter().next(), Some(true));
    }

    #[test]
    fn test_bool_rle_vec_alternating() {
        let mut rle_vec = BoolRleVec::new();
        rle_vec.push(true);
        rle_vec.push(false);
        rle_vec.push(true);
        rle_vec.push(false);
        assert_eq!(rle_vec.len(), 4);
        assert_eq!(
            rle_vec.iter().collect::<Vec<_>>(),
            vec![true, false, true, false]
        );
    }

    #[test]
    fn test_bool_rle_vec_long_run() {
        let mut rle_vec = BoolRleVec::new();
        for _ in 0..1000 {
            rle_vec.push(true);
        }
        rle_vec.push(false);
        assert_eq!(rle_vec.len(), 1001);
        assert_eq!(rle_vec.iter().filter(|&x| x).count(), 1000);
        assert_eq!(rle_vec.iter().filter(|&x| !x).count(), 1);
    }

    #[test]
    fn test_bool_rle_vec_nth() {
        let mut rle_vec = BoolRleVec::new();
        rle_vec.push(true);
        rle_vec.push(true);
        rle_vec.push(false);
        rle_vec.push(false);
        rle_vec.push(true);
        let mut iter = rle_vec.iter();
        assert_eq!(iter.nth(2), Some(false));
        assert_eq!(iter.nth(1), Some(true));
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn test_bool_rle_vec_iter_skip() {
        let mut rle_vec = BoolRleVec::new();
        rle_vec.push(true);
        rle_vec.push(true);
        rle_vec.push(false);
        rle_vec.push(false);
        rle_vec.push(true);
        rle_vec.push(false);

        // Test skipping zero elements
        let iter = rle_vec.iter();
        assert_eq!(
            iter.collect::<Vec<_>>(),
            vec![true, true, false, false, true, false]
        );

        // Test skipping some elements
        let iter = rle_vec.iter().skip(2);
        assert_eq!(iter.collect::<Vec<_>>(), vec![false, false, true, false]);

        // Test skipping all elements
        let mut iter = rle_vec.iter().skip(6);
        assert_eq!(iter.next(), None);

        // Test skipping more than available elements
        let mut iter = rle_vec.iter().skip(10);
        assert_eq!(iter.next(), None);

        // Test skipping with a long run
        let mut long_rle_vec = BoolRleVec::new();
        for _ in 0..1000 {
            long_rle_vec.push(true);
        }
        long_rle_vec.push(false);
        long_rle_vec.push(true);

        let iter = long_rle_vec.iter().skip(999);
        assert_eq!(iter.collect::<Vec<_>>(), vec![true, false, true]);
    }

    #[test]
    fn test_bool_rle_vec_merge() {
        // Test merging two empty vectors
        let mut vec1 = BoolRleVec::new();
        let vec2 = BoolRleVec::new();
        vec1.merge(&vec2);
        assert!(vec1.is_empty());

        // Test merging an empty vector with a non-empty one
        let mut vec1 = BoolRleVec::new();
        let mut vec2 = BoolRleVec::new();
        vec2.push(true);
        vec2.push(false);
        vec1.merge(&vec2);
        assert_eq!(vec1.iter().collect::<Vec<_>>(), vec![true, false]);

        // Test merging two non-empty vectors
        let mut vec1 = BoolRleVec::new();
        let mut vec2 = BoolRleVec::new();
        vec1.push(true);
        vec1.push(true);
        vec2.push(false);
        vec2.push(true);
        vec1.merge(&vec2);
        assert_eq!(
            vec1.iter().collect::<Vec<_>>(),
            vec![true, true, false, true]
        );

        // Test merging with alignment (last run of vec1 matches first run of vec2)
        let mut vec1 = BoolRleVec::new();
        let mut vec2 = BoolRleVec::new();
        vec1.push(true);
        vec1.push(false);
        vec2.push(false);
        vec2.push(true);
        vec1.merge(&vec2);
        assert_eq!(
            vec1.iter().collect::<Vec<_>>(),
            vec![true, false, false, true]
        );

        // Test merging with long runs
        let mut vec1 = BoolRleVec::new();
        let mut vec2 = BoolRleVec::new();
        for _ in 0..100 {
            vec1.push(true);
        }
        for _ in 0..100 {
            vec2.push(false);
        }
        vec1.merge(&vec2);
        assert_eq!(vec1.len(), 200);
        assert_eq!(vec1.rle_vec.len(), 3);
        assert_eq!(vec1.iter().take(100).filter(|&x| x).count(), 100);
        assert_eq!(vec1.iter().skip(100).filter(|&x| !x).count(), 100);
    }

    #[test]
    fn test_drop_last_n() {
        // Test dropping from a single run
        let mut vec = BoolRleVec::new();
        for _ in 0..5 {
            vec.push(true);
        }
        vec.drop_last_n(3);
        assert_eq!(vec.iter().collect::<Vec<_>>(), vec![true, true]);

        // Test dropping across multiple runs
        let mut vec = BoolRleVec::new();
        vec.push(true);
        vec.push(true);
        vec.push(false);
        vec.push(false);
        vec.push(true);
        vec.drop_last_n(3);
        assert_eq!(vec.iter().collect::<Vec<_>>(), vec![true, true]);

        // Test dropping entire vector
        let mut vec = BoolRleVec::new();
        vec.push(true);
        vec.push(false);
        vec.drop_last_n(2);
        assert!(vec.is_empty());

        // Test dropping from long runs
        let mut vec = BoolRleVec::new();
        for _ in 0..100 {
            vec.push(true);
        }
        for _ in 0..100 {
            vec.push(false);
        }
        vec.drop_last_n(150);
        assert_eq!(vec.len(), 50);
        assert!(vec.iter().all(|x| x));

        // Test dropping zero elements
        let mut vec = BoolRleVec::new();
        vec.push(true);
        vec.push(false);
        vec.drop_last_n(0);
        assert_eq!(vec.iter().collect::<Vec<_>>(), vec![true, false]);
    }
}

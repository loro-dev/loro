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

impl<'a> Iterator for BoolRleVecIter<'a> {
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
        let iter = rle_vec.iter().skip(0);
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
}

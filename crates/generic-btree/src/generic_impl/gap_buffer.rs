use std::ops::{Range, RangeBounds};

use crate::rle::{CanRemove, HasLength, Mergeable, Sliceable, TryInsert};

#[cfg(not(test))]
pub const MAX_STRING_SIZE: usize = 128;
#[cfg(test)]
pub const MAX_STRING_SIZE: usize = 12;

#[derive(Debug, Clone)]
pub(super) struct GapBuffer {
    buffer: [u8; MAX_STRING_SIZE],
    gap_start: u16,
    gap_len: u16,
}

impl GapBuffer {
    pub fn new() -> Self {
        Self {
            buffer: [0; MAX_STRING_SIZE],
            gap_start: 0,
            gap_len: MAX_STRING_SIZE as u16,
        }
    }

    pub fn shift_at(&mut self, index: usize) {
        if index > self.len() {
            panic!("index {} out of range len={}", index, self.len());
        }

        let gap_start = self.gap_start as usize;
        let gap_end = (self.gap_start + self.gap_len) as usize;
        match index.cmp(&gap_start) {
            std::cmp::Ordering::Equal => {}
            std::cmp::Ordering::Less => {
                let gap_move = gap_start - index;
                self.buffer
                    .copy_within(index..gap_start, gap_end - gap_move);
                self.gap_start -= gap_move as u16;
            }
            std::cmp::Ordering::Greater => {
                let gap_move = index - gap_start;
                let move_end = self.buffer.len().min(gap_end + gap_move);
                self.buffer.copy_within(gap_end..move_end, gap_start);
                self.gap_start += gap_move as u16;
            }
        }
    }

    #[allow(unused)]
    pub fn push(&mut self, value: u8) -> Result<(), ()> {
        if self.gap_len == 0 {
            return Err(());
        }
        self.buffer[self.gap_start as usize] = value;
        self.gap_start += 1;
        self.gap_len -= 1;
        Ok(())
    }

    #[inline(always)]
    pub fn push_bytes(&mut self, bytes: &[u8]) -> Result<(), ()> {
        self.insert_bytes(self.len(), bytes)
    }

    pub fn insert_bytes(&mut self, index: usize, bytes: &[u8]) -> Result<(), ()> {
        if (self.gap_len as usize) < bytes.len() {
            return Err(());
        }

        self.shift_at(index);
        self.buffer[index..index + bytes.len()].copy_from_slice(bytes);
        self.gap_start += bytes.len() as u16;
        self.gap_len -= bytes.len() as u16;
        Ok(())
    }

    pub fn insert_bytes_pair(
        &mut self,
        index: usize,
        (left, right): (&[u8], &[u8]),
    ) -> Result<(), ()> {
        let len = left.len() + right.len();
        if (self.gap_len as usize) < len {
            return Err(());
        }

        self.shift_at(index);
        self.buffer[index..index + left.len()].copy_from_slice(left);
        self.buffer[index + left.len()..index + len].copy_from_slice(right);
        self.gap_start += len as u16;
        self.gap_len -= len as u16;
        Ok(())
    }

    pub fn delete(&mut self, range: impl RangeBounds<usize>) {
        let mut start = match range.start_bound() {
            std::ops::Bound::Included(x) => *x,
            std::ops::Bound::Excluded(x) => x + 1,
            std::ops::Bound::Unbounded => 0,
        };
        let mut end = match range.end_bound() {
            std::ops::Bound::Included(x) => x + 1,
            std::ops::Bound::Excluded(x) => *x,
            std::ops::Bound::Unbounded => self.len(),
        };

        end = end.min(self.len());
        start = start.min(self.len()).min(end);
        if start == end {
            return;
        }

        let len = end - start;
        self.shift_at(end);
        self.gap_start = start as u16;
        self.gap_len += len as u16;
    }

    #[inline]
    pub fn capacity(&self) -> usize {
        self.buffer.len()
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.buffer.len() - self.gap_len as usize
    }

    pub fn as_bytes(&self) -> (&[u8], &[u8]) {
        (
            &self.buffer[..self.gap_start as usize],
            &self.buffer[(self.gap_start + self.gap_len) as usize..],
        )
    }

    #[allow(unused)]
    pub fn to_vec(&self) -> Vec<u8> {
        let mut vec = Vec::with_capacity(self.len());
        let (left, right) = self.as_bytes();
        vec.extend_from_slice(left);
        vec.extend_from_slice(right);
        vec
    }

    pub(crate) fn from_str(elem: &str) -> impl Iterator<Item = GapBuffer> + '_ {
        let mut i = 0;
        let elem = elem.as_bytes();
        std::iter::from_fn(move || {
            if i >= elem.len() {
                return None;
            }

            let mut gb = GapBuffer::new();
            gb.push_bytes(&elem[i..(i + MAX_STRING_SIZE).min(elem.len())])
                .unwrap();
            i += MAX_STRING_SIZE;
            Some(gb)
        })
    }
}

impl HasLength for GapBuffer {
    fn rle_len(&self) -> usize {
        self.len()
    }
}

impl Sliceable for GapBuffer {
    fn _slice(&self, range: Range<usize>) -> Self {
        let mut gb = Self::new();
        let start = range.start;
        let end = range.end;

        let (l, r) = self.as_bytes();
        if start < l.len() {
            gb.push_bytes(&l[start..end.min(l.len())]).unwrap();
        }
        if end > l.len() {
            gb.push_bytes(&r[start.saturating_sub(l.len())..end.saturating_sub(l.len())])
                .unwrap();
        }

        debug_assert_eq!(gb.len(), end - start);
        gb
    }

    fn slice_(&mut self, range: impl RangeBounds<usize>)
    where
        Self: Sized,
    {
        let start = match range.start_bound() {
            std::ops::Bound::Included(x) => *x,
            std::ops::Bound::Excluded(x) => x + 1,
            std::ops::Bound::Unbounded => 0,
        };
        let end = match range.end_bound() {
            std::ops::Bound::Included(x) => x + 1,
            std::ops::Bound::Excluded(x) => *x,
            std::ops::Bound::Unbounded => self.len(),
        };

        self.delete(end..);
        self.delete(..start);
        debug_assert_eq!(self.len(), end - start);
    }

    fn split(&mut self, pos: usize) -> Self
    where
        Self: Sized,
    {
        self.shift_at(pos);
        let right = self.as_bytes().1;
        let mut r = Self::new();
        r.push_bytes(right).unwrap();
        self.gap_len = (self.capacity() - pos) as u16;
        r
    }
}

impl Mergeable for GapBuffer {
    fn can_merge(&self, rhs: &Self) -> bool {
        self.len() + rhs.len() <= MAX_STRING_SIZE
    }

    fn merge_right(&mut self, rhs: &Self) {
        let pair = rhs.as_bytes();
        self.insert_bytes_pair(self.len(), pair).unwrap();
    }

    fn merge_left(&mut self, left: &Self) {
        let pair = left.as_bytes();
        self.insert_bytes_pair(0, pair).unwrap();
    }
}

impl TryInsert for GapBuffer {
    fn try_insert(&mut self, pos: usize, elem: Self) -> Result<(), Self>
    where
        Self: Sized,
    {
        if self.len() + elem.len() > MAX_STRING_SIZE {
            return Err(elem);
        }

        let pair = elem.as_bytes();
        self.insert_bytes_pair(pos, pair).unwrap();
        Ok(())
    }
}

impl CanRemove for GapBuffer {
    fn can_remove(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn basic() {
        let mut gb: GapBuffer = GapBuffer::new();
        gb.insert_bytes(0, &[3, 8]).unwrap();
        assert_eq!(gb.to_vec(), vec![3, 8]);
        gb.insert_bytes(1, &[4, 5, 6]).unwrap();
        assert_eq!(gb.to_vec(), vec![3, 4, 5, 6, 8]);
        assert_eq!(gb.len(), 5);
        gb.insert_bytes(4, &[7]).unwrap();
        assert_eq!(gb.to_vec(), vec![3, 4, 5, 6, 7, 8]);
        gb.insert_bytes(0, &[1, 2, 9, 9]).unwrap();
        assert_eq!(gb.to_vec(), vec![1, 2, 9, 9, 3, 4, 5, 6, 7, 8]);
        gb.delete(2..4);
        assert_eq!(gb.len(), 8);
        let (left, right) = gb.as_bytes();
        assert_eq!(left, &[1, 2]);
        assert_eq!(right, &[3, 4, 5, 6, 7, 8]);
        assert_eq!(gb.to_vec(), vec![1, 2, 3, 4, 5, 6, 7, 8])
    }

    #[test]
    fn slice() {
        let mut gb = GapBuffer::new();
        gb.push_bytes(&[0, 1, 2, 3, 4, 5, 6, 7]).unwrap();
        gb.shift_at(5);
        let b = gb.slice(2..5);
        assert_eq!(b.to_vec(), vec![2, 3, 4]);

        gb.slice_(2..5);
        assert_eq!(gb.to_vec(), vec![2, 3, 4]);
    }
}

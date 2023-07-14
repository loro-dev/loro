#![doc = include_str!("../README.md")]

use append_only_bytes::{AppendOnlyBytes, BytesSlice};
use fxhash::FxHasher32;
use std::{hash::Hasher, num::NonZeroU32, ops::Range};

/// it must be a power of 2
const DEFAULT_CAPACITY: usize = 1 << 16;
const MAX_TRIED: usize = 4;

/// # Memory Usage
///
/// The memory usage is capacity * 12 bytes.
/// The default capacity is 65536 (2^16), so the default memory usage is 0.75MB
///
/// You can set the capacity by calling `with_capacity`. The capacity must be a power of 2.
pub struct CompactBytes {
    bytes: AppendOnlyBytes,
    map: Box<[Option<NonZeroU32>]>,
    pos_and_next: Box<[PosLinkList]>,
    /// next write index fr pos_and_next
    index: usize,
    capacity: usize,
    mask: usize,
}

#[derive(Debug, Default, Clone, Copy)]
struct PosLinkList {
    /// position in the doc + 1
    value: Option<NonZeroU32>,
    /// next pos in the list
    next: Option<NonZeroU32>,
}

impl CompactBytes {
    pub fn new() -> Self {
        CompactBytes {
            bytes: AppendOnlyBytes::new(),
            map: vec![None; DEFAULT_CAPACITY].into_boxed_slice(),
            pos_and_next: vec![Default::default(); DEFAULT_CAPACITY].into_boxed_slice(),
            index: 1,
            capacity: DEFAULT_CAPACITY,
            mask: DEFAULT_CAPACITY - 1,
        }
    }

    /// cap will be adjusted to a power of 2
    pub fn with_capacity(cap: usize) -> Self {
        let cap = cap.max(1024).next_power_of_two();
        CompactBytes {
            bytes: AppendOnlyBytes::with_capacity(cap),
            map: vec![None; cap].into_boxed_slice(),
            pos_and_next: vec![Default::default(); cap].into_boxed_slice(),
            index: 1,
            capacity: cap,
            mask: cap - 1,
        }
    }

    #[inline]
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn take(self) -> AppendOnlyBytes {
        self.bytes
    }

    pub fn from_bytes(bytes: &[u8]) -> Self {
        let mut compact_bytes = CompactBytes::new();
        compact_bytes.append(bytes);
        compact_bytes
    }

    pub fn alloc(&mut self, bytes: &[u8]) -> BytesSlice {
        if let Some((position, length)) = self.lookup(bytes) {
            if length == bytes.len() {
                return self.bytes.slice(position..position + length);
            }
        }
        self.append(bytes)
    }

    #[inline]
    pub fn as_bytes(&self) -> &[u8] {
        self.bytes.as_bytes()
    }

    pub fn alloc_advance(&mut self, bytes: &[u8]) -> Vec<Range<usize>> {
        let mut ans: Vec<Range<usize>> = vec![];
        // this push will try to merge the new range with the last range in the ans
        fn push_with_merge(ans: &mut Vec<Range<usize>>, new: Range<usize>) {
            if let Some(last) = ans.last_mut() {
                if last.end == new.start {
                    last.end = new.end;
                    return;
                }
            }

            ans.push(new);
        }

        let mut index = 0;
        let min_match_size = 4.min(bytes.len());
        while index < bytes.len() {
            match self.lookup(&bytes[index..]) {
                Some((pos, len)) if len >= min_match_size => {
                    push_with_merge(&mut ans, pos..pos + len);
                    index += len;
                }
                _ => {
                    let old_len = self.bytes.len();
                    push_with_merge(&mut ans, self.bytes.len()..self.bytes.len() + 1);
                    self.bytes.push(bytes[index]);
                    self.record_new_prefix(old_len);
                    index += 1;
                }
            }
        }

        ans
    }

    pub fn append(&mut self, bytes: &[u8]) -> BytesSlice {
        let old_len = self.bytes.len();
        self.bytes.push_slice(bytes);
        self.record_new_prefix(old_len);
        self.bytes.slice(old_len..old_len + bytes.len())
    }

    /// Append the entries just created to the map
    fn record_new_prefix(&mut self, old_len: usize) {
        // if old doc = "", append "0123", then we need to add "0123" entry to the map
        // if old doc = "0123", append "x", then we need to add "123x" entry to the map
        // if old doc = "0123", append "xyz", then we need to add "123x", "23xy", "3xyz" entries to the map
        for i in old_len.saturating_sub(3)..self.bytes.len().saturating_sub(3) {
            let key = hash(self.bytes.as_bytes(), i, self.mask);
            // Override the min position in entry with the current position
            let old = self.map[key];
            self.pos_and_next[self.index] = PosLinkList {
                value: Some(unsafe { NonZeroU32::new_unchecked(i as u32 + 1) }),
                next: old,
            };
            self.map[key] = Some(NonZeroU32::new(self.index as u32).unwrap());
            self.index = (self.index + 1) & self.mask;
            if self.index == 0 {
                self.index = 1;
            }
        }
    }

    /// Given bytes, find the position with the longest match in the document
    /// It need exclusive reference to refresh the LRU
    ///
    /// return Option<(position, length)>
    fn lookup(&mut self, bytes: &[u8]) -> Option<(usize, usize)> {
        if bytes.len() < 4 {
            return None;
        }

        let key = hash(bytes, 0, self.mask);
        match self.map[key] {
            Some(pointer) => {
                let mut node = self.pos_and_next[pointer.get() as usize];
                let mut max_len = 0;
                let mut ans_pos = 0;
                let mut tried = 0;
                while let Some(pos) = node.value {
                    let pos = pos.get() as usize - 1;
                    node = node
                        .next
                        .map(|x| self.pos_and_next[x.get() as usize])
                        .unwrap_or_default();

                    let mut len = 0;
                    while pos + len < self.bytes.len()
                        && len < bytes.len()
                        && self.bytes[pos + len] == bytes[len]
                    {
                        len += 1;
                    }

                    if len < 4 {
                        break;
                    }

                    if len > max_len {
                        max_len = len;
                        ans_pos = pos;
                    }

                    tried += 1;
                    if tried > MAX_TRIED {
                        break;
                    }
                }

                Some((ans_pos, max_len))
            }
            None => None,
        }
    }
}

impl Default for CompactBytes {
    fn default() -> Self {
        Self::new()
    }
}

#[inline(always)]
fn hash(bytes: &[u8], n: usize, mask: usize) -> usize {
    let mut hasher = FxHasher32::default();
    hasher.write_u8(bytes[n]);
    hasher.write_u8(bytes[n + 1]);
    hasher.write_u8(bytes[n + 2]);
    hasher.write_u8(bytes[n + 3]);
    hasher.finish() as usize & mask
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {
        let mut bytes = CompactBytes::new();
        let a = bytes.alloc(b"12345");
        let b = bytes.alloc(b"12345");
        assert_eq!(b.start(), 0);
        assert_eq!(b.end(), 5);
        let b = bytes.alloc(b"2345");
        assert_eq!(b.start(), 1);
        assert_eq!(b.end(), 5);
        let b = bytes.alloc(b"23456");
        assert_eq!(b.start(), 5);
        assert_eq!(b.end(), 10);
        assert_eq!(a.as_bytes(), b"12345");
    }

    #[test]
    fn advance() {
        let mut bytes = CompactBytes::new();
        bytes.append(b"123456789");
        let ans = bytes.alloc_advance(b"haha12345567891234");
        assert_eq!(ans.len(), 4);
        assert_eq!(ans[0].len(), 4);
        assert_eq!(ans[0].start, 9);
        assert_eq!(ans[1].len(), 5);
        assert_eq!(ans[1].start, 0);
        assert_eq!(ans[2].len(), 5);
        assert_eq!(ans[2].start, 4);
        assert_eq!(ans[3].len(), 4);
        assert_eq!(ans[3].start, 0);
    }

    #[test]
    fn advance_alloc_should_be_indexed_as_well() {
        let mut bytes = CompactBytes::new();
        bytes.alloc_advance(b"1234");
        let a = bytes.alloc(b"1234");
        assert_eq!(a.start(), 0);
    }

    #[test]
    fn advance_should_use_longer_match() {
        let mut bytes = CompactBytes::new();
        bytes.append(b"1234kk 123456 1234xyz");
        let ans = bytes.alloc_advance(b"012345678");
        assert_eq!(ans.len(), 3);
        assert_eq!(ans[0].len(), 1);
        assert_eq!(ans[1].len(), 6);
        assert_eq!(ans[2].len(), 2);
    }
}

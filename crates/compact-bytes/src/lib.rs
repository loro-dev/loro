#![doc = include_str!("../README.md")]

use std::{
    collections::{HashMap, VecDeque},
    hash::BuildHasherDefault,
    ops::Range,
};

use append_only_bytes::{AppendOnlyBytes, BytesSlice};

const DEFAULT_CAPACITY: usize = 2 * 1024;

type Hasher = BuildHasherDefault<fxhash::FxHasher32>;

/// # Memory Usage
///
/// One entry in the hash table will take 36 bytes. And we need one entry for every position in the document.
/// So the size of the hash table will be (36 ~ 72) * document_size.
///
/// However, you can set the maximum size of the hashtable to reduce the memory usage.
/// It will drop the old entries when the size of the hashtable reaches the maximum size.
///
/// By default the maximum size of the hash table is 2 * 1024, which means the memory usage will be 72 * 2 * 1024 = 144KB.
/// It can fit L2 cache of most CPUs. This behavior is subjected to change in the future as we do more optimization.
///
pub struct CompactBytes {
    bytes: AppendOnlyBytes,
    /// Map 4 bytes to positions in the document.
    /// The actual position is value - 1, and 0 means the position is not found.
    map: HashMap<u32, u32, Hasher>,
    pos_and_next: Vec<PosLinkList>,
    /// Least Recently Used keys
    lru: VecDeque<u32>,
    last_key: u32,
    capacity: usize,
}

struct PosLinkList {
    /// position in the doc
    value: u32,
    /// next pos in the list, it will form a cyclic linked list
    next: u32,
}

impl CompactBytes {
    pub fn new() -> Self {
        CompactBytes {
            bytes: AppendOnlyBytes::new(),
            map: Default::default(),
            lru: Default::default(),
            pos_and_next: Default::default(),
            capacity: DEFAULT_CAPACITY,
            last_key: 0,
        }
    }

    /// Set the maximum size of the hash table
    /// When the size of the hash table reaches the maximum size, it will drop the old entries.
    /// When it's zero, it will never drop the old entries.
    pub fn set_capacity(&mut self, capacity: usize) {
        self.capacity = capacity;
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    fn drop_old_entry_if_reach_maximum_capacity(&mut self) {
        if self.capacity == 0 || self.lru.len() < self.capacity {
            return;
        }

        let target = self.capacity.saturating_sub(16);
        while self.lru.len() > target {
            let key = self.lru.pop_front().unwrap();
            self.map.remove(&key);
        }
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
        while index < bytes.len() {
            match self.lookup(&bytes[index..]) {
                Some((pos, len)) => {
                    push_with_merge(&mut ans, pos..pos + len);
                    index += len;
                }
                None => {
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
        let mut key = self.last_key;
        let mut is_first = true;
        for i in old_len.saturating_sub(3)..self.bytes.len().saturating_sub(3) {
            if is_first {
                key = to_key(&self.bytes[i..i + 4]);
                is_first = false;
            } else {
                key = (key << 8) | self.bytes[i + 3] as u32;
            }

            // Override the min position in entry with the current position
            let value = self.pos_and_next.len() as u32;
            self.pos_and_next.push(PosLinkList {
                value: i as u32,
                next: i as u32,
            });
            let old_value = self.map.insert(key, value);
            if let Some(old_value) = old_value {
                let next = self.pos_and_next[old_value as usize].next;
                self.pos_and_next[old_value as usize].next = value;
                self.pos_and_next[value as usize].next = next;
            }
        }

        self.drop_old_entry_if_reach_maximum_capacity()
    }

    /// Given bytes, find the position with the longest match in the document
    /// It need exclusive reference to refresh the LRU
    ///
    /// return Option<(position, length)>
    fn lookup(&mut self, bytes: &[u8]) -> Option<(usize, usize)> {
        if bytes.len() < 4 {
            return None;
        }

        let key = to_key(bytes);
        match self.map.get(&key).copied() {
            Some(start_pointer) => {
                let mut pointer = start_pointer;
                let mut max_len = 0;
                let mut ans_pos = 0;
                while pointer != start_pointer || max_len == 0 {
                    let pos = self.pos_and_next[pointer as usize].value as usize;
                    pointer = self.pos_and_next[pointer as usize].next;
                    let mut len = 4;
                    while pos + len < self.bytes.len()
                        && len < bytes.len()
                        && self.bytes[pos + len] == bytes[len]
                    {
                        len += 1;
                    }

                    if len > max_len {
                        max_len = len;
                        ans_pos = pos;
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

/// Convert the first 4 bytes into u32
fn to_key(bytes: &[u8]) -> u32 {
    u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
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

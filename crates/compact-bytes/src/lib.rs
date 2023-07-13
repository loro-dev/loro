#![doc = include_str!("../README.md")]

use std::{hash::BuildHasherDefault, ops::Range};

use append_only_bytes::{AppendOnlyBytes, BytesSlice};
use linked_hash_map::LinkedHashMap;

const DEFAULT_CAPACITY: usize = 2 * 1024;
const NUM_POS_PER_ENTRY: usize = 4;

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
    map: LinkedHashMap<u32, [u32; NUM_POS_PER_ENTRY], Hasher>,
    capacity: usize,
}

impl CompactBytes {
    pub fn new() -> Self {
        CompactBytes {
            bytes: AppendOnlyBytes::new(),
            map: LinkedHashMap::with_hasher(Default::default()),
            capacity: DEFAULT_CAPACITY,
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
        if self.capacity == 0 {
            return;
        }

        while self.map.len() > self.capacity {
            self.map.pop_front();
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
        let old_len = self.bytes.len();
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
                    push_with_merge(&mut ans, self.bytes.len()..self.bytes.len() + 1);
                    self.bytes.push(bytes[index]);
                    index += 1;
                }
            }
        }

        self.record_new_prefix(old_len);
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
        let mut key = 0;
        let mut is_first = true;
        for i in old_len.saturating_sub(3)..self.bytes.len().saturating_sub(3) {
            if is_first {
                key = to_key(&self.bytes[i..i + 4]);
                is_first = false;
            } else {
                key = (key << 8) | self.bytes[i + 3] as u32;
            }

            // Override the min position in entry with the current position
            let entry = self.map.entry(key).or_insert([0; NUM_POS_PER_ENTRY]);
            entry
                .iter_mut()
                .min()
                .map(|min| *min = i as u32 + 1)
                .unwrap();
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
        match self.map.get_refresh(&key).copied() {
            Some(poses) => {
                let mut max_len = 0;
                let mut ans_pos = 0;
                for &pos in poses.iter() {
                    if pos == 0 {
                        continue;
                    }

                    let pos = pos as usize - 1;
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

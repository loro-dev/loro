#![doc = include_str!("../README.md")]

use append_only_bytes::{AppendOnlyBytes, BytesSlice};
use fxhash::FxHashMap;

// One entry in the hashtable will take 16bytes. And we need one entry for every position in the document.
// So the size of the hashtable will be 16 * document_size.
pub struct CompactBytes {
    bytes: AppendOnlyBytes,
    /// map 4 bytes to position in the document
    map: FxHashMap<u32, u32>,
}

impl CompactBytes {
    pub fn new() -> Self {
        CompactBytes {
            bytes: AppendOnlyBytes::new(),
            map: FxHashMap::default(),
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

    pub fn alloc_advance(&mut self, bytes: &[u8]) -> Vec<BytesSlice> {
        // ans is Vec<(from_index, to_index)>
        let mut ans: Vec<(usize, usize)> = vec![];

        fn push(ans: &mut Vec<(usize, usize)>, new: (usize, usize)) {
            if let Some(last) = ans.last_mut() {
                if last.1 == new.0 {
                    last.1 = new.1;
                    return;
                }
            }

            ans.push(new);
        }

        let mut index = 0;
        while index < bytes.len() {
            match self.lookup(&bytes[index..]) {
                Some((pos, len)) => {
                    push(&mut ans, (pos, pos + len));
                    index += len;
                }
                None => {
                    push(&mut ans, (self.bytes.len(), self.bytes.len() + 1));
                    self.bytes.push(bytes[index]);
                    index += 1;
                }
            }
        }

        ans.into_iter()
            .map(|(from, to)| self.bytes.slice(from..to))
            .collect()
    }

    pub fn append(&mut self, bytes: &[u8]) -> BytesSlice {
        let old_len = self.bytes.len();
        self.bytes.push_slice(bytes);
        self.append_new_entries_to_map(old_len);
        self.bytes.slice(old_len..old_len + bytes.len())
    }

    /// Append the entries just created to the map
    fn append_new_entries_to_map(&mut self, old_len: usize) {
        // if old doc = "", append "0123", then we need to add "0123" entry to the map
        // if old doc = "0123", append "x", then we need to add "123x" entry to the map
        // if old doc = "0123", append "xyz", then we need to add "123x", "23xy", "3xyz" entries to the map
        for i in old_len.saturating_sub(3)..self.bytes.len().saturating_sub(3) {
            let key = to_key(&self.bytes[i..i + 4]);
            self.map.insert(key, i as u32);
        }
    }

    /// given bytes, find the position with the longest match in the document
    /// return Option<(position, length)>
    fn lookup(&self, bytes: &[u8]) -> Option<(usize, usize)> {
        if bytes.len() < 4 {
            return None;
        }

        let key = to_key(bytes);
        match self.map.get(&key).copied() {
            Some(pos) => {
                let mut pos = pos as usize;
                let mut len = 4;
                while pos + len < self.bytes.len()
                    && len < bytes.len()
                    && self.bytes[pos + len] == bytes[len]
                {
                    len += 1;
                }

                Some((pos, len))
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

/// Convert the first 4 btyes into u32
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
        assert_eq!(ans[0].len(), 4);
        assert_eq!(ans[0].start(), 9);
        assert_eq!(ans[1].len(), 5);
        assert_eq!(ans[1].start(), 0);
        assert_eq!(ans[2].len(), 5);
        assert_eq!(ans[2].start(), 4);
        assert_eq!(ans[3].len(), 4);
        assert_eq!(ans[3].start(), 0);
    }
}

use std::ops::{Bound, Deref, RangeBounds};

use append_only_bytes::{AppendOnlyBytes, BytesSlice};

use crate::container::richtext::richtext_state::unicode_to_utf8_index;
const INDEX_INTERVAL: u32 = 128;

#[derive(Default)]
pub struct StrArena {
    bytes: AppendOnlyBytes,
    unicode_indexes: Vec<Index>,
    len: Index,
}

#[derive(Default, Clone, Copy)]
struct Index {
    bytes: u32,
    utf16: u32,
    unicode: u32,
}

impl StrArena {
    #[inline]
    pub fn new() -> Self {
        Self {
            bytes: AppendOnlyBytes::new(),
            unicode_indexes: Vec::new(),
            len: Index {
                bytes: 0,
                utf16: 0,
                unicode: 0,
            },
        }
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len.bytes == 0
    }

    #[inline]
    pub fn len_bytes(&self) -> usize {
        self.len.bytes as usize
    }

    #[inline]
    pub fn len_utf16(&self) -> usize {
        self.len.utf16 as usize
    }

    #[inline]
    pub fn len_unicode(&self) -> usize {
        self.len.unicode as usize
    }

    pub fn alloc_and_slice(&mut self, s: &str) -> BytesSlice {
        let bytes_len = self.bytes.len();
        self.alloc(s);
        self.bytes.slice(bytes_len..)
    }

    // TODO: PERF handle super long s, it shuold be split into multiple chunks
    pub fn alloc(&mut self, s: &str) {
        let inner = self;
        let mut utf16 = 0;
        let mut unicode_len = 0;
        for c in s.chars() {
            utf16 += c.len_utf16() as u32;
            unicode_len += 1;
        }
        inner.len.bytes += s.len() as u32;
        inner.len.utf16 += utf16;
        inner.len.unicode += unicode_len as u32;
        inner.bytes.push_str(s);
        let cur_len = inner.len;

        let index = &mut inner.unicode_indexes;
        if index.is_empty() {
            index.push(Index {
                bytes: 0,
                utf16: 0,
                unicode: 0,
            });
        }

        let last = index.last().unwrap();
        if cur_len.bytes - last.bytes > INDEX_INTERVAL {
            inner.unicode_indexes.push(cur_len);
        }
    }

    #[inline]
    pub fn slice_by_unicode(&mut self, range: impl RangeBounds<usize>) -> BytesSlice {
        let (start, end) = self.unicode_range_to_utf8_range(range);
        self.bytes.slice(start..end)
    }

    #[inline]
    pub fn slice_str_by_unicode(&mut self, range: impl RangeBounds<usize>) -> &str {
        let (start, end) = self.unicode_range_to_utf8_range(range);
        // SAFETY: we know that the range is valid
        unsafe { std::str::from_utf8_unchecked(&self.bytes[start..end]) }
    }

    fn unicode_range_to_utf8_range(&mut self, range: impl RangeBounds<usize>) -> (usize, usize) {
        let start = match range.start_bound() {
            Bound::Included(&i) => i as u32,
            Bound::Excluded(&i) => unreachable!(),
            Bound::Unbounded => 0,
        };

        let end = match range.end_bound() {
            Bound::Included(&i) => i as u32 + 1,
            Bound::Excluded(&i) => i as u32,
            Bound::Unbounded => self.len.unicode,
        };

        let start = unicode_to_byte_index(&self.unicode_indexes, start, &self.bytes);
        let end = unicode_to_byte_index(&self.unicode_indexes, end, &self.bytes);
        (start, end)
    }

    #[inline]
    pub fn slice_bytes(&self, range: impl RangeBounds<usize>) -> BytesSlice {
        self.bytes.slice(range)
    }
}

fn unicode_to_byte_index(index: &[Index], unicode_index: u32, bytes: &AppendOnlyBytes) -> usize {
    let i = match index.binary_search_by_key(&unicode_index, |x| x.unicode) {
        Ok(i) => i,
        Err(i) => i - 1,
    };

    let index = index[i];
    if index.unicode == unicode_index {
        return index.bytes as usize;
    }

    // SAFETY: we know that the index must be valid, because we record and calculate the valid index
    let s = unsafe { std::str::from_utf8_unchecked(&bytes.deref()[index.bytes as usize..]) };
    unicode_to_utf8_index(s, unicode_index as usize - index.unicode as usize).unwrap()
        + index.bytes as usize
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test() {
        let mut arena = StrArena::new();
        arena.alloc("Hello");
        let slice = arena.slice_by_unicode(0..5);
        assert_eq!(slice.deref(), b"Hello");
        arena.alloc("World");
        let slice = arena.slice_by_unicode(5..10);
        assert_eq!(slice.deref(), b"World");
    }

    #[test]
    fn parse_unicode_correctly() {
        let mut arena = StrArena::new();
        arena.alloc("Hello");
        arena.alloc("World");
        arena.alloc("你好");
        arena.alloc("世界");
        let slice = arena.slice_by_unicode(0..4);
        assert_eq!(slice.deref(), b"Hell");

        let slice = arena.slice_by_unicode(4..8);
        assert_eq!(slice.deref(), b"oWor");

        let slice = arena.slice_by_unicode(8..10);
        assert_eq!(slice.deref(), b"ld");

        let slice = arena.slice_by_unicode(10..12);
        assert_eq!(slice.deref(), "你好".as_bytes());

        let slice = arena.slice_by_unicode(12..14);
        assert_eq!(slice.deref(), "世界".as_bytes());
    }

    #[test]
    fn slice_long_unicode_correctly() {
        let mut arena = StrArena::new();
        let src = "一二34567八九零";
        for s in std::iter::repeat(src).take(100) {
            arena.alloc(s);
        }

        let slice = arena.slice_by_unicode(110..120);
        assert_eq!(slice.deref(), src.as_bytes());
        let slice = arena.slice_by_unicode(111..121);
        assert_eq!(slice.deref(), "二34567八九零一".as_bytes());
    }
}

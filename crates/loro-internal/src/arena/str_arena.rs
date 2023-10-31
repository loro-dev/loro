use std::ops::{Bound, RangeBounds};

use append_only_bytes::{AppendOnlyBytes, BytesSlice};

use crate::container::richtext::richtext_state::unicode_to_utf8_index;
const INDEX_INTERVAL: u32 = 128;

#[derive(Default, Debug)]
pub(crate) struct StrArena {
    bytes: AppendOnlyBytes,
    unicode_indexes: Vec<Index>,
    len: Index,
}

#[derive(Debug, Default, Clone, Copy)]
struct Index {
    bytes: u32,
    utf16: u32,
    unicode: u32,
}

impl StrArena {
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len.bytes == 0
    }

    #[inline]
    #[allow(dead_code)]
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

    pub fn alloc(&mut self, input: &str) {
        let mut utf16 = 0;
        let mut unicode_len = 0;
        let mut last_save_index = 0;
        for (byte_index, c) in input.char_indices() {
            let byte_index = byte_index + c.len_utf8();
            utf16 += c.len_utf16() as u32;
            unicode_len += 1;
            if byte_index - last_save_index > INDEX_INTERVAL as usize {
                self._alloc(&input[last_save_index..byte_index], utf16, unicode_len);
                last_save_index = byte_index;
                utf16 = 0;
                unicode_len = 0;
            }
        }

        if last_save_index != input.len() {
            self._alloc(&input[last_save_index..], utf16, unicode_len);
        }
    }

    fn _alloc(&mut self, input: &str, utf16: u32, unicode_len: i32) {
        let s = input;
        self.len.bytes += s.len() as u32;
        self.len.utf16 += utf16;
        self.len.unicode += unicode_len as u32;
        self.bytes.push_str(s);
        let cur_len = self.len;

        let index = &mut self.unicode_indexes;
        if index.is_empty() {
            index.push(Index {
                bytes: 0,
                utf16: 0,
                unicode: 0,
            });
        }

        let last = index.last().unwrap();
        if cur_len.bytes - last.bytes > INDEX_INTERVAL {
            self.unicode_indexes.push(cur_len);
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
        if self.is_empty() {
            return (0, 0);
        }

        let start = match range.start_bound() {
            Bound::Included(&i) => {
                unicode_to_byte_index(&self.unicode_indexes, i as u32, &self.bytes)
            }
            Bound::Excluded(&_i) => unreachable!(),
            Bound::Unbounded => 0,
        };

        let end = match range.end_bound() {
            Bound::Included(&i) => {
                unicode_to_byte_index(&self.unicode_indexes, i as u32 + 1, &self.bytes)
            }
            Bound::Excluded(&i) => {
                unicode_to_byte_index(&self.unicode_indexes, i as u32, &self.bytes)
            }
            Bound::Unbounded => self.len.bytes as usize,
        };

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
    let s = unsafe { std::str::from_utf8_unchecked(&bytes[index.bytes as usize..]) };
    unicode_to_utf8_index(s, (unicode_index - index.unicode) as usize).unwrap()
        + index.bytes as usize
}

#[cfg(test)]
mod test {
    use std::ops::Deref;

    use super::*;

    #[test]
    fn test() {
        let mut arena = StrArena::default();
        arena.alloc("Hello");
        let slice = arena.slice_by_unicode(0..5);
        assert_eq!(slice.deref(), b"Hello");
        arena.alloc("World");
        let slice = arena.slice_by_unicode(5..10);
        assert_eq!(slice.deref(), b"World");
    }

    #[test]
    fn parse_unicode_correctly() {
        let mut arena = StrArena::default();
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
        let mut arena = StrArena::default();
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

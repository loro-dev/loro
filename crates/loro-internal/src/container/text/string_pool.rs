use std::{fmt, ops::Range, str::Chars};

use append_only_bytes::{AppendOnlyBytes, BytesSlice};
use rle::{HasLength, Mergable, RleVecWithIndex, Sliceable};

use crate::smstring::SmString;

use super::{text_content::SliceRange, unicode::TextLength};

#[derive(Debug, Default)]
pub struct StringPool {
    data: AppendOnlyBytes,
    alive_ranges: RleVecWithIndex<Alive>,
    deleted: usize,
}

#[derive(Debug, Clone)]
pub struct PoolString {
    pub(super) slice: Option<BytesSlice>,
    pub(super) unknown_len: u32,
    pub(super) utf16_length: i32,
}

#[derive(Debug)]
pub enum Alive {
    True(usize),
    False(usize),
}

impl HasLength for Alive {
    fn content_len(&self) -> usize {
        match self {
            Alive::True(u) => *u,
            Alive::False(u) => *u,
        }
    }
}

impl Mergable for Alive {
    fn is_mergable(&self, other: &Self, _conf: &()) -> bool
    where
        Self: Sized,
    {
        matches!(
            (self, other),
            (Alive::True(_), Alive::True(_)) | (Alive::False(_), Alive::False(_))
        )
    }

    fn merge(&mut self, _other: &Self, _conf: &())
    where
        Self: Sized,
    {
        match (self, _other) {
            (Alive::True(u), Alive::True(other_u)) => *u += other_u,
            (Alive::False(u), Alive::False(other_u)) => *u += other_u,
            _ => unreachable!(),
        }
    }
}

impl Sliceable for Alive {
    fn slice(&self, from: usize, to: usize) -> Self {
        match self {
            Alive::True(_) => Alive::True(to - from),
            Alive::False(_) => Alive::False(to - from),
        }
    }
}

impl StringPool {
    #[inline(always)]
    pub fn alloc(&mut self, s: &str) -> PoolString {
        let start = self.data.len();
        self.data.push_slice(s.as_bytes());
        let end = self.data.len();
        self.data.slice(start..end).into()
    }

    #[inline(always)]
    #[allow(unused)]
    pub fn slice(&self, range: &Range<u32>) -> &str {
        // SAFETY: we are sure the range is valid utf8
        unsafe {
            std::str::from_utf8_unchecked(&self.data[range.start as usize..range.end as usize])
        }
    }

    pub fn get_string(&self, range: &Range<u32>) -> SmString {
        let mut ans = SmString::default();
        ans.push_str(
            std::str::from_utf8(&self.data[range.start as usize..range.end as usize]).unwrap(),
        );

        ans
    }

    pub fn get_aliveness(&self, range: &Range<u32>) -> Vec<Alive> {
        if self.alive_ranges.is_empty() {
            return vec![Alive::True((range.end - range.start) as usize)];
        }

        let mut len = 0;
        let mut ans: Vec<Alive> = self
            .alive_ranges
            .slice_iter(range.start as usize, range.end as usize)
            .map(|x| {
                len += x.end - x.start;
                x.value.slice(x.start, x.end)
            })
            .collect();

        if len < (range.end - range.start) as usize {
            ans.push(Alive::True((range.end - range.start) as usize - len));
        }

        ans
    }

    pub fn update_aliveness<T>(&mut self, iter: T)
    where
        T: Iterator<Item = Range<u32>>,
    {
        let mut alive_ranges = RleVecWithIndex::new();
        let mut last = 0;
        let mut deleted = 0;
        let mut data: Vec<Range<u32>> = iter.filter(|x| x.atom_len() > 0).collect();
        data.sort_by_key(|x| x.start);
        for range in data {
            if range.start > last {
                let len = (range.start - last) as usize;
                deleted += len;
                alive_ranges.push(Alive::False(len));
            }
            alive_ranges.push(Alive::True((range.end - range.start) as usize));
            last = range.end;
        }
        if last < self.data.len() as u32 {
            let len = (self.data.len() as u32 - last) as usize;
            alive_ranges.push(Alive::True(len));
        }
        self.alive_ranges = alive_ranges;
        self.deleted = deleted;
    }

    pub fn should_update_aliveness(&self, current_state_len: usize) -> bool {
        self.data.len() - self.deleted > current_state_len / 3 * 4
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.data.len()
    }

    #[inline]
    pub fn as_bytes(&self) -> &[u8] {
        self.data.as_bytes()
    }

    #[inline]
    pub(crate) fn from_data(bytes: AppendOnlyBytes) -> Self {
        Self {
            data: bytes,
            ..Default::default()
        }
    }
}

impl HasLength for PoolString {
    #[inline(always)]
    fn content_len(&self) -> usize {
        self.slice
            .as_ref()
            .map_or(self.unknown_len as usize, |x| x.atom_len())
    }
}

impl Mergable for PoolString {
    fn is_mergable(&self, other: &Self, _: &()) -> bool
    where
        Self: Sized,
    {
        match (&self.slice, &other.slice) {
            (None, None) => true,
            (Some(a), Some(b)) => a.can_merge(b),
            _ => false,
        }
    }

    fn merge(&mut self, other: &Self, _: &())
    where
        Self: Sized,
    {
        match &mut self.slice {
            Some(a) => {
                let b = other.slice.as_ref().unwrap();
                a.merge(b, &());
                self.utf16_length += other.utf16_length;
            }
            None => {
                self.unknown_len += other.unknown_len;
            }
        }
    }
}

impl Sliceable for PoolString {
    fn slice(&self, from: usize, to: usize) -> Self {
        match self.slice.as_ref() {
            Some(bytes) => {
                let bytes = bytes.slice(from, to);
                // SAFETY: we are sure it's valid utf-8 str
                let utf16 = get_utf16_len(&bytes);
                Self {
                    utf16_length: utf16 as i32,
                    slice: Some(bytes),
                    unknown_len: 0,
                }
            }
            None => Self::new_unknown(to - from),
        }
    }
}

#[inline(always)]
fn get_utf16_len(bytes: &BytesSlice) -> usize {
    let str = bytes_to_str(bytes);
    let utf16 = encode_utf16(str).count();
    utf16
}

#[inline(always)]
fn bytes_to_str(bytes: &BytesSlice) -> &str {
    // SAFETY: we are sure the range is valid utf8
    let str = unsafe { std::str::from_utf8_unchecked(&bytes[..]) };
    str
}

impl From<BytesSlice> for PoolString {
    #[inline(always)]
    fn from(slice: BytesSlice) -> Self {
        Self {
            utf16_length: get_utf16_len(&slice) as i32,
            slice: Some(slice),
            unknown_len: 0,
        }
    }
}

impl PoolString {
    pub fn as_str_unchecked(&self) -> &str {
        let slice = self.slice.as_ref().unwrap();
        bytes_to_str(slice)
    }

    pub fn new_unknown(len: usize) -> Self {
        Self {
            slice: None,
            unknown_len: len as u32,
            utf16_length: 0,
        }
    }

    pub fn is_unknown(&self) -> bool {
        self.slice.is_none()
    }

    pub fn from_slice_range(pool: &StringPool, range: SliceRange) -> Self {
        if range.is_unknown() {
            Self {
                utf16_length: 0,
                slice: None,
                unknown_len: range.atom_len() as u32,
            }
        } else {
            let slice = pool
                .data
                .slice(range.0.start as usize..range.0.end as usize);
            slice.into()
        }
    }

    pub fn text_len(&self) -> TextLength {
        TextLength {
            utf8: self.content_len() as i32,
            utf16: if self.unknown_len > 0 {
                0
            } else {
                self.utf16_length
            },
            unknown_elem_len: self.is_unknown() as i32,
        }
    }

    pub fn utf16_index_to_utf8(&self, end: usize) -> usize {
        let str = bytes_to_str(self.slice.as_ref().unwrap());
        utf16_index_to_utf8(str, end)
    }

    pub fn utf8_index_to_utf16(&self, end: usize) -> usize {
        let str = bytes_to_str(self.slice.as_ref().unwrap());
        encode_utf16(&str[..end]).count()
    }
}

#[inline(always)]
fn utf16_index_to_utf8(str: &str, end: usize) -> usize {
    let len = str.len();
    let mut iter = encode_utf16(str);
    for _ in 0..end {
        iter.next();
    }
    len - iter.chars.as_str().len()
}

fn encode_utf16(s: &str) -> EncodeUtf16 {
    EncodeUtf16 {
        chars: s.chars(),
        extra: 0,
    }
}

// from std
#[derive(Clone)]
pub struct EncodeUtf16<'a> {
    pub(super) chars: Chars<'a>,
    pub(super) extra: u16,
}

impl fmt::Debug for EncodeUtf16<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EncodeUtf16").finish_non_exhaustive()
    }
}

impl<'a> Iterator for EncodeUtf16<'a> {
    type Item = u16;

    #[inline]
    fn next(&mut self) -> Option<u16> {
        if self.extra != 0 {
            let tmp = self.extra;
            self.extra = 0;
            return Some(tmp);
        }

        let mut buf = [0; 2];
        self.chars.next().map(|ch| {
            let n = ch.encode_utf16(&mut buf).len();
            if n == 2 {
                self.extra = buf[1];
            }
            buf[0]
        })
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        let (low, high) = self.chars.size_hint();
        // every char gets either one u16 or two u16,
        // so this iterator is between 1 or 2 times as
        // long as the underlying iterator.
        (low, high.and_then(|n| n.checked_mul(2)))
    }
}

#[cfg(test)]
mod test {
    use super::{encode_utf16, utf16_index_to_utf8};

    #[test]
    fn utf16_convert() {
        assert_eq!(utf16_index_to_utf8("你aaaaa", 4), 6);
        assert_eq!(utf16_index_to_utf8("你好aaaa", 4), 8);
        assert_eq!(utf16_index_to_utf8("你好aaaa", 6), 10);
        assert_eq!("你好".len(), 6);
        assert_eq!(encode_utf16("你好").count(), 2);
        assert_eq!(encode_utf16("ab").count(), 2);
    }
}

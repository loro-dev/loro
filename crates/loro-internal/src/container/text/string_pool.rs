use std::ops::Range;

use append_only_bytes::{AppendOnlyBytes, BytesSlice};
use rle::{HasLength, Mergable, RleVecWithIndex, Sliceable};

use crate::smstring::SmString;

use super::{text_content::SliceRange, unicode::TextLength, utf16::count_utf16_chars};

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
                let utf16 = count_utf16_chars(&bytes);
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
fn bytes_to_str(bytes: &BytesSlice) -> &str {
    // SAFETY: we are sure the range is valid utf8
    let str = unsafe { std::str::from_utf8_unchecked(&bytes[..]) };
    str
}

impl From<BytesSlice> for PoolString {
    #[inline(always)]
    fn from(slice: BytesSlice) -> Self {
        Self {
            utf16_length: count_utf16_chars(&slice) as i32,
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
        let b = self.slice.as_ref().unwrap();
        utf16_index_to_utf8(b, end)
    }

    pub fn utf8_index_to_utf16(&self, end: usize) -> Option<usize> {
        let slice = self.slice.as_ref()?;
        Some(count_utf16_chars(&slice[..end]))
    }
}

#[inline(always)]
fn utf16_index_to_utf8(str: &[u8], end: usize) -> usize {
    let mut utf8_index = 0;
    let mut utf16_count = 0;

    let mut iter = str.iter().cloned();

    while let Some(byte) = iter.next() {
        if utf16_count >= end {
            break;
        }

        utf8_index += 1;
        if byte & 0b1000_0000 == 0 {
            utf16_count += 1;
        } else if byte & 0b1110_0000 == 0b1100_0000 {
            let _ = iter.next();

            utf16_count += 1;
            utf8_index += 1;
        } else if byte & 0b1111_0000 == 0b1110_0000 {
            let _ = iter.next();
            let _ = iter.next();

            utf16_count += 1;
            utf8_index += 2;
        } else if byte & 0b1111_1000 == 0b1111_0000 {
            let u = ((byte & 0b0000_0111) as u32) << 18
                | ((iter.next().unwrap_or(0) & 0b0011_1111) as u32) << 12
                | ((iter.next().unwrap_or(0) & 0b0011_1111) as u32) << 6
                | ((iter.next().unwrap_or(0) & 0b0011_1111) as u32);

            utf8_index += 3;
            if u >= 0x10000 {
                utf16_count += 2;
            } else {
                utf16_count += 1;
            }
        } else {
            unreachable!()
        }
    }

    utf8_index
}

#[cfg(test)]
mod test {
    use crate::container::text::utf16::count_utf16_chars;

    use super::utf16_index_to_utf8;

    #[test]
    fn utf16_convert() {
        assert_eq!(utf16_index_to_utf8("你aaaaa".as_bytes(), 4), 6);
        assert_eq!(utf16_index_to_utf8("你好aaaa".as_bytes(), 4), 8);
        assert_eq!(utf16_index_to_utf8("你好aaaa".as_bytes(), 6), 10);
        assert_eq!("你好".len(), 6);
        assert_eq!(count_utf16_chars("你好".as_bytes()), 2);
        assert_eq!(count_utf16_chars("ab".as_bytes()), 2);
    }
}

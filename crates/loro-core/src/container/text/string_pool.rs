use std::{cell::RefCell, fmt, ops::Range, rc::Rc, str::Chars};

use rle::{HasLength, Mergable, RleVecWithIndex, Sliceable};

use crate::smstring::SmString;

use super::{text_content::SliceRange, unicode::TextLength};

#[derive(Debug, Default)]
pub struct StringPool {
    data: Vec<u8>,
    alive_ranges: RleVecWithIndex<Alive>,
    deleted: usize,
}

#[derive(Debug, Clone)]
pub struct PoolString {
    pub(super) pool: Rc<RefCell<StringPool>>,
    pub(super) range: SliceRange,
    pub(super) utf16_length: Option<u32>,
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
    pub fn alloc(&mut self, s: &str) -> Range<u32> {
        let ans = self.data.len() as u32..self.data.len() as u32 + s.len() as u32;
        self.data.extend_from_slice(s.as_bytes());
        ans
    }

    #[inline(always)]
    #[allow(unused)]
    pub fn slice(&self, range: &Range<u32>) -> &str {
        std::str::from_utf8(&self.data[range.start as usize..range.end as usize]).unwrap()
    }

    pub fn alloc_pool_string(this: &Rc<RefCell<Self>>, s: &str) -> PoolString {
        let mut pool = this.borrow_mut();
        let range = SliceRange(pool.alloc(s));
        PoolString {
            pool: Rc::clone(this),
            range,
            utf16_length: Some(encode_utf16(s).count() as u32),
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
        let mut data: Vec<Range<u32>> = iter.collect();
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
}

impl HasLength for PoolString {
    fn content_len(&self) -> usize {
        self.range.atom_len()
    }
}

impl Mergable for PoolString {
    fn is_mergable(&self, other: &Self, conf: &()) -> bool
    where
        Self: Sized,
    {
        self.range.is_mergable(&other.range, conf)
    }

    fn merge(&mut self, other: &Self, conf: &())
    where
        Self: Sized,
    {
        self.range.merge(&other.range, conf);
        if let (Some(u), Some(other_u)) = (self.utf16_length, other.utf16_length) {
            self.utf16_length = Some(u + other_u);
        } else {
            self.utf16_length = None;
        }
    }
}

impl Sliceable for PoolString {
    fn slice(&self, from: usize, to: usize) -> Self {
        let range = self.range.slice(from, to);
        if range.is_unknown() {
            Self {
                pool: Rc::clone(&self.pool),
                range,
                utf16_length: None,
            }
        } else {
            let borrow = self.pool.borrow();
            let str = borrow.slice(&range.0);
            let utf16_length = encode_utf16(str).count();
            Self {
                pool: Rc::clone(&self.pool),
                range,
                utf16_length: Some(utf16_length as u32),
            }
        }
    }
}

impl PoolString {
    pub fn from_slice(pool: &Rc<RefCell<StringPool>>, slice: SliceRange) -> Self {
        Self {
            pool: Rc::clone(pool),
            utf16_length: if slice.is_unknown() {
                None
            } else {
                let borrow = pool.borrow();
                let str = borrow.slice(&slice.0);
                let utf16_length = encode_utf16(str).count();
                Some(utf16_length as u32)
            },
            range: slice,
        }
    }

    pub fn text_len(&self) -> TextLength {
        TextLength {
            utf8: self.range.atom_len() as u32,
            utf16: self.utf16_length,
        }
    }

    pub fn utf16_index_to_utf8(&self, end: usize) -> usize {
        let borrow = self.pool.borrow();
        let str = borrow.slice(&self.range.0);
        utf16_index_to_utf8(str, end)
    }

    pub fn utf8_index_to_utf16(&self, end: usize) -> usize {
        let borrow = self.pool.borrow();
        let str = borrow.slice(&self.range.0);
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

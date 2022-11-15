use std::ops::Range;

use rle::{HasLength, Mergable, RleVecWithIndex, Sliceable};

use crate::smstring::SmString;

#[derive(Debug, Default)]
pub struct StringPool {
    data: Vec<u8>,
    alive_ranges: RleVecWithIndex<Alive>,
    deleted: usize,
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
    pub fn slice(&self, range: Range<u32>) -> &str {
        std::str::from_utf8(&self.data[range.start as usize..range.end as usize]).unwrap()
    }

    pub fn get_str(&self, range: &Range<u32>) -> SmString {
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

        self.alive_ranges
            .slice_iter(range.start as usize, range.end as usize)
            .map(|x| x.value.slice(x.start, x.end))
            .collect()
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
        println!(
            "data len={} size={}",
            data.len(),
            data.iter().map(|x| x.len()).sum::<usize>()
        );
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

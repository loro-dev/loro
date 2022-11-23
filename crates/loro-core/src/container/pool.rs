use std::ops::{Index, Range};

use crate::value::LoroValue;

#[derive(Debug, Default)]
pub(crate) struct Pool(Vec<LoroValue>);

impl Pool {
    #[inline(always)]
    pub fn alloc<V: Into<LoroValue>>(&mut self, s: V) -> Range<u32> {
        self.0.push(s.into());
        (self.0.len() - 1) as u32..self.0.len() as u32
    }

    #[inline(always)]
    pub fn alloc_arr(&mut self, values: Vec<LoroValue>) -> Range<u32> {
        let start = self.0.len() as u32;
        for v in values {
            self.0.push(v);
        }
        start..self.0.len() as u32
    }

    #[inline(always)]
    pub fn slice(&self, range: &Range<u32>) -> &[LoroValue] {
        &self.0[range.start as usize..range.end as usize]
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.0.len()
    }
}

impl Index<u32> for Pool {
    type Output = LoroValue;

    fn index(&self, index: u32) -> &Self::Output {
        &self.0[index as usize]
    }
}

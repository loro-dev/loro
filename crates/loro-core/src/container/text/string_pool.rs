use std::ops::Range;

use crate::smstring::SmString;

#[derive(Debug, Default)]
pub struct StringPool(Vec<u8>);

impl StringPool {
    #[inline(always)]
    pub fn alloc(&mut self, s: &str) -> Range<u32> {
        let ans = self.0.len() as u32..self.0.len() as u32 + s.len() as u32;
        self.0.extend_from_slice(s.as_bytes());
        ans
    }

    #[inline(always)]
    pub fn slice(&self, range: Range<u32>) -> &str {
        std::str::from_utf8(&self.0[range.start as usize..range.end as usize]).unwrap()
    }

    pub fn get_str(&self, range: &Range<u32>) -> SmString {
        let mut ans = SmString::default();
        ans.push_str(
            std::str::from_utf8(&self.0[range.start as usize..range.end as usize]).unwrap(),
        );

        ans
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.0.len()
    }
}

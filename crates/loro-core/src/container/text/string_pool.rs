use std::ops::Range;

use crate::smstring::SmString;

#[derive(Debug, Default)]
pub struct StringPool(String);

impl StringPool {
    #[inline(always)]
    pub fn alloc(&mut self, s: &str) -> Range<usize> {
        let ans = self.0.len()..self.0.len() + s.len();
        self.0.push_str(s);
        ans
    }

    #[inline(always)]
    pub fn slice(&self, range: Range<usize>) -> &str {
        &self.0[range]
    }

    pub fn get_str(&self, ranges: &[Range<usize>]) -> SmString {
        let mut ans = SmString::default();
        for range in ranges {
            ans.push_str(&self.0[range.clone()]);
        }

        ans
    }
}

use std::ops::Range;

use crate::smstring::SmString;

#[derive(Debug, Default)]
pub struct StringPool(Vec<u8>);

impl StringPool {
    #[inline(always)]
    pub fn alloc(&mut self, s: &str) -> Range<usize> {
        let ans = self.0.len()..self.0.len() + s.len();
        self.0.extend_from_slice(s.as_bytes());
        ans
    }

    #[inline(always)]
    pub fn slice(&self, range: Range<usize>) -> &str {
        std::str::from_utf8(&self.0[range]).unwrap()
    }

    pub fn get_str(&self, range: &Range<usize>) -> SmString {
        let mut ans = SmString::default();
        ans.push_str(std::str::from_utf8(&self.0[range.clone()]).unwrap());

        ans
    }
}

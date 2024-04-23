use generic_btree::rle::{HasLength, Mergeable, Sliceable, TryInsert};

use crate::delta_trait::DeltaValue;

#[derive(Debug, Clone)]
pub struct AsUtf16Index<S: AsRef<str>> {
    s: S,
    utf16_len: usize,
}

impl<S: AsRef<str>> AsUtf16Index<S> {
    pub fn new(s: S) -> Self {
        let utf16_len = s
            .as_ref()
            .chars()
            .fold(0, |prev, cur| prev + cur.len_utf16());
        Self { s, utf16_len }
    }

    fn convert_utf16_to_utf8(&self, index: usize) -> usize {
        if index == 0 {
            return 0;
        }

        let s = self.s.as_ref();
        if index == self.utf16_len {
            return s.len();
        }

        if index > self.utf16_len {
            panic!("Index out of bounds");
        }

        let mut utf16_index = 0;
        for (i, c) in s.char_indices() {
            if utf16_index >= index {
                return i;
            }
            utf16_index += c.len_utf16();
        }

        unreachable!();
    }

    #[allow(unused)]
    fn convert_utf8_to_utf16(&self, index: usize) -> usize {
        if index == 0 {
            return 0;
        }

        let s = self.s.as_ref();
        if index == s.len() {
            return self.utf16_len;
        }

        if index > s.len() {
            panic!("Index out of bounds");
        }

        let mut utf16_index = 0;
        for (i, c) in s.char_indices() {
            if i >= index {
                return utf16_index;
            }
            utf16_index += c.len_utf16();
        }

        unreachable!();
    }
}

impl<S: AsRef<str>> HasLength for AsUtf16Index<S> {
    fn rle_len(&self) -> usize {
        self.utf16_len
    }
}

impl<S: AsRef<str> + Mergeable> Mergeable for AsUtf16Index<S> {
    fn can_merge(&self, rhs: &Self) -> bool {
        self.s.can_merge(&rhs.s)
    }

    fn merge_right(&mut self, rhs: &Self) {
        self.s.merge_right(&rhs.s);
        self.utf16_len += rhs.utf16_len;
    }

    fn merge_left(&mut self, left: &Self) {
        self.s.merge_left(&left.s);
        self.utf16_len += left.utf16_len;
    }
}

impl<S: AsRef<str> + Sliceable> Sliceable for AsUtf16Index<S> {
    fn _slice(&self, range: std::ops::Range<usize>) -> Self {
        let start = self.convert_utf16_to_utf8(range.start);
        let end = self.convert_utf16_to_utf8(range.end);
        Self {
            s: self.s._slice(start..end),
            utf16_len: range.len(),
        }
    }
}

impl<S: AsRef<str> + Sliceable + TryInsert> TryInsert for AsUtf16Index<S> {
    fn try_insert(&mut self, pos: usize, elem: Self) -> Result<(), Self>
    where
        Self: Sized,
    {
        let start = self.convert_utf16_to_utf8(pos);
        match self.s.try_insert(start, elem.s) {
            Ok(()) => {
                self.utf16_len += elem.utf16_len;
                Ok(())
            }
            Err(e) => Err(Self {
                s: e,
                utf16_len: elem.utf16_len,
            }),
        }
    }
}

impl<S: AsRef<str> + DeltaValue> Default for AsUtf16Index<S> {
    fn default() -> Self {
        Self {
            s: S::default(),
            utf16_len: 0,
        }
    }
}

impl<S: AsRef<str> + DeltaValue> DeltaValue for AsUtf16Index<S> {}

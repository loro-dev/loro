use crate::{delta_trait::DeltaValue, DeltaItem, DeltaRope};
use arrayvec::ArrayString;
use generic_btree::rle::{HasLength, Mergeable, Sliceable};

#[cfg(test)]
const MAX_STRING_SIZE: usize = 8;
#[cfg(not(test))]
const MAX_STRING_SIZE: usize = 128;

#[derive(Debug, Clone)]
pub struct Chunk(ArrayString<MAX_STRING_SIZE>);
pub type TextDelta = DeltaRope<Chunk, ()>;

impl TextDelta {
    pub fn insert_str(&mut self, index: usize, s: &str) {
        if s.is_empty() || index == self.len() {
            self.push_str_insert(s);
            return;
        }

        if s.len() > MAX_STRING_SIZE {
            unimplemented!();
        }

        self.insert_value(
            index,
            &[DeltaItem::Insert {
                value: Chunk(ArrayString::from(s).unwrap()),
                attr: (),
            }],
        );
    }

    pub fn push_str_insert(&mut self, s: &str) -> &mut Self {
        if s.is_empty() {
            return self;
        }

        if s.len() <= MAX_STRING_SIZE {
            self.insert(Chunk(ArrayString::from(s).unwrap()), ());
            return self;
        }

        let mut split_end = 128;
        let mut split_start = 0;
        while split_end != s.len() {
            while !s.is_char_boundary(split_end) {
                split_end -= 1;
            }

            let chunk = Chunk(ArrayString::from(&s[split_start..split_end]).unwrap());
            self.insert(chunk, ());
            split_start = split_end;
            split_end = (split_end + 128).min(s.len());
        }

        self
    }

    pub fn try_to_string(&self) -> Option<String> {
        let mut ans = String::with_capacity(self.len());
        for item in self.iter() {
            match item {
                crate::DeltaItem::Delete(_) => return None,
                crate::DeltaItem::Retain { .. } => return None,
                crate::DeltaItem::Insert { value, .. } => {
                    ans.push_str(&value.0);
                }
            }
        }

        Some(ans)
    }
}

impl HasLength for Chunk {
    fn rle_len(&self) -> usize {
        self.0.len()
    }
}

impl Mergeable for Chunk {
    fn can_merge(&self, rhs: &Self) -> bool {
        MAX_STRING_SIZE >= self.0.len() + rhs.0.len()
    }

    fn merge_right(&mut self, rhs: &Self) {
        self.0.push_str(&rhs.0)
    }

    fn merge_left(&mut self, left: &Self) {
        let ptr = self.0.as_mut_ptr();
        // Safety: `self.0` is a valid `ArrayString` and `left.0` is a valid `ArrayString`.
        unsafe {
            ptr.copy_to(ptr.add(left.0.len()), self.0.len());
            ptr.copy_from_nonoverlapping(left.0.as_ptr(), left.0.len());
            self.0.set_len(self.0.len() + left.0.len());
        }
    }
}

impl Sliceable for Chunk {
    fn _slice(&self, range: std::ops::Range<usize>) -> Self {
        let mut new = ArrayString::new();
        new.push_str(&self.0.as_str()[range]);
        Chunk(new)
    }

    fn split(&mut self, pos: usize) -> Self {
        let mut right = ArrayString::new();
        right.push_str(&self.0.as_str()[pos..]);
        self.0.truncate(pos);
        Chunk(right)
    }
}

impl DeltaValue for Chunk {}

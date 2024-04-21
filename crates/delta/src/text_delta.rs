use crate::{
    delta_trait::{DeltaAttr, DeltaValue},
    DeltaItem, DeltaRope,
};
use arrayvec::ArrayString;
use generic_btree::rle::{HasLength, Mergeable, Sliceable, TryInsert};

#[cfg(test)]
const MAX_STRING_SIZE: usize = 8;
#[cfg(not(test))]
const MAX_STRING_SIZE: usize = 128;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chunk(ArrayString<MAX_STRING_SIZE>);
pub type TextDelta<Attr = ()> = DeltaRope<Chunk, Attr>;

impl<Attr: DeltaAttr> TextDelta<Attr> {
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
                attr: Attr::default(),
            }],
        );
    }

    pub fn push_str_insert(&mut self, s: &str) -> &mut Self {
        if s.is_empty() {
            return self;
        }

        if s.len() <= MAX_STRING_SIZE {
            self.push_insert(Chunk(ArrayString::from(s).unwrap()), Attr::default());
            return self;
        }

        let mut split_end = 128;
        let mut split_start = 0;
        while split_end != s.len() {
            while !s.is_char_boundary(split_end) {
                split_end -= 1;
            }

            let chunk = Chunk(ArrayString::from(&s[split_start..split_end]).unwrap());
            self.push_insert(chunk, Attr::default());
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

impl Chunk {
    pub(crate) fn try_insert(&mut self, pos: usize, s: &str) -> Result<(), ()> {
        if self.0.len() + s.len() > MAX_STRING_SIZE {
            return Err(());
        }

        assert!(self.0.is_char_boundary(pos));
        let new_len = self.0.len() + s.len();
        unsafe {
            let ptr = self.0.as_mut_ptr().add(pos);
            ptr.copy_to(ptr.add(s.len()), self.0.len() - pos);
            ptr.copy_from_nonoverlapping(s.as_ptr(), s.len());
            self.0.set_len(new_len);
        }

        Ok(())
    }

    pub fn try_from_str(s: &str) -> Option<Self> {
        Some(Chunk(ArrayString::from(s).ok()?))
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

impl TryInsert for Chunk {
    fn try_insert(&mut self, pos: usize, elem: Self) -> Result<(), Self>
    where
        Self: Sized,
    {
        match self.try_insert(pos, elem.0.as_str()) {
            Ok(_) => Ok(()),
            Err(_) => Err(elem),
        }
    }
}

impl DeltaValue for Chunk {}

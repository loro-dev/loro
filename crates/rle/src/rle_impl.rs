use std::ops::Range;

use crate::{HasLength, Mergable, Sliceable};
use num::{cast, Integer, NumCast};
use smallvec::{Array, SmallVec};

impl Sliceable for bool {
    fn slice(&self, _: usize, _: usize) -> Self {
        *self
    }
}

impl<T: Integer + NumCast + Copy> Sliceable for Range<T> {
    fn slice(&self, start: usize, end: usize) -> Self {
        self.start + cast(start).unwrap()..self.start + cast(end).unwrap()
    }
}

impl<T: PartialOrd<T> + Copy> Mergable for Range<T> {
    fn is_mergable(&self, other: &Self, _: &()) -> bool {
        other.start <= self.end && other.start >= self.start
    }

    fn merge(&mut self, other: &Self, _conf: &())
    where
        Self: Sized,
    {
        self.end = other.end;
    }
}

impl<T: num::Integer + NumCast + Copy> HasLength for Range<T> {
    fn content_len(&self) -> usize {
        cast(self.end - self.start).unwrap()
    }
}

/// this can make iter return type has len
impl<A, T: HasLength> HasLength for (A, T) {
    fn content_len(&self) -> usize {
        self.1.content_len()
    }
}

/// this can make iter return type has len
impl<T: HasLength> HasLength for &T {
    fn content_len(&self) -> usize {
        (*self).content_len()
    }
}

impl<T: HasLength + Sliceable, A: Array<Item = T>> Sliceable for SmallVec<A> {
    fn slice(&self, from: usize, to: usize) -> Self {
        let mut index = 0;
        let mut ans: SmallVec<A> = smallvec::smallvec![];
        if to == from {
            return ans;
        }

        for item in self.iter() {
            if index < to && from < index + item.atom_len() {
                let start = if index < from { from - index } else { 0 };
                ans.push(item.slice(start, item.atom_len().min(to - index)));
            }

            index += item.atom_len();
        }

        ans
    }
}

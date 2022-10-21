use std::{fmt::Debug, ops::Range};

use num::{cast, Integer, NumCast};
use smallvec::{Array, SmallVec};

pub trait Mergable<Cfg = ()> {
    fn is_mergable(&self, _other: &Self, _conf: &Cfg) -> bool
    where
        Self: Sized,
    {
        false
    }

    fn merge(&mut self, _other: &Self, _conf: &Cfg)
    where
        Self: Sized,
    {
        unreachable!()
    }
}

pub trait Sliceable {
    fn slice(&self, from: usize, to: usize) -> Self;
}

#[derive(Debug, Clone, Copy)]
pub struct Slice<'a, T> {
    pub value: &'a T,
    pub start: usize,
    pub end: usize,
}

impl<T: Sliceable> Slice<'_, T> {
    pub fn into_inner(&self) -> T {
        self.value.slice(self.start, self.end)
    }
}

#[allow(clippy::len_without_is_empty)]
pub trait HasLength {
    /// if the content is deleted, len should be zero
    fn len(&self) -> usize;

    /// the actual length of the value, cannot be affected by delete state
    fn content_len(&self) -> usize {
        self.len()
    }
}

pub trait Rle<Cfg = ()>: HasLength + Sliceable + Mergable<Cfg> + Debug + Clone {}

pub trait ZeroElement {
    fn zero_element() -> Self;
}

impl<T: Default> ZeroElement for T {
    fn zero_element() -> Self {
        Default::default()
    }
}

impl<T: HasLength + Sliceable + Mergable<Cfg> + Debug + Clone, Cfg> Rle<Cfg> for T {}

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
    fn len(&self) -> usize {
        cast(self.end - self.start).unwrap()
    }
}

/// this can make iter return type has len
impl<A, T: HasLength> HasLength for (A, T) {
    fn len(&self) -> usize {
        self.1.len()
    }
}

/// this can make iter return type has len
impl<T: HasLength> HasLength for &T {
    fn len(&self) -> usize {
        (*self).len()
    }
}

impl<T: HasLength + Sliceable, A: Array<Item = T>> Sliceable for SmallVec<A> {
    fn slice(&self, from: usize, to: usize) -> Self {
        let mut index = 0;
        let mut ans = smallvec::smallvec![];
        for item in self.iter() {
            if index < to && from < index + item.content_len() {
                let start = if index < from { from - index } else { 0 };
                let len = (item.content_len() - start).min(to - index);
                ans.push(item.slice(start, start + len));
            }

            index += item.content_len();
        }

        ans
    }
}

use std::ops::Range;

use num::{cast, FromPrimitive, Integer, Num, NumCast};

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

pub trait HasLength {
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn len(&self) -> usize;
}

pub trait Rle<Cfg = ()>: HasLength + Sliceable + Mergable<Cfg> {}

impl<T: HasLength + Sliceable + Mergable<Cfg>, Cfg> Rle<Cfg> for T {}

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

use std::fmt::Debug;

use num::{traits::AsPrimitive, FromPrimitive, Integer};

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

/// NOTE: [Sliceable] implementation should be coherent with [Mergable]:
///
/// - For all k, a.slice(0,k).merge(a.slice(k, a.len())) == a
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

pub trait HasIndex: HasLength {
    type Int: GlobalIndex;
    fn get_start_index(&self) -> Self::Int;

    #[inline]
    fn get_end_index(&self) -> Self::Int {
        self.get_start_index() + Self::Int::from_usize(self.len()).unwrap()
    }
}

pub trait GlobalIndex:
    Debug + Integer + Copy + Default + FromPrimitive + AsPrimitive<usize>
{
}

impl<T: Debug + Integer + Copy + Default + FromPrimitive + AsPrimitive<usize>> GlobalIndex for T {}

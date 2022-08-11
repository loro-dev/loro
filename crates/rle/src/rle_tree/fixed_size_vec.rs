// TODO: remove this module
use bumpalo::collections::vec::Drain;

use super::Bump;
use super::BumpVec;
use std::marker::PhantomPinned;
use std::ops::Deref;
use std::ops::DerefMut;
use std::ops::RangeBounds;

#[derive(Debug)]
pub(super) struct FixedSizedVec<'a, T> {
    data: BumpVec<'a, T>,
    _pin: PhantomPinned,
}

impl<'a, T> FixedSizedVec<'a, T> {
    #[inline]
    pub(super) fn with_capacity(capacity: usize, bump: &'a Bump) -> Self {
        Self {
            data: BumpVec::with_capacity_in(capacity, bump),
            _pin: PhantomPinned,
        }
    }

    #[inline]
    pub(super) fn push(&mut self, value: T) {
        debug_assert!(self.data.len() < self.data.capacity());
        self.data.push(value);
    }

    #[inline]
    pub(crate) fn inner(&mut self) -> &mut BumpVec<'a, T> {
        &mut self.data
    }

    #[inline]
    pub(super) fn insert(&mut self, index: usize, value: T) {
        debug_assert!(self.data.len() < self.data.capacity());
        self.data.insert(index, value);
    }

    #[inline]
    pub(super) fn pop(&mut self) -> Option<T> {
        self.data.pop()
    }

    #[inline]
    pub fn drain<R>(&mut self, range: R) -> Drain<T>
    where
        R: RangeBounds<usize>,
    {
        self.data.drain(range)
    }
}

impl<'a, T> Deref for FixedSizedVec<'a, T> {
    type Target = [T];

    fn deref(&self) -> &Self::Target {
        &self.data
    }
}

impl<'a, T> DerefMut for FixedSizedVec<'a, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.data
    }
}

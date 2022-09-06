use std::ptr::NonNull;

use crate::{HasLength, Mergable, Sliceable};

impl<T> Mergable for NonNull<T> {
    fn is_mergable(&self, other: &Self, _conf: &()) -> bool
    where
        Self: Sized,
    {
        false
    }

    fn merge(&mut self, _other: &Self, _conf: &())
    where
        Self: Sized,
    {
        unreachable!()
    }
}

impl<T> Sliceable for NonNull<T> {
    fn slice(&self, from: usize, to: usize) -> Self {
        *self
    }
}

impl<T> HasLength for NonNull<T> {
    fn len(&self) -> usize {
        1
    }
}

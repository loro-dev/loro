use core::ops::RangeBounds;
use std::ops::Range;

/// For better performance, it's advised to impl split
pub trait Sliceable: HasLength + Sized {
    #[must_use]
    fn _slice(&self, range: Range<usize>) -> Self;

    #[must_use]
    #[inline(always)]
    fn slice(&self, range: impl RangeBounds<usize>) -> Self {
        let start = match range.start_bound() {
            std::ops::Bound::Included(x) => *x,
            std::ops::Bound::Excluded(x) => x + 1,
            std::ops::Bound::Unbounded => 0,
        };

        let end = match range.end_bound() {
            std::ops::Bound::Included(x) => x + 1,
            std::ops::Bound::Excluded(x) => *x,
            std::ops::Bound::Unbounded => self.rle_len(),
        };

        self._slice(start..end)
    }

    /// slice in-place
    #[inline(always)]
    fn slice_(&mut self, range: impl RangeBounds<usize>) {
        *self = self.slice(range);
    }

    #[must_use]
    fn split(&mut self, pos: usize) -> Self {
        let right = self.slice(pos..);
        self.slice_(..pos);
        right
    }

    /// Update the slice in the given range.
    /// This method may split `self` into two or three parts.
    /// If so, it will make `self` the leftmost part and return the next split parts.
    ///
    /// # Example
    ///
    /// If `self.rle_len() == 10`, `self.update(1..5)` will split self into three parts and update the middle part.
    /// It returns the middle and the right part.
    fn update_with_split(
        &mut self,
        range: impl RangeBounds<usize>,
        f: impl FnOnce(&mut Self),
    ) -> (Option<Self>, Option<Self>) {
        let start = match range.start_bound() {
            std::ops::Bound::Included(x) => *x,
            std::ops::Bound::Excluded(x) => x + 1,
            std::ops::Bound::Unbounded => 0,
        };

        let end = match range.end_bound() {
            std::ops::Bound::Included(x) => x + 1,
            std::ops::Bound::Excluded(x) => *x,
            std::ops::Bound::Unbounded => self.rle_len(),
        };

        if start >= end {
            return (None, None);
        }

        match (start == 0, end == self.rle_len()) {
            (true, true) => {
                f(self);
                (None, None)
            }
            (true, false) => {
                let right = self.split(end);
                f(self);
                (Some(right), None)
            }
            (false, true) => {
                let mut right = self.split(start);
                f(&mut right);
                (Some(right), None)
            }
            (false, false) => {
                let right = self.split(end);
                let mut middle = self.split(start);
                f(&mut middle);
                (Some(middle), Some(right))
            }
        }
    }
}

pub trait Mergeable {
    /// Whether self can merge rhs with self on the left.
    ///
    /// Note: This is not symmetric.
    fn can_merge(&self, rhs: &Self) -> bool;
    fn merge_right(&mut self, rhs: &Self);
    fn merge_left(&mut self, left: &Self);
}

pub trait HasLength {
    fn rle_len(&self) -> usize;
}

pub trait TryInsert {
    fn try_insert(&mut self, pos: usize, elem: Self) -> Result<(), Self>
    where
        Self: Sized;
}

pub trait CanRemove {
    fn can_remove(&self) -> bool;
}

impl CanRemove for () {
    fn can_remove(&self) -> bool {
        true
    }
}
impl CanRemove for usize {
    fn can_remove(&self) -> bool {
        *self == 0
    }
}
impl CanRemove for isize {
    fn can_remove(&self) -> bool {
        *self == 0
    }
}
impl CanRemove for u32 {
    fn can_remove(&self) -> bool {
        *self == 0
    }
}
impl CanRemove for i32 {
    fn can_remove(&self) -> bool {
        *self == 0
    }
}

use std::fmt::Debug;
use std::ops::{Deref, DerefMut};

use generic_btree::rle::{HasLength, Mergeable, Sliceable, TryInsert};
use heapless::Vec;

use crate::delta_trait::{DeltaAttr, DeltaValue};
use crate::{DeltaRope, DeltaRopeBuilder};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct ArrayVec<V, const C: usize> {
    vec: Vec<V, C>,
}

impl<V, const C: usize> ArrayVec<V, C> {
    pub fn new() -> Self {
        Self { vec: Vec::new() }
    }

    pub fn insert_many(&mut self, pos: usize, values: Self) -> Result<(), Self> {
        if C < self.len() + values.len() {
            return Err(values);
        }

        // SAFETY: We have the ownership of the values
        unsafe {
            let ptr_start = self.vec.as_mut_ptr().add(pos);
            ptr_start.copy_to(ptr_start.add(values.len()), self.len() - pos);
            ptr_start.copy_from_nonoverlapping(values.as_ptr(), values.len());
            self.vec.set_len(self.len() + values.len());
        }

        // This will not cause memory leak because the vec is on the stack
        // And we already take the ownership of the elements inside the vec
        std::mem::forget(values.vec);
        Ok(())
    }

    pub fn from_many(mut iter: impl Iterator<Item = V>) -> impl Iterator<Item = Self> {
        let mut new = Self::new();
        std::iter::from_fn(move || {
            let mut v = iter.next();
            while let Some(iv) = v {
                if new.vec.push(iv).is_err() {
                    unreachable!()
                }

                if new.vec.is_full() {
                    break;
                }

                v = iter.next();
            }

            if new.is_empty() {
                return None;
            }

            Some(std::mem::take(&mut new))
        })
    }

    // The type system doesn't allow us to use unexported types in trait's associated types
    // So we don't impl the trait for this
    #[allow(clippy::should_implement_trait)]
    pub fn into_iter(self) -> impl Iterator<Item = V> {
        self.vec.into_iter()
    }
}

impl<V: Debug + Clone, const C: usize, Attr: DeltaAttr> DeltaRope<ArrayVec<V, C>, Attr> {
    pub fn from_many(iter: impl Iterator<Item = V>) -> Self {
        let mut rope = DeltaRope::new();
        rope.insert_values(
            0,
            ArrayVec::from_many(iter).map(|x| crate::DeltaItem::Replace {
                value: x,
                attr: Default::default(),
                delete: 0,
            }),
        );

        rope
    }
}

impl<V: Debug + Clone, Attr: DeltaAttr, const C: usize> DeltaRopeBuilder<ArrayVec<V, C>, Attr> {
    pub fn insert_many(mut self, value_iter: impl IntoIterator<Item = V>, attr: Attr) -> Self {
        let iter = ArrayVec::from_many(value_iter.into_iter());
        for value in iter {
            self = self.insert(value, attr.clone());
        }
        self
    }
}

impl<V, const C: usize> Default for ArrayVec<V, C> {
    fn default() -> Self {
        Self::new()
    }
}

impl<V, const C: usize> Deref for ArrayVec<V, C> {
    type Target = Vec<V, C>;

    fn deref(&self) -> &Self::Target {
        &self.vec
    }
}

impl<V, const C: usize> DerefMut for ArrayVec<V, C> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.vec
    }
}

impl<const C: usize, T> HasLength for ArrayVec<T, C> {
    fn rle_len(&self) -> usize {
        self.len()
    }
}

impl<const C: usize, T: Clone> Sliceable for ArrayVec<T, C> {
    fn _slice(&self, range: std::ops::Range<usize>) -> Self {
        Self {
            vec: Vec::from_slice(&self.vec.as_slice()[range]).unwrap(),
        }
    }

    fn split(&mut self, pos: usize) -> Self {
        let right = self.slice(pos..);
        self.vec.truncate(pos);
        right
    }
}

impl<const C: usize, T: Clone> Mergeable for ArrayVec<T, C> {
    fn can_merge(&self, rhs: &Self) -> bool {
        C >= self.len() + rhs.len()
    }

    fn merge_right(&mut self, rhs: &Self) {
        self.extend_from_slice(rhs.as_slice()).unwrap();
    }

    fn merge_left(&mut self, left: &Self) {
        match self.insert_many(0, left.clone()) {
            Ok(_) => {}
            Err(_) => unreachable!(),
        }
    }
}

impl<const C: usize, T> TryInsert for ArrayVec<T, C> {
    fn try_insert(&mut self, pos: usize, elem: Self) -> Result<(), Self>
    where
        Self: Sized,
    {
        self.insert_many(pos, elem)
    }
}

impl<const C: usize, T> DeltaValue for ArrayVec<T, C> where T: Clone + Debug {}

impl<T, const A: usize, const C: usize> From<[T; A]> for ArrayVec<T, C>
where
    T: Clone,
{
    fn from(array: [T; A]) -> Self {
        let vec = Vec::from_slice(&array).unwrap();
        ArrayVec { vec }
    }
}

#[cfg(test)]
mod test {
    use std::sync::Arc;

    use super::*;

    #[test]
    fn test_insert() {
        let mut array_vec: ArrayVec<Arc<i32>, 8> =
            ArrayVec::from([Arc::new(1), Arc::new(2), Arc::new(3)]);
        let b = ArrayVec::from([Arc::new(1), Arc::new(2), Arc::new(3)]);
        array_vec.insert_many(1, b).unwrap();
    }

    #[test]
    fn test_slice() {
        let array_vec: ArrayVec<i32, 5> = ArrayVec::from([1, 2, 3, 4, 5]);
        let sliced = array_vec._slice(1..3);
        assert_eq!(sliced.as_slice(), &[2, 3]);
    }

    #[test]
    fn test_split() {
        let mut array_vec: ArrayVec<i32, 5> = ArrayVec::from([1, 2, 3, 4, 5]);
        let right = array_vec.split(2);
        assert_eq!(array_vec.as_slice(), &[1, 2]);
        assert_eq!(right.as_slice(), &[3, 4, 5]);
    }

    #[test]
    fn test_can_merge() {
        let array_vec1: ArrayVec<i32, 10> = ArrayVec::from([1, 2, 3]);
        let array_vec2: ArrayVec<i32, 10> = ArrayVec::from([4, 5, 6]);
        assert!(array_vec1.can_merge(&array_vec2));
    }

    #[test]
    fn test_merge_right() {
        let mut array_vec1: ArrayVec<i32, 10> = ArrayVec::from([1, 2, 3]);
        let array_vec2: ArrayVec<i32, 10> = ArrayVec::from([4, 5, 6]);
        array_vec1.merge_right(&array_vec2);
        assert_eq!(array_vec1.as_slice(), &[1, 2, 3, 4, 5, 6]);
    }

    #[test]
    fn test_merge_left() {
        let mut array_vec1: ArrayVec<i32, 10> = ArrayVec::from([4, 5, 6]);
        let array_vec2: ArrayVec<i32, 10> = ArrayVec::from([1, 2, 3]);
        array_vec1.merge_left(&array_vec2);
        assert_eq!(array_vec1.as_slice(), &[1, 2, 3, 4, 5, 6]);
    }

    #[test]
    fn test_try_insert() {
        let mut array_vec: ArrayVec<i32, 10> = ArrayVec::from([1, 2, 5]);
        let result = array_vec.try_insert(2, ArrayVec::from([3, 4]));
        assert!(result.is_ok());
        assert_eq!(array_vec.as_slice(), &[1, 2, 3, 4, 5]);
    }

    #[test]
    fn test_merge_fail() {
        let array_vec1: ArrayVec<i32, 5> = ArrayVec::from([1, 2, 3, 4, 5]);
        let array_vec2: ArrayVec<i32, 5> = ArrayVec::from([6, 7, 8]);
        let result = array_vec1.can_merge(&array_vec2);
        assert!(!result);
    }

    #[test]
    fn test_try_insert_fail() {
        let mut array_vec: ArrayVec<i32, 5> = ArrayVec::from([1, 2, 4, 5, 6]);
        let result = array_vec.try_insert(2, ArrayVec::from([3]));
        assert!(result.is_err());
    }

    #[test]
    fn test_insert_many_fail() {
        let mut array_vec: ArrayVec<i32, 5> = ArrayVec::from([1, 2, 3]);
        let result = array_vec.insert_many(1, ArrayVec::from([4, 5, 6]));
        assert!(result.is_err());
    }

    #[test]
    #[should_panic]
    fn merge_right_overflow() {
        let mut array_vec: ArrayVec<i32, 5> = ArrayVec::from([1, 2, 3]);
        array_vec.merge_right(&ArrayVec::from([4, 5, 6]));
    }
}

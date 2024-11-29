//! Run length encoding library.
//!
//! There are many mergeable types. By merging them together we can get a more compact representation of the data.
//! For example, in many cases, `[0..5, 5..10]` can be merged into `0..10`.
//!
//! # RleVec
//!
//! RleVec<T> is a vector that can be compressed using run-length encoding.
//!
//! A T value may be merged with its neighbors. When we push new element, the new value
//! may be merged with the last element in the array. Each value has a length, so there
//! are two types of indexes:
//! 1. (merged) It refers to the index of the merged element.
//! 2. (atom) The index of substantial elements. It refers to the index of the atom element.
//!
//! By default, we use atom index in RleVec.
//! - len() returns the number of atom elements in the array.
//! - get(index) returns the atom element at the index.
//! - slice(from, to) returns a slice of atom elements from the index from to the index to.
//!
//!
#![deny(clippy::undocumented_unsafe_blocks)]
mod rle_trait;
mod rle_vec;
pub use crate::rle_trait::{
    HasIndex, HasLength, Mergable, Rle, RleCollection, RlePush, Slice, Sliceable, ZeroElement,
};
pub use crate::rle_vec::{slice_vec_by, RleVec, RleVecWithLen};
pub mod rle_impl;

use num::Integer;

#[derive(Clone)]
pub struct SearchResult<'a, T, I: Integer> {
    pub element: &'a T,
    pub merged_index: usize,
    pub offset: I,
}

pub struct SliceIterator<'a, T> {
    vec: &'a [T],
    cur_index: usize,
    cur_offset: usize,
    end_index: Option<usize>,
    end_offset: Option<usize>,
}

impl<T> SliceIterator<'_, T> {
    fn new_empty() -> Self {
        Self {
            vec: &[],
            cur_index: 0,
            cur_offset: 0,
            end_index: None,
            end_offset: None,
        }
    }
}

impl<'a, T: HasLength> Iterator for SliceIterator<'a, T> {
    type Item = Slice<'a, T>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.vec.is_empty() {
            return None;
        }

        let end_index = self.end_index.unwrap_or(self.vec.len() - 1);
        if self.cur_index == end_index {
            let elem = &self.vec[self.cur_index];
            let end = self.end_offset.unwrap_or_else(|| elem.atom_len());
            if self.cur_offset == end {
                return None;
            }

            let ans = Slice {
                value: elem,
                start: self.cur_offset,
                end,
            };
            self.cur_offset = end;
            return Some(ans);
        }

        let ans = Slice {
            value: &self.vec[self.cur_index],
            start: self.cur_offset,
            end: self.vec[self.cur_index].atom_len(),
        };

        self.cur_index += 1;
        self.cur_offset = 0;
        Some(ans)
    }
}

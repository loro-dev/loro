use std::{
    fmt::Debug,
    ops::{Deref, Index, Range},
};

use num::{traits::AsPrimitive, FromPrimitive};
use smallvec::{Array, SmallVec};

use crate::{rle_trait::HasIndex, HasLength, Mergable, SearchResult, SliceIterator, Sliceable};

/// RleVec<T> is a vector that can be compressed using run-length encoding.
///
/// A T value may be merged with its neighbors. When we push new element, the new value
/// may be merged with the last element in the array. Each value has a length, so there
/// are two types of indexes:
/// 1. (merged) It refers to the index of the merged element.
/// 2. (atom) The index of substantial elements. It refers to the index of the atom element.
///
/// By default, we use atom index in RleVec.
/// - len() returns the number of atom elements in the array.
/// - get(index) returns the atom element at the index.
/// - slice(from, to) returns a slice of atom elements from the index from to the index to.
pub struct RleVec<A: Array> {
    vec: SmallVec<A>,
}

pub struct RleVecWithLen<A: Array> {
    vec: RleVec<A>,
    atom_len: usize,
}

impl<A: Array> RleVecWithLen<A>
where
    A::Item: HasLength + Mergable,
{
    pub fn push(&mut self, value: A::Item) {
        self.atom_len += value.atom_len();
        self.vec.push(value);
    }

    pub fn new() -> Self {
        Self {
            vec: Default::default(),
            atom_len: 0,
        }
    }

    pub fn with_capacity(size: usize) -> Self {
        Self {
            vec: RleVec::with_capacity(size),
            atom_len: 0,
        }
    }

    pub fn merged_len(&self) -> usize {
        self.vec.merged_len()
    }

    pub fn iter(&self) -> std::slice::Iter<'_, A::Item> {
        self.vec.iter()
    }

    /// # Safety
    ///
    /// should not change the element's length during iter
    pub unsafe fn iter_mut(&mut self) -> std::slice::IterMut<'_, A::Item> {
        self.vec.iter_mut()
    }

    pub fn reverse(&mut self) {
        self.vec.reverse()
    }

    pub fn check(&self) {
        assert_eq!(
            self.atom_len,
            self.vec.iter().map(|x| x.atom_len()).sum::<usize>()
        );
    }
}

impl<A: Array> Default for RleVecWithLen<A>
where
    A::Item: HasLength + Mergable,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<A: Array> RleVecWithLen<A>
where
    A::Item: HasLength + Mergable + HasIndex,
{
    pub fn get(
        &self,
        index: <A::Item as HasIndex>::Int,
    ) -> Option<SearchResult<'_, A::Item, <A::Item as HasIndex>::Int>> {
        self.vec.get(index)
    }

    pub fn iter_by_index(
        &self,
        from: <A::Item as HasIndex>::Int,
        to: <A::Item as HasIndex>::Int,
    ) -> SliceIterator<'_, A::Item> {
        self.vec.iter_by_index(from, to)
    }
}

impl<A: Array> HasLength for RleVecWithLen<A>
where
    A::Item: HasLength + Mergable,
{
    fn content_len(&self) -> usize {
        self.atom_len
    }
}

impl<A: Array> RleVec<A> {
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.vec.is_empty()
    }

    #[inline]
    pub fn new() -> Self {
        RleVec {
            vec: SmallVec::new(),
        }
    }

    #[inline]
    pub fn with_capacity(size: usize) -> Self {
        RleVec {
            vec: SmallVec::with_capacity(size),
        }
    }

    #[inline]
    pub fn capacity(&self) -> usize {
        self.vec.capacity()
    }

    /// this is the length of merged elements
    pub fn len(&self) -> usize {
        self.vec.len()
    }

    pub fn reverse(&mut self) {
        self.vec.reverse()
    }
}

impl<A: Array> IntoIterator for RleVec<A> {
    type Item = A::Item;

    type IntoIter = smallvec::IntoIter<A>;

    fn into_iter(self) -> Self::IntoIter {
        self.vec.into_iter()
    }
}

impl<A: Array> Debug for RleVecWithLen<A>
where
    A::Item: Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RleVecWithLen")
            .field("vec", &self.vec)
            .field("atom_len", &self.atom_len)
            .finish()
    }
}

impl<A: Array> Clone for RleVecWithLen<A>
where
    A::Item: Clone,
{
    fn clone(&self) -> Self {
        Self {
            vec: self.vec.clone(),
            atom_len: self.atom_len,
        }
    }
}

impl<A: Array> Clone for RleVec<A>
where
    A::Item: Clone,
{
    fn clone(&self) -> Self {
        Self {
            vec: self.vec.clone(),
        }
    }
}
impl<A: Array> Debug for RleVec<A>
where
    A::Item: Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RleVec").field("vec", &self.vec).finish()
    }
}

impl<A: Array> Index<usize> for RleVec<A> {
    type Output = A::Item;

    fn index(&self, index: usize) -> &Self::Output {
        &self.vec[index]
    }
}

impl<A: Array> Index<usize> for RleVecWithLen<A> {
    type Output = A::Item;

    fn index(&self, index: usize) -> &Self::Output {
        &self.vec[index]
    }
}

impl<A: Array> PartialEq for RleVec<A>
where
    A::Item: PartialEq,
{
    fn eq(&self, other: &Self) -> bool {
        self.vec == other.vec
    }
}

impl<A: Array> Eq for RleVec<A> where A::Item: Eq + PartialEq {}
impl<A: Array> PartialEq for RleVecWithLen<A>
where
    A::Item: Eq + PartialEq,
{
    fn eq(&self, other: &Self) -> bool {
        self.vec == other.vec
    }
}

impl<A: Array> Eq for RleVecWithLen<A> where A::Item: Eq + PartialEq {}

impl<A: Array> RleVec<A>
where
    A::Item: Mergable + HasLength,
{
    /// push a new element to the end of the array. It may be merged with last element.
    pub fn push(&mut self, value: A::Item) {
        if let Some(last) = self.vec.last_mut() {
            if last.is_mergable(&value, &()) {
                last.merge(&value, &());
                return;
            }
        }

        self.vec.push(value);
    }
}
impl<A: Array> RleVec<A>
where
    A::Item: Mergable + HasLength + HasIndex,
{
    /// return end - start
    pub fn span(&self) -> <A::Item as HasIndex>::Int {
        match (self.vec.first(), self.vec.last()) {
            (Some(first), Some(last)) => last.get_end_index() - first.get_start_index(),
            _ => <<A::Item as HasIndex>::Int as FromPrimitive>::from_usize(0).unwrap(),
        }
    }

    pub fn iter_by_index(
        &self,
        from: <A::Item as HasIndex>::Int,
        to: <A::Item as HasIndex>::Int,
    ) -> SliceIterator<'_, A::Item> {
        if from == to {
            return SliceIterator::new_empty();
        }

        let start = self.get(from);
        if start.is_none() {
            return SliceIterator::new_empty();
        }

        let start = start.unwrap();
        let end = self.get(to);
        if let Some(end) = end {
            SliceIterator {
                vec: &self.vec,
                cur_index: start.merged_index,
                cur_offset: start.offset.as_(),
                end_index: Some(end.merged_index),
                end_offset: Some(end.offset.as_()),
            }
        } else {
            SliceIterator {
                vec: &self.vec,
                cur_index: start.merged_index,
                cur_offset: start.offset.as_(),
                end_index: None,
                end_offset: None,
            }
        }
    }

    #[inline]
    pub fn end(&self) -> <A::Item as HasIndex>::Int {
        self.vec
            .last()
            .map(|x| x.get_end_index())
            .unwrap_or_else(|| {
                <<A::Item as HasIndex>::Int as FromPrimitive>::from_usize(0).unwrap()
            })
    }

    /// get the element at the given atom index.
    /// return: (element, merged_index, offset)
    pub fn get(
        &self,
        index: <A::Item as HasIndex>::Int,
    ) -> Option<SearchResult<'_, A::Item, <A::Item as HasIndex>::Int>> {
        if index > self.end() {
            return None;
        }

        let mut start = 0;
        let mut end = self.vec.len() - 1;
        while start < end {
            let mid = (start + end) / 2;
            match self[mid].get_start_index().cmp(&index) {
                std::cmp::Ordering::Equal => {
                    start = mid;
                    break;
                }
                std::cmp::Ordering::Less => {
                    start = mid + 1;
                }
                std::cmp::Ordering::Greater => {
                    end = mid;
                }
            }
        }

        if index < self[start].get_start_index() {
            start -= 1;
        }

        let value = &self.vec[start];
        Some(SearchResult {
            element: value,
            merged_index: start,
            offset: index - self[start].get_start_index(),
        })
    }

    #[inline]
    pub fn slice_merged(&self, range: Range<usize>) -> &[A::Item] {
        &self.vec[range]
    }
}
impl<A: Array> RleVec<A>
where
    A::Item: Mergable + HasLength + HasIndex + Sliceable,
{
    /// This is different from [Sliceable::slice].
    /// This slice method is based on each element's [HasIndex].
    /// [Sliceable::slice] is based on the accumulated length of each element
    pub fn slice_by_index(
        &self,
        from: <A::Item as HasIndex>::Int,
        to: <A::Item as HasIndex>::Int,
    ) -> Self {
        self.iter_by_index(from, to)
            .map(|x| x.value.slice(x.start, x.end))
            .collect()
    }
}

impl<A: Array> From<Vec<A::Item>> for RleVec<A>
where
    A::Item: Mergable + HasLength,
{
    fn from(vec: Vec<A::Item>) -> Self {
        let mut ans: RleVec<A> = RleVec::with_capacity(vec.len());
        for v in vec {
            ans.push(v);
        }
        ans.vec.shrink_to_fit();
        ans
    }
}

impl<A: Array> From<&[A::Item]> for RleVec<A>
where
    A::Item: Mergable + HasLength + Clone,
{
    fn from(value: &[A::Item]) -> Self {
        let mut ans: RleVec<A> = RleVec::with_capacity(value.len());
        for v in value.iter() {
            ans.push(v.clone());
        }
        ans.vec.shrink_to_fit();
        ans
    }
}

impl<A: Array> From<SmallVec<A>> for RleVec<A> {
    fn from(value: SmallVec<A>) -> Self {
        RleVec { vec: value }
    }
}

impl<A: Array> From<RleVec<A>> for SmallVec<A> {
    fn from(value: RleVec<A>) -> Self {
        value.vec
    }
}

impl<A: Array> RleVec<A> {
    #[inline(always)]
    pub fn merged_len(&self) -> usize {
        self.vec.len()
    }

    #[inline(always)]
    pub fn vec(&self) -> &SmallVec<A> {
        &self.vec
    }

    #[inline(always)]
    pub fn vec_mut(&mut self) -> &mut SmallVec<A> {
        &mut self.vec
    }

    #[inline(always)]
    pub fn iter(&self) -> std::slice::Iter<'_, A::Item> {
        self.vec.iter()
    }

    #[inline(always)]
    pub fn iter_mut(&mut self) -> std::slice::IterMut<'_, A::Item> {
        self.vec.iter_mut()
    }

    #[inline(always)]
    pub fn get_merged(&self, index: usize) -> Option<&A::Item> {
        self.vec.get(index)
    }
}

impl<A: Array> Default for RleVec<A> {
    fn default() -> Self {
        Self::new()
    }
}

impl<A: Array> FromIterator<A::Item> for RleVec<A>
where
    A::Item: Mergable + HasLength,
{
    fn from_iter<I: IntoIterator<Item = A::Item>>(iter: I) -> Self {
        let mut vec = RleVec::new();
        for item in iter {
            vec.push(item);
        }
        vec
    }
}

impl<A: Array> Mergable for RleVec<A>
where
    A::Item: Clone + Mergable + HasLength + Sliceable,
{
    fn is_mergable(&self, other: &Self, _: &()) -> bool {
        self.vec.len() + other.vec.len() < self.capacity()
    }

    fn merge(&mut self, other: &Self, _: &()) {
        for item in other.vec.iter() {
            self.push(item.clone());
        }
    }
}

impl<A: Array> Sliceable for RleVec<A>
where
    A::Item: Mergable + HasLength + Sliceable,
{
    fn slice(&self, start: usize, end: usize) -> Self {
        if start >= end {
            return Self::new();
        }

        let mut ans = SmallVec::new();
        let mut index = 0;
        for i in 0..self.vec.len() {
            if index >= end {
                break;
            }

            let len = self[i].atom_len();
            if start < index + len {
                ans.push(self[i].slice(start.saturating_sub(index), (end - index).min(len)))
            }

            index += len;
        }

        Self { vec: ans }
    }
}

impl<A: Array> Mergable for RleVecWithLen<A>
where
    A::Item: Clone + Mergable + HasLength + Sliceable,
{
    fn is_mergable(&self, other: &Self, _: &()) -> bool {
        self.vec.is_mergable(&other.vec, &())
    }

    fn merge(&mut self, other: &Self, _: &()) {
        for item in other.vec.iter() {
            self.push(item.clone());
        }
    }
}

impl<A: Array> Sliceable for RleVecWithLen<A>
where
    A::Item: Mergable + HasLength + Sliceable,
{
    fn slice(&self, start: usize, end: usize) -> Self {
        Self {
            vec: self.vec.slice(start, end),
            atom_len: end - start,
        }
    }
}

impl<A: Array> Deref for RleVec<A> {
    type Target = [A::Item];

    fn deref(&self) -> &Self::Target {
        &self.vec
    }
}

impl<A: Array> Deref for RleVecWithLen<A> {
    type Target = RleVec<A>;

    fn deref(&self) -> &Self::Target {
        &self.vec
    }
}

pub fn slice_vec_by<T, F>(vec: &Vec<T>, index: F, start: usize, end: usize) -> Vec<T>
where
    F: Fn(&T) -> usize,
    T: Sliceable + HasLength,
{
    if start >= end || vec.is_empty() {
        return Vec::new();
    }

    let start = start - index(&vec[0]);
    let end = end - index(&vec[0]);
    let mut ans = Vec::new();
    let mut index = 0;
    for i in 0..vec.len() {
        if index >= end {
            break;
        }

        let len = vec[i].atom_len();
        if start < index + len {
            ans.push(vec[i].slice(start.saturating_sub(index), (end - index).min(len)))
        }

        index += len;
    }

    ans
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn slice() {
        let mut a: RleVec<[Range<usize>; 4]> = RleVec::new();
        a.push(0..5);
        a.push(5..8);
        a.push(10..13);
        assert_eq!(&*a.slice(0, 5), &vec![0..5]);
        assert_eq!(&*a.slice(5, 10), &vec![5..8, 10..12]);
        assert_eq!(&*a.slice(5, 5), &vec![]);

        let ans = a.slice_by_index(3, 11);
        assert_eq!(&*ans, &vec![3..8, 10..11]);
        let ans = a.slice_by_index(3, 100);
        assert_eq!(&*ans, &vec![3..8, 10..13]);
        assert_eq!(*a.last().unwrap(), 10..13);
        for k in a.iter() {
            println!("{:?}", k);
        }
    }
}

use std::ops::{Deref, Range};

use num::Integer;

use crate::{HasLength, Mergable, Slice, Sliceable};

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
///
/// TODO: remove index and _len
#[derive(Debug, Clone)]
pub struct RleVecWithIndex<T, Cfg = ()> {
    vec: Vec<T>,
    atom_len: usize,
    index: Vec<usize>,
    cfg: Cfg,
}

#[derive(Clone)]
pub struct SearchResult<'a, T, I: Integer> {
    pub element: &'a T,
    pub merged_index: usize,
    pub offset: I,
}

impl<T: Eq + PartialEq> PartialEq for RleVecWithIndex<T> {
    fn eq(&self, other: &Self) -> bool {
        self.vec == other.vec
    }
}

impl<T: Eq + PartialEq> Eq for RleVecWithIndex<T> {}

impl<T: Mergable<Cfg> + HasLength, Cfg> RleVecWithIndex<T, Cfg> {
    /// push a new element to the end of the array. It may be merged with last element.
    pub fn push(&mut self, value: T) {
        self.atom_len += value.content_len();
        if self.vec.is_empty() {
            self.vec.push(value);
            self.index.push(0);
            self.index.push(self.atom_len);
            return;
        }

        let last = self.vec.last_mut().unwrap();
        if last.is_mergable(&value, &self.cfg) {
            last.merge(&value, &self.cfg);
            *self.index.last_mut().unwrap() = self.atom_len;
            return;
        }
        self.vec.push(value);
        self.index.push(self.atom_len);
    }

    pub fn is_empty(&self) -> bool {
        self.vec.is_empty()
    }

    /// get the element at the given atom index.
    /// return: (element, merged_index, offset)
    pub fn get(&self, index: usize) -> Option<SearchResult<'_, T, usize>> {
        if index > self.atom_len {
            return None;
        }

        // TODO: test this threshold
        if self.vec.len() < 8 {
            for (i, v) in self.vec.iter().enumerate() {
                if self.index[i] <= index && index < self.index[i + 1] {
                    return Some(SearchResult {
                        element: v,
                        merged_index: i,
                        offset: index - self.index[i],
                    });
                }
            }

            return None;
        }

        let mut start = 0;
        let mut end = self.index.len() - 1;
        while start < end {
            let mid = (start + end) / 2;
            if self.index[mid] == index {
                start = mid;
                break;
            }

            if self.index[mid] < index {
                start = mid + 1;
            } else {
                end = mid;
            }
        }

        if index < self.index[start] {
            start -= 1;
        }

        let value = &self.vec[start];
        Some(SearchResult {
            element: value,
            merged_index: start,
            offset: index - self.index[start],
        })
    }

    /// get a slice from `from` to `to` with atom indexes
    pub fn slice_iter(&self, from: usize, to: usize) -> SliceIterator<'_, T> {
        if from == to {
            return SliceIterator::new_empty();
        }

        let from_result = self.get(from);
        if from_result.is_none() {
            return SliceIterator::new_empty();
        }

        let from_result = from_result.unwrap();
        let to_result = if to == self.atom_len {
            None
        } else {
            self.get(to)
        };
        if let Some(to_result) = to_result {
            SliceIterator {
                vec: &self.vec,
                cur_index: from_result.merged_index,
                cur_offset: from_result.offset,
                end_index: Some(to_result.merged_index),
                end_offset: Some(to_result.offset),
            }
        } else {
            SliceIterator {
                vec: &self.vec,
                cur_index: from_result.merged_index,
                cur_offset: from_result.offset,
                end_index: None,
                end_offset: None,
            }
        }
    }

    pub fn slice_merged(&self, range: Range<usize>) -> &[T] {
        &self.vec[range]
    }
}

impl<T, Conf: Default> RleVecWithIndex<T, Conf> {
    pub fn new() -> Self {
        RleVecWithIndex {
            vec: Vec::new(),
            atom_len: 0,
            index: Vec::new(),
            cfg: Default::default(),
        }
    }
}

impl<T, Cfg> RleVecWithIndex<T, Cfg> {
    pub fn new_with_conf(cfg: Cfg) -> Self {
        RleVecWithIndex {
            vec: Vec::new(),
            atom_len: 0,
            index: Vec::new(),
            cfg,
        }
    }
}

impl<T, Conf> RleVecWithIndex<T, Conf> {
    pub fn with_capacity(&mut self, capacity: usize) -> &mut Self {
        self.vec.reserve(capacity);
        self.index.reserve(capacity + 1);
        self
    }
}

impl<T: Mergable<Conf> + HasLength, Conf: Default> From<Vec<T>> for RleVecWithIndex<T, Conf> {
    fn from(vec: Vec<T>) -> Self {
        let mut ans: RleVecWithIndex<T, Conf> = RleVecWithIndex::new();
        ans.with_capacity(vec.len());
        for v in vec {
            ans.push(v);
        }
        ans
    }
}

impl<T, Conf> RleVecWithIndex<T, Conf> {
    #[inline]
    pub fn new_cfg(cfg: Conf) -> Self {
        RleVecWithIndex {
            vec: Vec::new(),
            atom_len: 0,
            index: Vec::new(),
            cfg,
        }
    }

    #[inline(always)]
    pub fn merged_len(&self) -> usize {
        self.vec.len()
    }

    #[inline(always)]
    pub fn to_vec(self) -> Vec<T> {
        self.vec
    }

    #[inline(always)]
    pub fn vec(&self) -> &Vec<T> {
        &self.vec
    }

    #[inline(always)]
    pub fn iter(&self) -> std::slice::Iter<'_, T> {
        self.vec.iter()
    }

    #[inline(always)]
    pub fn vec_mut(&mut self) -> &mut Vec<T> {
        &mut self.vec
    }

    #[inline(always)]
    pub fn get_merged(&self, index: usize) -> Option<&T> {
        self.vec.get(index)
    }
}

impl<T> Default for RleVecWithIndex<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Mergable + HasLength> FromIterator<T> for RleVecWithIndex<T> {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        let mut vec = RleVecWithIndex::new();
        for item in iter {
            vec.push(item);
        }
        vec
    }
}

pub struct SliceIterator<'a, T> {
    pub(super) vec: &'a [T],
    pub(super) cur_index: usize,
    pub(super) cur_offset: usize,
    pub(super) end_index: Option<usize>,
    pub(super) end_offset: Option<usize>,
}

impl<'a, T> SliceIterator<'a, T> {
    pub(super) fn new_empty() -> Self {
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

impl<T: Mergable<Cfg> + HasLength + Sliceable + Clone, Cfg> Mergable<Cfg>
    for RleVecWithIndex<T, Cfg>
{
    fn is_mergable(&self, _: &Self, _: &Cfg) -> bool {
        true
    }

    fn merge(&mut self, other: &Self, _: &Cfg) {
        for item in other.vec.iter() {
            self.push(item.clone());
        }
    }
}

impl<T: Mergable + HasLength + Sliceable + Clone> Sliceable for RleVecWithIndex<T> {
    fn slice(&self, start: usize, end: usize) -> Self {
        self.slice_iter(start, end)
            .map(|x| x.into_inner())
            .collect()
    }
}

impl<T, Cfg> HasLength for RleVecWithIndex<T, Cfg> {
    fn content_len(&self) -> usize {
        self.atom_len
    }

    fn atom_len(&self) -> usize {
        self.atom_len
    }
}

impl<T, Cfg> Deref for RleVecWithIndex<T, Cfg> {
    type Target = [T];

    fn deref(&self) -> &Self::Target {
        self.vec()
    }
}

#[cfg(test)]
mod test {
    mod prime_value {
        use crate::{HasLength, Mergable, RleVecWithIndex, Sliceable};

        impl HasLength for String {
            fn content_len(&self) -> usize {
                self.len()
            }
        }

        impl Mergable for String {
            fn is_mergable(&self, _: &Self, _: &()) -> bool {
                self.len() < 8
            }

            fn merge(&mut self, other: &Self, _: &()) {
                self.push_str(other);
            }
        }

        impl Sliceable for String {
            fn slice(&self, start: usize, end: usize) -> Self {
                self[start..end].to_string()
            }
        }

        #[test]
        fn get_at_atom_index() {
            let mut vec: RleVecWithIndex<String> = RleVecWithIndex::new();
            vec.push("1234".to_string());
            vec.push("5678".to_string());
            vec.push("12345678".to_string());
            assert_eq!(vec.get(4).unwrap().element, "12345678");
            assert_eq!(vec.get(4).unwrap().merged_index, 0);
            assert_eq!(vec.get(4).unwrap().offset, 4);

            assert_eq!(vec.get(8).unwrap().element, "12345678");
            assert_eq!(vec.get(8).unwrap().merged_index, 1);
            assert_eq!(vec.get(8).unwrap().offset, 0);
        }

        #[test]
        fn slice() {
            let mut vec: RleVecWithIndex<String> = RleVecWithIndex::new();
            vec.push("1234".to_string());
            vec.push("56".to_string());
            vec.push("78".to_string());
            vec.push("12345678".to_string());
            let mut iter = vec.slice_iter(4, 12);
            let first = iter.next().unwrap();
            assert_eq!(first.value, "12345678");
            assert_eq!(first.start, 4);
            assert_eq!(first.end, 8);
            let second = iter.next().unwrap();
            assert_eq!(second.value, "12345678");
            assert_eq!(second.start, 0);
            assert_eq!(second.end, 4);
        }
    }
}

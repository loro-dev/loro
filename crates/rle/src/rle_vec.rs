use num::{cast, Integer, NumCast};
use std::ops::{Range, Sub};

use crate::{HasLength, Mergable, Rle, Slice, Sliceable};

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
#[derive(Debug, Clone)]
pub struct RleVec<T, Cfg = ()> {
    vec: Vec<T>,
    _len: usize,
    index: Vec<usize>,
    cfg: Cfg,
}

pub struct SearchResult<'a, T> {
    pub element: &'a T,
    pub merged_index: usize,
    pub offset: usize,
}

impl<T: Mergable<Cfg> + HasLength, Cfg> RleVec<T, Cfg> {
    /// push a new element to the end of the array. It may be merged with last element.
    pub fn push(&mut self, value: T) {
        self._len += value.len();
        if self.vec.is_empty() {
            self.vec.push(value);
            self.index.push(0);
            self.index.push(self._len);
            return;
        }

        let last = self.vec.last_mut().unwrap();
        if last.is_mergable(&value, &self.cfg) {
            last.merge(&value, &self.cfg);
            *self.index.last_mut().unwrap() = self._len;
            return;
        }
        self.vec.push(value);
        self.index.push(self._len);
    }

    pub fn is_empty(&self) -> bool {
        self.vec.is_empty()
    }

    /// number of atom elements in the array.
    pub fn len(&self) -> usize {
        self._len
    }

    /// get the element at the given atom index.
    /// return: (element, merged_index, offset)
    pub fn get(&self, index: usize) -> Option<SearchResult<'_, T>> {
        if index > self.len() {
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
            unreachable!();
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
        let from_result = self.get(from).unwrap();
        let to_result = self.get(to).unwrap();
        SliceIterator {
            vec: &self.vec,
            cur_index: from_result.merged_index,
            cur_offset: from_result.offset,
            end_index: to_result.merged_index,
            end_offset: to_result.offset,
        }
    }

    pub fn slice_merged(&self, range: Range<usize>) -> &[T] {
        &self.vec[range]
    }
}

impl<T, Conf: Default> RleVec<T, Conf> {
    pub fn new() -> Self {
        RleVec {
            vec: Vec::new(),
            _len: 0,
            index: Vec::new(),
            cfg: Default::default(),
        }
    }
}

impl<T, Conf> RleVec<T, Conf> {
    pub fn with_capacity(&mut self, capacity: usize) -> &mut Self {
        self.vec.reserve(capacity);
        self.index.reserve(capacity + 1);
        self
    }
}

impl<T: Mergable<Conf> + HasLength, Conf: Default> From<Vec<T>> for RleVec<T, Conf> {
    fn from(vec: Vec<T>) -> Self {
        let mut ans: RleVec<T, Conf> = RleVec::new();
        ans.with_capacity(vec.len());
        for v in vec {
            ans.push(v);
        }
        ans
    }
}

impl<T, Conf> RleVec<T, Conf> {
    #[inline]
    pub fn new_cfg(cfg: Conf) -> Self {
        RleVec {
            vec: Vec::new(),
            _len: 0,
            index: Vec::new(),
            cfg,
        }
    }

    #[inline]
    pub fn merged_len(&self) -> usize {
        self.vec.len()
    }

    #[inline]
    pub fn to_vec(self) -> Vec<T> {
        self.vec
    }

    #[inline]
    pub fn vec(&self) -> &Vec<T> {
        &self.vec
    }

    #[inline]
    pub fn iter(&self) -> std::slice::Iter<'_, T> {
        self.vec.iter()
    }

    #[inline]
    pub fn vec_mut(&mut self) -> &mut Vec<T> {
        &mut self.vec
    }

    #[inline]
    pub fn get_merged(&self, index: usize) -> Option<&T> {
        self.vec.get(index)
    }
}

impl<T> Default for RleVec<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Mergable + HasLength> FromIterator<T> for RleVec<T> {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        let mut vec = RleVec::new();
        for item in iter {
            vec.push(item);
        }
        vec
    }
}

pub struct SliceIterator<'a, T> {
    vec: &'a Vec<T>,
    cur_index: usize,
    cur_offset: usize,
    end_index: usize,
    end_offset: usize,
}

impl<'a, T: HasLength> Iterator for SliceIterator<'a, T> {
    type Item = Slice<'a, T>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.cur_index == self.end_index {
            if self.cur_offset == self.end_offset {
                return None;
            }

            let ans = Slice {
                value: &self.vec[self.cur_index],
                start: self.cur_offset,
                end: self.end_offset,
            };
            self.cur_offset = self.end_offset;
            return Some(ans);
        }

        let ans = Slice {
            value: &self.vec[self.cur_index],
            start: self.cur_offset,
            end: self.vec[self.cur_index].len(),
        };

        self.cur_index += 1;
        self.cur_offset = 0;
        Some(ans)
    }
}

impl<T: Mergable<Cfg> + HasLength + Sliceable + Clone, Cfg> Mergable<Cfg> for RleVec<T, Cfg> {
    fn is_mergable(&self, _: &Self, _: &Cfg) -> bool {
        true
    }

    fn merge(&mut self, other: &Self, _: &Cfg) {
        for item in other.vec.iter() {
            self.push(item.clone());
        }
    }
}

impl<T: Mergable + HasLength + Sliceable + Clone> Sliceable for RleVec<T> {
    fn slice(&self, start: usize, end: usize) -> Self {
        self.slice_iter(start, end)
            .map(|x| x.into_inner())
            .collect()
    }
}

impl<T> HasLength for RleVec<T> {
    fn len(&self) -> usize {
        self._len
    }
}

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

#[cfg(test)]
mod test {
    mod prime_value {
        use crate::{HasLength, Mergable, RleVec, Sliceable};

        impl HasLength for String {
            fn len(&self) -> usize {
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
            let mut vec: RleVec<String> = RleVec::new();
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
            let mut vec: RleVec<String> = RleVec::new();
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

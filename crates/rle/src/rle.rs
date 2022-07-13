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
pub struct RleVec<T> {
    vec: Vec<T>,
    _len: usize,
    index: Vec<usize>,
}

pub trait Mergable {
    fn is_mergable(&self, other: &Self) -> bool;
    fn merge(&mut self, other: &Self);
}

pub trait Sliceable {
    fn slice(&self, from: usize, to: usize) -> Self;
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

pub struct SearchResult<'a, T> {
    element: &'a T,
    merged_index: usize,
    offset: usize,
}

impl<T: Mergable + HasLength> RleVec<T> {
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
        if last.is_mergable(&value) {
            last.merge(&value);
            *self.index.last_mut().unwrap() = self._len;
            return;
        }
        self.vec.push(value);
        self.index.push(self._len);
    }

    pub fn is_empty(&self) -> bool {
        self.vec.is_empty()
    }

    pub fn len(&self) -> usize {
        self._len
    }

    /// get the element at the given atom index.
    /// return: (element, merged_index, offset)
    pub fn get(&self, index: usize) -> SearchResult<'_, T> {
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
        SearchResult {
            element: value,
            merged_index: start,
            offset: index - self.index[start],
        }
    }

    /// get a slice from `from` to `to` with atom indexes
    pub fn slice_iter(&self, from: usize, to: usize) -> SliceIterator<'_, T> {
        let from_result = self.get(from);
        let to_result = self.get(to);
        SliceIterator {
            vec: &self.vec,
            cur_index: from_result.merged_index,
            cur_offset: from_result.offset,
            end_index: to_result.merged_index,
            end_offset: to_result.offset,
        }
    }
}

impl<T> RleVec<T> {
    pub fn new() -> Self {
        RleVec {
            vec: Vec::new(),
            _len: 0,
            index: Vec::new(),
        }
    }

    pub fn merged_len(&self) -> usize {
        self.vec.len()
    }

    pub fn to_vec(self) -> Vec<T> {
        self.vec
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

#[derive(Debug, Clone, Copy)]
pub struct Slice<'a, T> {
    pub value: &'a T,
    pub start: usize,
    pub end: usize,
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

impl<T: Mergable + HasLength + Sliceable + Clone> Mergable for RleVec<T> {
    fn is_mergable(&self, _: &Self) -> bool {
        true
    }

    fn merge(&mut self, other: &Self) {
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
            fn is_mergable(&self, _: &Self) -> bool {
                self.len() < 8
            }

            fn merge(&mut self, other: &Self) {
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
            assert_eq!(vec.get(4).element, "12345678");
            assert_eq!(vec.get(4).merged_index, 0);
            assert_eq!(vec.get(4).offset, 4);

            assert_eq!(vec.get(8).element, "12345678");
            assert_eq!(vec.get(8).merged_index, 1);
            assert_eq!(vec.get(8).offset, 0);
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

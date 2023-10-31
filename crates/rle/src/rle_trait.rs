use std::fmt::Debug;

use num::{traits::AsPrimitive, FromPrimitive, Integer};
use smallvec::{Array, SmallVec};

use crate::SearchResult;

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
    /// It is the length of the content, i.e. the length when no [Mergable::merge] ever happen.
    ///
    /// However, when the content is deleted, [HasLength::content_len] is expected to be zero in some [crate::RleTree] use cases
    fn content_len(&self) -> usize;

    /// It is the length of the atom element underneath, i.e. the length when no [Mergable::merge] ever happen.
    ///
    /// It is the same as [HasLength::atom_len] in the most of the cases.
    /// However, oppose to [HasLength::atom_len], when the content is deleted, [HasLength::content_len] should not change
    fn atom_len(&self) -> usize {
        self.content_len()
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
        self.get_start_index() + Self::Int::from_usize(self.atom_len()).unwrap()
    }
}

pub trait GlobalIndex:
    Debug + Integer + Copy + Default + FromPrimitive + AsPrimitive<usize>
{
}

impl<T: Debug + Integer + Copy + Default + FromPrimitive + AsPrimitive<usize>> GlobalIndex for T {}

impl HasLength for String {
    fn content_len(&self) -> usize {
        self.len()
    }
}

impl<T> HasLength for Vec<T> {
    fn content_len(&self) -> usize {
        self.len()
    }
}

impl<T: Clone> Sliceable for Vec<T> {
    fn slice(&self, from: usize, to: usize) -> Self {
        self[from..to].to_vec()
    }
}

impl Sliceable for String {
    fn slice(&self, from: usize, to: usize) -> Self {
        self[from..to].to_string()
    }
}

pub trait RlePush<T> {
    fn push_rle_element(&mut self, element: T);
}

pub trait RleCollection<T: HasIndex> {
    fn start(&self) -> <T as HasIndex>::Int;
    fn end(&self) -> <T as HasIndex>::Int;
    fn sum_atom_len(&self) -> <T as HasIndex>::Int;
    fn search_atom_index(&self, index: <T as HasIndex>::Int) -> usize;
    fn get_by_atom_index(
        &self,
        index: <T as HasIndex>::Int,
    ) -> Option<SearchResult<'_, T, <T as HasIndex>::Int>>;
}

impl<T: Mergable> RlePush<T> for Vec<T> {
    fn push_rle_element(&mut self, element: T) {
        match self.last_mut() {
            Some(last) if last.is_mergable(&element, &()) => {
                last.merge(&element, &());
            }
            _ => {
                self.push(element);
            }
        }
    }
}

impl<T: Mergable + HasIndex + HasLength> RleCollection<T> for Vec<T> {
    fn search_atom_index(&self, index: <T as HasIndex>::Int) -> usize {
        let mut start = 0;
        let mut end = self.len() - 1;
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
        start
    }

    fn get_by_atom_index(
        &self,
        index: <T as HasIndex>::Int,
    ) -> Option<SearchResult<'_, T, <T as HasIndex>::Int>> {
        if index > self.end() {
            return None;
        }

        let merged_index = self.search_atom_index(index);
        let value = &self[merged_index];
        Some(SearchResult {
            merged_index,
            element: value,
            offset: index - self[merged_index].get_start_index(),
        })
    }

    fn start(&self) -> <T as HasIndex>::Int {
        self.first()
            .map(|x| x.get_start_index())
            .unwrap_or_default()
    }

    fn end(&self) -> <T as HasIndex>::Int {
        self.last().map(|x| x.get_end_index()).unwrap_or_default()
    }

    fn sum_atom_len(&self) -> <T as HasIndex>::Int {
        self.end() - self.start()
    }
}

impl<A: Array> RlePush<A::Item> for SmallVec<A>
where
    A::Item: Mergable,
{
    fn push_rle_element(&mut self, element: A::Item) {
        match self.last_mut() {
            Some(last) if last.is_mergable(&element, &()) => {
                last.merge(&element, &());
            }
            _ => {
                self.push(element);
            }
        }
    }
}

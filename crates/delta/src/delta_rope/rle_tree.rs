use std::{
    fmt::Debug,
    iter::Sum,
    ops::{Add, AddAssign, Sub},
};

use generic_btree::{rle::CanRemove, ArenaIndex, BTreeTrait, Child, FindResult, Query};

use crate::{
    delta_trait::{DeltaAttr, DeltaValue},
    DeltaItem,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct Len {
    /// The length of insertions + retains
    pub data_len: isize,
    /// The length of deletions + retains + insertions
    pub delta_len: isize,
}

impl CanRemove for Len {
    fn can_remove(&self) -> bool {
        self.data_len == 0 && self.delta_len == 0
    }
}

impl Add for Len {
    type Output = Self;

    fn add(self, rhs: Self) -> Self {
        Self {
            data_len: self.data_len + rhs.data_len,
            delta_len: self.delta_len + rhs.delta_len,
        }
    }
}

impl AddAssign for Len {
    fn add_assign(&mut self, rhs: Self) {
        self.data_len += rhs.data_len;
        self.delta_len += rhs.delta_len;
    }
}

impl Sub for Len {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self {
        Self {
            data_len: self.data_len - rhs.data_len,
            delta_len: self.delta_len - rhs.delta_len,
        }
    }
}

impl Sum for Len {
    fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
        iter.fold(
            Self {
                data_len: 0,
                delta_len: 0,
            },
            |acc, x| acc + x,
        )
    }
}

pub(crate) struct DeltaTreeTrait<V, Attr> {
    _phantom: std::marker::PhantomData<(V, Attr)>,
}

impl<V: DeltaValue + Debug, Attr: DeltaAttr + Debug> BTreeTrait for DeltaTreeTrait<V, Attr> {
    type Elem = DeltaItem<V, Attr>;

    type Cache = Len;

    type CacheDiff = Len;

    fn calc_cache_internal(
        cache: &mut Self::Cache,
        caches: &[generic_btree::Child<Self>],
    ) -> Self::CacheDiff {
        let new = caches.iter().map(|c| c.cache).sum();
        let diff = new - *cache;
        *cache = new;
        diff
    }

    fn apply_cache_diff(cache: &mut Self::Cache, diff: &Self::CacheDiff) {
        *cache += *diff;
    }

    fn merge_cache_diff(diff1: &mut Self::CacheDiff, diff2: &Self::CacheDiff) {
        *diff1 += *diff2;
    }

    fn get_elem_cache(elem: &Self::Elem) -> Self::Cache {
        match elem {
            DeltaItem::Retain { len, attr: _ } => Len {
                data_len: *len as isize,
                delta_len: *len as isize,
            },
            DeltaItem::Replace {
                value,
                attr: _,
                delete,
            } => Len {
                data_len: value.rle_len() as isize,
                delta_len: *delete as isize + value.rle_len() as isize,
            },
        }
    }

    fn new_cache_to_diff(cache: &Self::Cache) -> Self::CacheDiff {
        *cache
    }

    fn sub_cache(cache_lhs: &Self::Cache, cache_rhs: &Self::Cache) -> Self::CacheDiff {
        *cache_lhs - *cache_rhs
    }
}

/// A generic length finder
pub struct LengthFinder {
    pub left: usize,
    pub slot: u8,
    pub parent: Option<ArenaIndex>,
}

impl LengthFinder {
    #[inline(always)]
    pub fn new() -> Self {
        Self {
            left: 0,
            slot: 0,
            parent: None,
        }
    }
}

impl Default for LengthFinder {
    #[inline(always)]
    fn default() -> Self {
        Self::new()
    }
}

impl<V: DeltaValue + Debug, Attr: DeltaAttr + Debug> Query<DeltaTreeTrait<V, Attr>>
    for LengthFinder
{
    type QueryArg = usize;

    #[inline(always)]
    fn init(target: &Self::QueryArg) -> Self {
        Self {
            left: *target,
            slot: 0,
            parent: None,
        }
    }

    #[inline(always)]
    fn find_node(
        &mut self,
        _: &Self::QueryArg,
        child_caches: &[Child<DeltaTreeTrait<V, Attr>>],
    ) -> FindResult {
        let mut last_left = self.left;
        let is_internal = matches!(child_caches.first().unwrap().arena, ArenaIndex::Internal(_));
        for (i, cache) in child_caches.iter().enumerate() {
            let len = cache.cache.data_len as usize;
            if self.left >= len {
                last_left = self.left;
                self.left -= len;
            } else {
                if is_internal {
                    self.parent = Some(cache.arena);
                } else {
                    self.slot = i as u8;
                }
                return FindResult::new_found(i, self.left);
            }
        }

        self.left = last_left;
        if is_internal {
            self.parent = Some(child_caches.last().unwrap().arena);
        } else {
            self.slot = child_caches.len() as u8 - 1;
        }
        FindResult::new_missing(child_caches.len() - 1, last_left)
    }

    #[inline(always)]
    fn confirm_elem(&mut self, _: &Self::QueryArg, elem: &DeltaItem<V, Attr>) -> (usize, bool) {
        (self.left, self.left < elem.data_len())
    }
}

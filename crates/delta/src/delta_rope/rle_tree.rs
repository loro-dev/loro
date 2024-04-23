use std::{
    fmt::Debug,
    iter::Sum,
    ops::{Add, AddAssign, Sub},
};

use generic_btree::{rle::CanRemove, BTreeTrait, UseLengthFinder};

use crate::{
    delta_trait::{DeltaAttr, DeltaValue},
    DeltaItem,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct Len {
    pub new_len: isize,
    pub old_len: isize,
}

impl CanRemove for Len {
    fn can_remove(&self) -> bool {
        self.new_len == 0 && self.old_len == 0
    }
}

impl Add for Len {
    type Output = Self;

    fn add(self, rhs: Self) -> Self {
        Self {
            new_len: self.new_len + rhs.new_len,
            old_len: self.old_len + rhs.old_len,
        }
    }
}

impl AddAssign for Len {
    fn add_assign(&mut self, rhs: Self) {
        self.new_len += rhs.new_len;
        self.old_len += rhs.old_len;
    }
}

impl Sub for Len {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self {
        Self {
            new_len: self.new_len - rhs.new_len,
            old_len: self.old_len - rhs.old_len,
        }
    }
}

impl Sum for Len {
    fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
        iter.fold(
            Self {
                new_len: 0,
                old_len: 0,
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
            DeltaItem::Retain { len, attr } => Len {
                new_len: *len as isize,
                old_len: *len as isize,
            },
            DeltaItem::Replace {
                value,
                attr,
                delete,
            } => Len {
                new_len: value.rle_len() as isize,
                old_len: *delete as isize,
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

impl<V: DeltaValue + Debug, Attr: DeltaAttr + Debug> UseLengthFinder<DeltaTreeTrait<V, Attr>>
    for DeltaTreeTrait<V, Attr>
{
    fn get_len(cache: &<DeltaTreeTrait<V, Attr> as BTreeTrait>::Cache) -> usize {
        cache.new_len as usize
    }
}

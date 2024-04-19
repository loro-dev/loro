use std::fmt::Debug;

use generic_btree::{rle::HasLength, BTreeTrait, UseLengthFinder};

use crate::{
    delta_trait::{DeltaAttr, DeltaValue},
    DeltaItem,
};

pub(crate) struct DeltaTreeTrait<V, Attr> {
    _phantom: std::marker::PhantomData<(V, Attr)>,
}

impl<V: DeltaValue + Debug, Attr: DeltaAttr + Debug> BTreeTrait for DeltaTreeTrait<V, Attr> {
    type Elem = DeltaItem<V, Attr>;

    type Cache = isize;

    type CacheDiff = isize;

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
        elem.rle_len() as isize
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
        *cache as usize
    }
}

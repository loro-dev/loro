use std::marker::PhantomData;

use generic_btree::{BTreeTrait, FindResult, Query};

/// An easy way to implement [Query] by using key index
///
/// This query implementation will prefer right element when both left element and right element are valid.
pub struct IndexQuery<T: QueryByLen<B>, B: BTreeTrait> {
    left: usize,
    _data: PhantomData<(T, B)>,
}

pub trait QueryByLen<B: BTreeTrait> {
    fn get_cache_len(cache: &B::Cache) -> usize;
    fn get_elem_len(elem: &B::Elem) -> usize;
}

impl<T: QueryByLen<B>, B: BTreeTrait> Query<B> for IndexQuery<T, B> {
    type QueryArg = usize;

    fn init(target: &Self::QueryArg) -> Self {
        Self {
            left: *target,
            _data: PhantomData,
        }
    }

    fn find_node(
        &mut self,
        _: &Self::QueryArg,
        child_caches: &[generic_btree::Child<B>],
    ) -> generic_btree::FindResult {
        let mut last_left = self.left;
        for (i, cache) in child_caches.iter().enumerate() {
            let len = T::get_cache_len(&cache.cache);
            if self.left >= len {
                last_left = self.left;
                self.left -= len;
            } else {
                return FindResult::new_found(i, self.left);
            }
        }

        self.left = last_left;
        FindResult::new_missing(child_caches.len() - 1, last_left)
    }

    fn find_element(
        &mut self,
        _: &Self::QueryArg,
        elements: &[<B as BTreeTrait>::Elem],
    ) -> generic_btree::FindResult {
        let mut last_left = self.left;
        for (i, elem) in elements.iter().enumerate() {
            let len = T::get_elem_len(elem);
            if self.left >= len {
                last_left = self.left;
                self.left -= len;
            } else {
                return FindResult::new_found(i, self.left);
            }
        }

        generic_btree::FindResult {
            index: elements.len(),
            offset: last_left,
            found: false,
        }
    }
}

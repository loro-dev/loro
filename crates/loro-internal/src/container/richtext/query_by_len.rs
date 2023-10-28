use std::marker::PhantomData;

use generic_btree::{BTreeTrait, FindResult, Query};

use crate::utils::utf16::count_utf16_chars;

use super::richtext_state::{
    unicode_to_utf16_index, unicode_to_utf8_index, RichtextStateChunk, RichtextTreeTrait,
};

/// An easy way to implement [Query] by using key index
///
/// This query implementation will
/// - prefer next element when this element has zero length.
/// - prefer next element rather than previous element when the left is at the middle of two elements.
pub struct IndexQuery<T: QueryByLen<B>, B: BTreeTrait> {
    left: usize,
    _data: PhantomData<(T, B)>,
}

/// An easy way to implement [Query] by using key index
///
/// This query implementation will
/// - prefer next element when this element has zero length.
/// - prefer next element rather than previous element when the left is at the middle of two elements.
pub struct IndexQueryWithEntityIndex<T: QueryByLen<B>, B: BTreeTrait> {
    left: usize,
    entity_index: usize,
    _data: PhantomData<(T, B)>,
}

/// An easy way to implement [Query] by using key index
///
/// This query implementation will
/// - prefer next element when this element has zero length.
/// - prefer next element rather than previous element when the left is at the middle of two elements.
pub struct EntityIndexQueryWithEventIndex {
    left: usize,
    pub(super) event_index: usize,
}

impl<T: QueryByLen<B>, B: BTreeTrait> IndexQueryWithEntityIndex<T, B> {
    pub fn entity_index(&self) -> usize {
        self.entity_index
    }
}

/// The default query implementation will
///
/// - prefer next element when this element has zero length.
/// - prefer next element rather than previous element when the left is at the middle of two elements.
pub trait QueryByLen<B: BTreeTrait> {
    fn get_cache_entity_len(cache: &B::Cache) -> usize;
    fn get_cache_len(cache: &B::Cache) -> usize;
    fn get_elem_len(elem: &B::Elem) -> usize;
    fn get_offset_and_found(left: usize, elem: &B::Elem) -> (usize, bool);
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

    fn confirm_elem(
        &mut self,
        q: &Self::QueryArg,
        elem: &<B as BTreeTrait>::Elem,
    ) -> (usize, bool) {
        T::get_offset_and_found(self.left, elem)
    }
}

impl<T: QueryByLen<B>, B: BTreeTrait> Query<B> for IndexQueryWithEntityIndex<T, B> {
    type QueryArg = usize;

    fn init(target: &Self::QueryArg) -> Self {
        Self {
            left: *target,
            entity_index: 0,
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
                self.entity_index += T::get_cache_entity_len(&cache.cache);
                last_left = self.left;
                self.left -= len;
            } else {
                return FindResult::new_found(i, self.left);
            }
        }

        self.left = last_left;
        FindResult::new_missing(child_caches.len() - 1, last_left)
    }

    fn confirm_elem(
        &mut self,
        q: &Self::QueryArg,
        elem: &<B as BTreeTrait>::Elem,
    ) -> (usize, bool) {
        let (offset, found) = T::get_offset_and_found(self.left, elem);
        self.entity_index += offset;
        (offset, found)
    }
}

impl Query<RichtextTreeTrait> for EntityIndexQueryWithEventIndex {
    type QueryArg = usize;

    fn init(target: &Self::QueryArg) -> Self {
        Self {
            left: *target,
            event_index: 0,
        }
    }

    fn find_node(
        &mut self,
        _: &Self::QueryArg,
        child_caches: &[generic_btree::Child<RichtextTreeTrait>],
    ) -> generic_btree::FindResult {
        let mut last_left = self.left;
        let mut last_event_left = self.left;
        for (i, cache) in child_caches.iter().enumerate() {
            let len = cache.cache.entity_len as usize;
            if self.left >= len {
                last_event_left = self.event_index;
                self.event_index += cache.cache.event_len() as usize;
                last_left = self.left;
                self.left -= len;
            } else {
                return FindResult::new_found(i, self.left);
            }
        }

        self.left = last_left;
        self.event_index = last_event_left;
        FindResult::new_missing(child_caches.len() - 1, last_left)
    }

    fn confirm_elem(&mut self, q: &Self::QueryArg, elem: &RichtextStateChunk) -> (usize, bool) {
        let left = self.left;
        match elem {
            RichtextStateChunk::Text { unicode_len, text } => {
                if *unicode_len as usize >= left {
                    self.event_index += if cfg!(feature = "wasm") {
                        unicode_to_utf16_index(std::str::from_utf8(text).unwrap(), left).unwrap()
                    } else {
                        left
                    };
                    return (left, true);
                }

                self.event_index += if cfg!(feature = "wasm") {
                    count_utf16_chars(text)
                } else {
                    *unicode_len as usize
                };
                (left, false)
            }
            RichtextStateChunk::Style { .. } => {
                if left == 0 {
                    return (0, true);
                }

                (left, false)
            }
        }
    }
}

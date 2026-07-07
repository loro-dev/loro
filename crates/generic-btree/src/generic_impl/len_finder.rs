use std::fmt::Debug;

use thunderdome::Index;

use crate::rle::HasLength;
use crate::{BTreeTrait, FindResult, Query};

/// A generic length finder
pub struct LengthFinder {
    pub left: usize,
    pub slot: u8,
    pub parent: Option<Index>,
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

pub trait UseLengthFinder<B: BTreeTrait> {
    fn get_len(cache: &B::Cache) -> usize;
}

impl<Elem: HasLength + Debug, B: BTreeTrait<Elem = Elem> + UseLengthFinder<B>> Query<B>
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
        child_caches: &[crate::Child<B>],
    ) -> crate::FindResult {
        let mut last_left = self.left;
        let is_internal = child_caches.first().unwrap().is_internal();
        for (i, cache) in child_caches.iter().enumerate() {
            let len = B::get_len(&cache.cache);
            if self.left >= len {
                last_left = self.left;
                self.left -= len;
            } else {
                if is_internal {
                    self.parent = Some(cache.arena.unwrap());
                } else {
                    self.slot = i as u8;
                }
                return FindResult::new_found(i, self.left);
            }
        }

        self.left = last_left;
        if is_internal {
            self.parent = Some(child_caches.last().unwrap().arena.unwrap());
        } else {
            self.slot = child_caches.len() as u8 - 1;
        }
        FindResult::new_missing(child_caches.len() - 1, last_left)
    }

    #[inline(always)]
    fn confirm_elem(
        &mut self,
        _: &Self::QueryArg,
        elem: &<B as BTreeTrait>::Elem,
    ) -> (usize, bool) {
        (self.left, self.left < elem.rle_len())
    }
}

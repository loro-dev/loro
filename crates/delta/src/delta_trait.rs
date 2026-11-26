use generic_btree::rle::{HasLength, Mergeable, Sliceable, TryInsert};
use std::hash::{BuildHasher, Hash};
use std::{collections::HashMap, fmt::Debug};

pub trait DeltaValue:
    HasLength + Sliceable + Mergeable + TryInsert + Debug + Clone + Default
{
}

pub trait DeltaAttr: Clone + PartialEq + Debug + Default {
    fn compose(&mut self, other: &Self);
    fn attr_is_empty(&self) -> bool;
    /// Transform the attributes of a retain in `self` against a concurrent retain's
    /// attributes `other` over the same range.
    ///
    /// When `left_prior` is false, `other` wins: every key it sets is removed from
    /// `self`, as concurrent map writes are transformed. The default keeps `self`
    /// unchanged, which was the behavior before this hook existed.
    fn transform(&mut self, _other: &Self, _left_prior: bool) {}
}

mod implementations {
    use super::*;

    impl DeltaAttr for () {
        fn compose(&mut self, _other: &Self) {}
        fn attr_is_empty(&self) -> bool {
            true
        }
    }

    impl<K, V, S> DeltaAttr for HashMap<K, V, S>
    where
        K: Eq + Hash + Debug + Clone,
        V: Debug + PartialEq + Clone,
        S: BuildHasher + Default + Clone,
    {
        fn compose(&mut self, other: &Self) {
            for (key, value) in other {
                self.insert(key.clone(), value.clone());
            }
        }

        fn attr_is_empty(&self) -> bool {
            self.is_empty()
        }

        fn transform(&mut self, other: &Self, left_prior: bool) {
            if !left_prior {
                for key in other.keys() {
                    self.remove(key);
                }
            }
        }
    }
}

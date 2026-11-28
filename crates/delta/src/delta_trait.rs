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
    }
}

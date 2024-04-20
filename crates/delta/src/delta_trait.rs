use std::fmt::Debug;

use generic_btree::rle::{HasLength, Mergeable, Sliceable, TryInsert};

pub trait DeltaValue: HasLength + Sliceable + Mergeable + TryInsert + Debug + Clone {}

pub trait DeltaAttr: Clone + PartialEq + Debug + Default {
    fn merge(&mut self, other: &Self);
    fn attr_is_empty(&self) -> bool;
}

impl DeltaAttr for () {
    fn merge(&mut self, _other: &Self) {}
    fn attr_is_empty(&self) -> bool {
        true
    }
}

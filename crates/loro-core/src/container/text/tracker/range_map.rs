use std::{fmt::Debug};


use num::{FromPrimitive};
use rle::{
    rle_tree::{
        tree_trait::{GlobalIndex, GlobalTreeTrait, HasGlobalIndex},
    },
    HasLength, Mergable, Rle, RleTree, Sliceable,
};



#[derive(Debug)]
struct WithGlobalIndex<Value, Index: GlobalIndex> {
    value: Value,
    index: Index,
}

impl<Value: Rle, Index: GlobalIndex> HasLength for WithGlobalIndex<Value, Index> {
    fn len(&self) -> usize {
        self.value.len()
    }
}

impl<Value: Rle, Index: GlobalIndex> Sliceable for WithGlobalIndex<Value, Index> {
    fn slice(&self, from: usize, to: usize) -> Self {
        Self {
            value: self.value.slice(from, to),
            index: self.index,
        }
    }
}

impl<Value: Rle, Index: GlobalIndex> Mergable for WithGlobalIndex<Value, Index> {
    fn is_mergable(&self, other: &Self, conf: &()) -> bool {
        self.value.is_mergable(&other.value, conf)
            && self.index + Index::from_usize(self.value.len()).unwrap() == other.index
    }

    fn merge(&mut self, other: &Self, conf: &()) {
        self.value.merge(&other.value, conf)
    }
}

impl<Value: Rle, Index: GlobalIndex> HasGlobalIndex for WithGlobalIndex<Value, Index> {
    type Int = Index;

    fn get_global_start(&self) -> Self::Int {
        self.index
    }
}

pub(super) struct RangeMap<Index: GlobalIndex + 'static, Value: Rle + 'static> {
    tree:
        RleTree<WithGlobalIndex<Value, Index>, GlobalTreeTrait<WithGlobalIndex<Value, Index>, 10>>,
}

impl<Index: GlobalIndex + 'static, Value: Rle + 'static> Default for RangeMap<Index, Value> {
    fn default() -> Self {
        Self {
            tree: Default::default(),
        }
    }
}

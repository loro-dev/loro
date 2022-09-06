use std::fmt::Debug;

use crate::{
    rle_tree::tree_trait::{GlobalIndex, GlobalTreeTrait, HasGlobalIndex},
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

#[repr(transparent)]
pub struct RangeMap<Index: GlobalIndex + 'static, Value: Rle + 'static> {
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

impl<Index: GlobalIndex + 'static, Value: Rle + 'static> RangeMap<Index, Value> {
    #[inline]
    pub fn insert(&mut self, start: Index, value: Value) {
        self.tree.with_tree_mut(|tree| {
            tree.delete_range(
                Some(start),
                Some(start + Index::from_usize(value.len()).unwrap()),
            );
            tree.insert(
                start,
                WithGlobalIndex {
                    value,
                    index: start,
                },
            );
        });
    }

    #[inline]
    pub fn delete(&mut self, start: Option<Index>, end: Option<Index>) {
        self.tree.with_tree_mut(|tree| {
            tree.delete_range(start, end);
        });
    }

    #[inline]
    pub fn get_range(&self, start: Index, end: Index) -> Vec<&Value> {
        let mut ans = Vec::new();
        self.tree.with_tree(|tree| {
            for value in tree.iter_range(start, Some(end)) {
                ans.push(&value.value)
            }
        });
        ans
    }

    #[inline]
    pub fn get(&self, index: Index) -> &Value {
        &self
            .tree
            .with_tree(|tree| tree.iter_range(index, None).next())
            .unwrap()
            .value
    }
}

#[cfg(test)]
mod test {
    use super::*;
    #[derive(Debug)]
    struct V(usize);
    impl HasLength for V {
        fn len(&self) -> usize {
            self.0
        }
    }
    impl Mergable for V {}
    impl Sliceable for V {
        fn slice(&self, from: usize, to: usize) -> Self {
            V(to - from)
        }
    }

    type VRangeMap = RangeMap<usize, V>;

    #[test]
    fn test_0() {
        let mut map: VRangeMap = Default::default();
        map.insert(10, V(10));
        map.insert(12, V(2));
        println!("{:#?}", map.get_range(10, 20));
    }
}

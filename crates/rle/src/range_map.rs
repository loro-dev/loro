use std::fmt::Debug;

use crate::{
    rle_trait::ZeroElement,
    rle_tree::tree_trait::{GlobalIndex, GlobalTreeTrait, HasGlobalIndex},
    HasLength, Mergable, Rle, RleTree, Sliceable,
};

#[derive(Debug, Clone)]
pub(crate) struct WithGlobalIndex<Value, Index: GlobalIndex> {
    pub(crate) value: Value,
    pub(crate) index: Index,
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
            index: self.index + Index::from_usize(from).unwrap(),
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
#[derive(Debug)]
pub struct RangeMap<Index: GlobalIndex + 'static, Value: Rle + ZeroElement + 'static> {
    pub(crate) tree:
        RleTree<WithGlobalIndex<Value, Index>, GlobalTreeTrait<WithGlobalIndex<Value, Index>, 10>>,
}

impl<Index: GlobalIndex + 'static, Value: Rle + ZeroElement + 'static> Default
    for RangeMap<Index, Value>
{
    fn default() -> Self {
        Self {
            tree: Default::default(),
        }
    }
}

impl<Index: GlobalIndex + 'static, Value: Rle + ZeroElement + 'static> RangeMap<Index, Value> {
    pub fn set(&mut self, start: Index, value: Value) {
        self.tree.delete_range(
            Some(start),
            Some(start + Index::from_usize(std::cmp::max(value.len(), 1)).unwrap()),
        );
        self.tree.insert(
            start,
            WithGlobalIndex {
                value,
                index: start,
            },
        );
    }

    #[inline]
    pub fn debug_check(&mut self) {
        self.tree.debug_check()
    }

    #[inline]
    pub fn delete(&mut self, start: Option<Index>, end: Option<Index>) {
        self.tree.delete_range(start, end);
    }

    #[inline]
    pub fn get_range(&self, start: Index, end: Index) -> Vec<&Value> {
        let mut ans = Vec::new();
        for value in self.tree.iter_range(start, Some(end)) {
            ans.push(&value.as_tree_ref().value)
        }
        ans
    }

    #[inline]
    pub fn get_range_with_index(&self, start: Index, end: Index) -> Vec<(Index, &Value)> {
        let mut ans = Vec::new();
        for value in self.tree.iter_range(start, Some(end)) {
            let value = value.as_tree_ref();
            ans.push((value.index, &value.value));
        }

        ans
    }

    #[inline]
    pub fn get(&self, index: Index) -> Option<&Value> {
        let cursor = self.tree.get(index);
        if let Some(cursor) = cursor {
            match cursor.pos() {
                crate::rle_tree::Position::Before
                | crate::rle_tree::Position::End
                | crate::rle_tree::Position::After => None,
                crate::rle_tree::Position::Start | crate::rle_tree::Position::Middle => {
                    Some(&cursor.as_tree_ref().value)
                }
            }
        } else {
            None
        }
    }

    #[inline]
    pub fn has(&self, index: Index) -> bool {
        self.get(index).is_some()
    }

    #[inline]
    pub fn clear(&mut self) {
        self.tree = Default::default();
    }
}

#[derive(Debug, Clone)]
pub struct WithStartEnd<Index: GlobalIndex, T> {
    pub start: Index,
    pub end: Index,
    pub value: T,
}

impl<Index: GlobalIndex, T: Clone> WithStartEnd<Index, T> {
    #[inline]
    pub fn new(start: Index, end: Index, value: T) -> Self {
        Self { start, end, value }
    }
}

impl<Index: GlobalIndex, T: Sliceable> Sliceable for WithStartEnd<Index, T> {
    fn slice(&self, from: usize, to: usize) -> Self {
        Self {
            start: self.start + Index::from_usize(from).unwrap(),
            end: Index::min(self.end, self.start + Index::from_usize(to).unwrap()),
            value: self.value.slice(from, to),
        }
    }
}

impl<Index: GlobalIndex, T> HasLength for WithStartEnd<Index, T> {
    fn len(&self) -> usize {
        Index::as_(self.end - self.start)
    }
}

impl<Index: GlobalIndex, T: ZeroElement> ZeroElement for WithStartEnd<Index, T> {
    fn zero_element() -> Self {
        Self {
            start: Index::from_usize(0).unwrap(),
            end: Index::from_usize(0).unwrap(),
            value: T::zero_element(),
        }
    }
}

impl<Index: GlobalIndex, T: PartialEq + Eq> Mergable for WithStartEnd<Index, T> {
    fn is_mergable(&self, other: &Self, _conf: &()) -> bool
    where
        Self: Sized,
    {
        self.end == other.start && self.value == other.value
    }

    fn merge(&mut self, other: &Self, _conf: &())
    where
        Self: Sized,
    {
        self.end = other.end;
    }
}

#[cfg(test)]
mod test {
    use std::ops::Range;

    use super::*;
    #[derive(Debug, PartialEq, Eq, Clone)]
    struct V {
        from: usize,
        to: usize,
    }

    impl V {
        fn new(from: usize, to: usize) -> Self {
            Self { from, to }
        }
    }
    impl HasLength for V {
        fn len(&self) -> usize {
            self.to - self.from
        }
    }
    impl Mergable for V {}
    impl Sliceable for V {
        fn slice(&self, from: usize, to: usize) -> Self {
            V {
                from: self.from + from,
                to: self.from + to,
            }
        }
    }

    impl ZeroElement for V {
        fn zero_element() -> Self {
            Self { from: 0, to: 0 }
        }
    }

    type VRangeMap = RangeMap<usize, V>;

    #[test]
    fn test_0() {
        let mut map: VRangeMap = Default::default();
        map.set(10, V::new(10, 20));
        map.set(12, V::new(12, 15));
        // 10-12, 12-15, 15-20
        assert_eq!(map.get_range(7, 8), Vec::<&V>::new());
        assert_eq!(map.get_range(8, 12), vec![&V::new(10, 12)]);
        assert_eq!(
            map.get_range(14, 16),
            vec![&V::new(12, 15), &V::new(15, 20)]
        );

        // 10-11, 11-12, 12-15, 15-20
        map.set(11, V::new(11, 12));
        assert_eq!(
            map.get_range(9, 15),
            vec![&V::new(10, 11), &V::new(11, 12), &V::new(12, 15)]
        );

        // 5-20
        map.set(5, V::new(5, 20));
        assert_eq!(map.get_range(9, 15), vec![&V::new(5, 20)]);
    }

    static_assertions::assert_not_impl_any!(RangeMap<usize, Range<usize>>: Sync, Send);
}

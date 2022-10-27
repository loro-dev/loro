use bumpalo::boxed::Box as BumpBox;
use std::{cell::UnsafeCell, fmt::Debug, ptr::NonNull};

use fxhash::FxHashSet;

use crate::{
    rle_trait::{GlobalIndex, HasIndex, ZeroElement},
    rle_tree::{
        node::{InternalNode, LeafNode},
        tree_trait::GlobalTreeTrait,
        Position, UnsafeCursor,
    },
    HasLength, Mergable, Rle, RleTree, Sliceable,
};

#[derive(Debug, Clone)]
pub(crate) struct WithIndex<Value, Index: GlobalIndex> {
    pub(crate) value: Value,
    pub(crate) index: Index,
}

impl<Value: Rle, Index: GlobalIndex> HasLength for WithIndex<Value, Index> {
    fn content_len(&self) -> usize {
        self.value.content_len()
    }
}

impl<Value: Rle, Index: GlobalIndex> Sliceable for WithIndex<Value, Index> {
    fn slice(&self, from: usize, to: usize) -> Self {
        Self {
            value: self.value.slice(from, to),
            index: self.index + Index::from_usize(from).unwrap(),
        }
    }
}

impl<Value: Rle, Index: GlobalIndex> Mergable for WithIndex<Value, Index> {
    fn is_mergable(&self, other: &Self, conf: &()) -> bool {
        self.value.is_mergable(&other.value, conf)
            && self.index + Index::from_usize(self.value.content_len()).unwrap() == other.index
    }

    fn merge(&mut self, other: &Self, conf: &()) {
        self.value.merge(&other.value, conf)
    }
}

impl<Value: Rle, Index: GlobalIndex> HasIndex for WithIndex<Value, Index> {
    type Int = Index;

    fn get_start_index(&self) -> Self::Int {
        self.index
    }
}

#[repr(transparent)]
#[derive(Debug)]
pub struct RangeMap<Index: GlobalIndex + 'static, Value: Rle + ZeroElement + 'static> {
    pub(crate) tree: RleTree<WithIndex<Value, Index>, GlobalTreeTrait<WithIndex<Value, Index>, 10>>,
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
    pub fn set_large_range(&mut self, start: Index, value: Value) {
        let end = start + Index::from_usize(std::cmp::max(value.content_len(), 1)).unwrap();
        self.tree.delete_range(Some(start), Some(end));
        self.tree.insert(
            start,
            WithIndex {
                value,
                index: start,
            },
        );
    }

    /// In our use cases, most of the set operation is at small range.
    /// So we can travel from the first cursor to modify each element gradually
    pub fn set_small_range(&mut self, start: Index, value: Value) {
        let end = start + Index::from_usize(std::cmp::max(value.atom_len(), 1)).unwrap();
        let cursor = self.tree.get_cursor_ge(start);
        if cursor.is_none() {
            self.tree.insert(
                start,
                WithIndex {
                    value,
                    index: start,
                },
            );
            return;
        }

        let mut cursor = cursor.unwrap();
        // SAFETY: we have exclusive ref to the tree
        let mut cur_leaf = unsafe { cursor.0.leaf.as_mut() };
        let cur_ptr = cur_leaf.into();
        let mut index = cursor.0.index;
        let mut elem = &mut cur_leaf.children[index];
        let elem_end = elem.index + Index::from_usize(elem.atom_len()).unwrap();
        // there are a lot of updates are in-place, we can update them directly and return
        // because cache won't change
        if elem.index == start && elem_end == end {
            **elem = WithIndex {
                value,
                index: start,
            };
            return;
        }

        if elem.index >= end {
            // there is no elements inside the target range, we can insert directly
            self.tree.insert(
                start,
                WithIndex {
                    value,
                    index: start,
                },
            );
            return;
        }

        if elem.index < start && end < elem_end {
            // element contains the target range
            let offset = (start - elem.index).as_();
            let leaf: NonNull<_> = cur_leaf.into();

            self.tree.update_at_cursors(
                &mut [UnsafeCursor {
                    // SAFETY: ignore lifetime to bypass tree mut borrow check
                    leaf: unsafe { std::mem::transmute(leaf) },
                    index,
                    offset,
                    pos: crate::rle_tree::Position::Middle,
                    len: (end - start).as_(),
                }],
                &mut |v| {
                    v.value = value.clone();
                },
                &mut |_, _| {},
            );

            return;
        }

        let mut visited_nodes: FxHashSet<NonNull<LeafNode<_, _>>> = FxHashSet::default();
        visited_nodes.insert(cur_ptr);
        let mut last_end: Index = start;
        let mut last_inside_element: Option<NonNull<_>> = None;
        // iterate over the elements inside the range
        loop {
            if elem.index >= end {
                break;
            }

            let elem_end = elem.index + Index::from_usize(elem.atom_len()).unwrap();
            if start > elem_end {
                debug_assert!(false, "something wrong with get_cursor_ge")
                // go to next loop
            } else if elem.index < start {
                // start element overlaps with target range
                // let it keep its left part
                **elem = elem.slice(0, (start - elem.index).as_());
            } else if elem_end > end {
                // end element overlaps with target range
                // let it keep its right part
                **elem = elem.slice((end - elem.index).as_(), elem.atom_len());
            } else {
                // elements inside the target range
                // extends its start to last_end
                **elem = WithIndex {
                    index: last_end,
                    value: value.slice((last_end - start).as_(), (elem_end - start).as_()),
                };
                last_inside_element = Some(elem.into());
                last_end = elem_end;
            }

            // move to next element
            if index + 1 < cur_leaf.children().len() {
                index += 1;
                elem = &mut cur_leaf.children[index];
            } else {
                if let Some(next) = cur_leaf.next_mut() {
                    visited_nodes.insert(next.into());
                    cur_leaf = next;
                } else {
                    // is the last element of the tree
                    break;
                }

                index = 0;
                elem = &mut cur_leaf.children[index];
            }
        }

        if last_end != end {
            if let Some(mut insider) = last_inside_element {
                // we can extended the last element to the end
                // SAFETY: we just got the element from the tree and save it to the option value
                let insider = unsafe { insider.as_mut() };
                insider.value = value.slice((insider.index - start).as_(), (end - start).as_());
                last_end = end;
            }
        }

        let mut visited_internal_nodes: FxHashSet<NonNull<InternalNode<_, _>>> =
            FxHashSet::default();
        for mut leaf in visited_nodes {
            // SAFETY: we have exclusive ref to the tree
            let leaf = unsafe { leaf.as_mut() };
            leaf.update_cache();
            visited_internal_nodes.insert(leaf.parent);
        }

        while !visited_internal_nodes.is_empty() {
            for mut internal in std::mem::take(&mut visited_internal_nodes) {
                // SAFETY: we have exclusive ref to the tree
                let internal = unsafe { internal.as_mut() };
                internal.update_cache();
                if let Some(parent) = internal.parent {
                    visited_internal_nodes.insert(parent);
                }
            }
        }

        if last_end != end {
            // TODO: Can be optimized?
            // need to insert a new element from here
            // current pointer must be greater than start or at the end of the tree
            self.tree.insert(
                last_end,
                WithIndex {
                    value: value.slice((last_end - start).as_(), (end - start).as_()),
                    index: last_end,
                },
            );
        }
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

    /// TODO: need double check this method
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
    fn content_len(&self) -> usize {
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
        key: String,
    }

    impl V {
        fn new(from: usize, to: usize, key: &str) -> Self {
            Self {
                from,
                to,
                key: key.into(),
            }
        }
    }
    impl HasLength for V {
        fn content_len(&self) -> usize {
            self.to - self.from
        }
    }
    impl Mergable for V {}
    impl Sliceable for V {
        fn slice(&self, from: usize, to: usize) -> Self {
            V {
                from: self.from + from,
                to: self.from + to,
                key: self.key.clone(),
            }
        }
    }

    impl ZeroElement for V {
        fn zero_element() -> Self {
            Self {
                from: 0,
                to: 0,
                key: "".to_string(),
            }
        }
    }

    type VRangeMap = RangeMap<usize, V>;

    #[test]
    fn test_0() {
        let mut map: VRangeMap = Default::default();
        map.set_large_range(10, V::new(10, 20, "a"));
        map.set_large_range(12, V::new(12, 15, "b"));
        // 10-12, 12-15, 15-20
        assert_eq!(map.get_range(7, 8), Vec::<&V>::new());
        assert_eq!(map.get_range(8, 12), vec![&V::new(10, 12, "a")]);
        assert_eq!(
            map.get_range(14, 16),
            vec![&V::new(12, 15, "b"), &V::new(15, 20, "a")]
        );

        // 10-11, 11-12, 12-15, 15-20
        map.set_large_range(11, V::new(11, 12, "c"));
        assert_eq!(
            map.get_range(9, 15),
            vec![
                &V::new(10, 11, "a"),
                &V::new(11, 12, "c"),
                &V::new(12, 15, "b")
            ]
        );

        // 5-20
        map.set_large_range(5, V::new(5, 20, "k"));
        assert_eq!(map.get_range(9, 15), vec![&V::new(5, 20, "k")]);
    }

    #[test]
    fn test_small_range() {
        let mut map: VRangeMap = Default::default();
        map.set_small_range(10, V::new(10, 20, "a"));
        map.set_small_range(12, V::new(12, 15, "b"));
        // 10-12, 12-15, 15-20
        assert_eq!(map.get_range(7, 8), Vec::<&V>::new());
        assert_eq!(map.get_range(8, 12), vec![&V::new(10, 12, "a")]);
        assert_eq!(
            map.get_range(14, 16),
            vec![&V::new(12, 15, "b"), &V::new(15, 20, "a")]
        );

        // 10-11, 11-12, 12-15, 15-20
        map.set_small_range(11, V::new(11, 12, "c"));
        assert_eq!(
            map.get_range(9, 15),
            vec![
                &V::new(10, 11, "a"),
                &V::new(11, 12, "c"),
                &V::new(12, 15, "b")
            ]
        );

        // 5-20
        map.set_small_range(5, V::new(5, 20, "k"));
        assert_eq!(
            map.get_range(9, 15),
            vec![
                &V::new(5, 11, "k"),
                &V::new(11, 12, "k"),
                &V::new(12, 15, "k"),
            ]
        );
    }

    #[test]
    fn test_small_range_across_spans() {
        let mut map: VRangeMap = Default::default();
        map.set_small_range(0, V::new(0, 5, "a"));
        map.set_small_range(10, V::new(10, 15, "b"));
        map.set_small_range(20, V::new(20, 25, "c"));
        map.set_small_range(2, V::new(2, 23, "k"));
        assert_eq!(
            map.get_range(0, 30),
            vec![
                &V::new(0, 2, "a"),
                &V::new(2, 23, "k"),
                &V::new(23, 25, "c"),
            ]
        )
    }

    #[test]
    fn test_small_range_inside() {
        let mut map: VRangeMap = Default::default();
        map.set_small_range(0, V::new(0, 5, "a"));
        map.set_small_range(1, V::new(1, 3, "b"));
        assert_eq!(
            map.get_range(0, 30),
            vec![&V::new(0, 1, "a"), &V::new(1, 3, "b"), &V::new(3, 5, "a"),]
        );

        let mut map: VRangeMap = Default::default();
        map.set_small_range(0, V::new(0, 5, "a"));
        map.set_small_range(0, V::new(0, 3, "b"));
        assert_eq!(
            map.get_range(0, 30),
            vec![&V::new(0, 3, "b"), &V::new(3, 5, "a"),]
        );

        let mut map: VRangeMap = Default::default();
        map.set_small_range(0, V::new(0, 5, "a"));
        map.set_small_range(3, V::new(3, 5, "b"));
        assert_eq!(
            map.get_range(0, 30),
            vec![&V::new(0, 3, "a"), &V::new(3, 5, "b"),]
        );
        map.set_small_range(3, V::new(3, 6, "c"));
        assert_eq!(
            map.get_range(0, 30),
            vec![&V::new(0, 3, "a"), &V::new(3, 6, "c")]
        );
    }

    static_assertions::assert_not_impl_any!(RangeMap<usize, Range<usize>>: Sync, Send);
}

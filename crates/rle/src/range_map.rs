use std::{fmt::Debug, ptr::NonNull};

use fxhash::{FxHashMap, FxHashSet};

use crate::{
    rle_trait::{GlobalIndex, HasIndex, ZeroElement},
    rle_tree::{
        node::{InternalNode, LeafNode},
        tree_trait::GlobalTreeTrait,
        Arena, HeapMode, UnsafeCursor, VecTrait,
    },
    HasLength, Mergable, Rle, RleTree, Sliceable,
};

const MAX_CHILDREN_SIZE: usize = 32;
type RangeMapTrait<Index, Value, TreeArena> =
    GlobalTreeTrait<WithIndex<Value, Index>, MAX_CHILDREN_SIZE, TreeArena>;

#[repr(transparent)]
#[derive(Debug)]
pub struct RangeMap<
    Index: GlobalIndex + 'static,
    Value: Rle + ZeroElement + 'static,
    TreeArena: Arena + 'static = HeapMode,
> {
    pub(crate) tree: RleTree<WithIndex<Value, Index>, RangeMapTrait<Index, Value, TreeArena>>,
}

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

impl<
        Index: GlobalIndex + 'static,
        Value: Rle + ZeroElement + 'static,
        TreeArena: Arena + 'static,
    > Default for RangeMap<Index, Value, TreeArena>
{
    fn default() -> Self {
        Self {
            tree: Default::default(),
        }
    }
}

impl<
        Index: GlobalIndex + 'static,
        Value: Rle + ZeroElement + 'static,
        TreeArena: Arena + 'static,
    > RangeMap<Index, Value, TreeArena>
{
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
        #[allow(clippy::type_complexity)]
        let cursor = self.tree.get_cursor_ge_mut(start);
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
        let mut cur_ptr = cur_leaf.into();
        let mut index = cursor.0.index;
        let mut iter_children = cur_leaf.children.iter_mut().skip(index);
        let mut elem = iter_children.next().unwrap();
        let elem_end = elem.index + Index::from_usize(elem.atom_len()).unwrap();
        // there are a lot of updates are in-place, we can update them directly and return
        // because cache won't change
        if elem.index == start && elem_end == end {
            *elem = WithIndex {
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
            #[allow(clippy::type_complexity)]
            let leaf: NonNull<
                LeafNode<'_, WithIndex<Value, Index>, RangeMapTrait<Index, Value, TreeArena>>,
                // SAFETY: ignore lifetime to bypass tree mut borrow check
            > = unsafe { std::mem::transmute(leaf) };

            self.tree.update_at_cursors(
                &mut [UnsafeCursor {
                    leaf,
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

        #[derive(Default, Debug)]
        struct Data {
            delete_start: Option<usize>,
            delete_end: Option<usize>,
        }

        let mut visited_nodes: FxHashMap<NonNull<LeafNode<_, _>>, Data> = Default::default();
        let mut cur_data: Data = Default::default();
        let mut last_inside_element: Option<NonNull<_>> = None;
        // iterate over the elements inside the range
        loop {
            if elem.index >= end {
                visited_nodes.insert(cur_leaf.into(), cur_data);
                break;
            }

            let elem_end = elem.index + Index::from_usize(elem.atom_len()).unwrap();
            if start > elem_end {
                // go to next loop
            } else if elem.index < start {
                // start element overlaps with target range
                // let it keep its left part
                let new_len = (start - elem.index).as_();
                *elem = elem.slice(0, new_len);
            } else if elem_end > end {
                // end element overlaps with target range
                // let it keep its right part
                let start = (end - elem.index).as_();
                *elem = elem.slice(start, elem.atom_len());
                visited_nodes.insert(cur_ptr, cur_data);
                break;
            } else {
                // elements inside the target range
                // extends its start to last_end
                if last_inside_element.is_none() {
                    last_inside_element = Some(elem.into());
                } else {
                    cur_data.delete_start.get_or_insert(index);
                    cur_data.delete_end = Some(index + 1);
                }
            }

            // move to next element
            if let Some(next) = iter_children.next() {
                index += 1;
                elem = next;
            } else {
                if let Some(next) = cur_leaf.next_mut() {
                    visited_nodes.insert(cur_ptr, cur_data);
                    cur_ptr = next.into();
                    cur_data = Default::default();
                    cur_leaf = next;
                    iter_children = cur_leaf.children.iter_mut().skip(0);
                } else {
                    visited_nodes.insert(cur_ptr, cur_data);
                    // is the last element of the tree
                    break;
                }

                index = 0;
                elem = iter_children.next().unwrap();
            }
        }

        if let Some(mut insider) = last_inside_element {
            // we can extended the last element to the end
            // SAFETY: we just got the element from the tree and save it to the option value
            let insider = unsafe { insider.as_mut() };
            insider.index = start;
            insider.value = value;
        } else {
            // need to insert a new element from here
            // current pointer must be greater than start or at the end of the tree
            // SAFETY: we just visited cursor
            unsafe {
                let cursor: UnsafeCursor<_, RangeMapTrait<Index, Value, TreeArena>> =
                    UnsafeCursor::new(cur_ptr, index, 0, crate::rle_tree::Position::Start, 0);
                let last_item = cursor.as_ref();
                if last_item.index >= end {
                    let value = WithIndex {
                        value,
                        index: start,
                    };
                    cursor.insert_notify(value, &mut |_, _| {});
                } else if last_item.get_end_index() <= start {
                    // current pointer points to the end of the tree
                    let cursor: UnsafeCursor<_, RangeMapTrait<Index, Value, TreeArena>> =
                        cursor.shift(last_item.atom_len()).unwrap();
                    cursor.insert_notify(
                        WithIndex {
                            value,
                            index: start,
                        },
                        &mut |_, _| {},
                    );
                } else {
                    unreachable!()
                }
            }
        }

        let mut visited_internal_nodes: FxHashSet<NonNull<InternalNode<_, _>>> =
            FxHashSet::with_capacity_and_hasher(visited_nodes.len(), Default::default());
        for (mut leaf, data) in visited_nodes {
            // SAFETY: we have exclusive ref to the tree
            let leaf = unsafe { leaf.as_mut() };
            if let (Some(start), Some(end)) = (data.delete_start, data.delete_end) {
                leaf.children.drain(start..end);
            }
            leaf.update_cache();
            visited_internal_nodes.insert(leaf.parent);
        }

        while !visited_internal_nodes.is_empty() {
            let len = visited_internal_nodes.len();
            for mut internal in std::mem::replace(
                &mut visited_internal_nodes,
                FxHashSet::with_capacity_and_hasher(len, Default::default()),
            ) {
                // SAFETY: we have exclusive ref to the tree
                let internal = unsafe { internal.as_mut() };
                let mut del_start = None;
                let mut del_end = None;
                for i in 0..internal.children().len() {
                    let child = &internal.children()[i];
                    if child.node.is_empty() {
                        del_start.get_or_insert(i);
                        del_end = Some(i + 1);
                    }
                }

                if let (Some(start), Some(end)) = (del_start, del_end) {
                    internal.drain_children(start, end);
                }

                internal.update_cache(None);
                if let Some(parent) = internal.parent {
                    visited_internal_nodes.insert(parent);
                }
            }
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

    /// Return the values overlap with the given range
    ///
    /// Note that the returned values may exceed the given range
    #[inline]
    pub fn get_range(&self, start: Index, end: Index) -> impl Iterator<Item = &Value> {
        self.iter_range(start, end).map(|(_, b)| b)
    }

    /// Return the values overlap with the given range and their indexes
    ///
    /// Note that the returned values may exceed the given range
    ///
    /// TODO: need double check this method
    #[inline]
    pub fn get_range_with_index(
        &self,
        start: Index,
        end: Index,
    ) -> impl Iterator<Item = (Index, &Value)> {
        self.iter_range(start, end)
    }

    /// Return the values contained by the given range, the returned values are sliced by the given range
    #[inline]
    pub fn get_range_sliced(
        &self,
        start: Index,
        end: Index,
    ) -> impl Iterator<Item = (Index, Value)> + '_ {
        self.tree.iter_range(start, Some(end)).map(|x| {
            let sliced = x.get_sliced();
            (sliced.index, sliced.value)
        })
    }

    #[inline]
    pub fn get_mut(&mut self, index: Index) -> Option<&mut Value> {
        let cursor = self.tree.get_mut(index);
        if let Some(mut cursor) = cursor {
            match cursor.pos() {
                crate::rle_tree::Position::Before
                | crate::rle_tree::Position::End
                | crate::rle_tree::Position::After => None,
                crate::rle_tree::Position::Start | crate::rle_tree::Position::Middle => {
                    Some(&mut cursor.as_tree_mut().value)
                }
            }
        } else {
            None
        }
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

    pub fn iter_range(&self, start: Index, end: Index) -> impl Iterator<Item = (Index, &Value)> {
        let mut cursor = if start < end {
            self.tree.get_cursor_ge(start)
        } else {
            None
        };
        std::iter::from_fn(move || loop {
            if let Some(inner) = std::mem::take(&mut cursor) {
                cursor = inner.next_elem_start();
                let item = inner.as_tree_ref();
                if item.get_end_index() <= start {
                    continue;
                } else if item.index >= end {
                    return None;
                } else {
                    let ans = (item.index, &item.value);
                    return Some(ans);
                }
            } else {
                return None;
            }
        })
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
        assert_eq!(map.get_range(7, 8).collect::<Vec<&V>>(), Vec::<&V>::new());
        assert_eq!(
            map.get_range(8, 12).collect::<Vec<&V>>(),
            vec![&V::new(10, 12, "a")]
        );
        assert_eq!(
            map.get_range(14, 16).collect::<Vec<&V>>(),
            vec![&V::new(12, 15, "b"), &V::new(15, 20, "a")]
        );

        // 10-11, 11-12, 12-15, 15-20
        map.set_large_range(11, V::new(11, 12, "c"));
        assert_eq!(
            map.get_range(9, 15).collect::<Vec<&V>>(),
            vec![
                &V::new(10, 11, "a"),
                &V::new(11, 12, "c"),
                &V::new(12, 15, "b")
            ]
        );

        // 5-20
        map.set_large_range(5, V::new(5, 20, "k"));
        assert_eq!(
            map.get_range(9, 15).collect::<Vec<&V>>(),
            vec![&V::new(5, 20, "k")]
        );
    }

    #[test]
    fn test_small_range() {
        let mut map: VRangeMap = Default::default();
        map.set_small_range(10, V::new(10, 20, "a"));
        map.set_small_range(12, V::new(12, 15, "b"));
        // 10-12, 12-15, 15-20
        assert_eq!(map.get_range(7, 8).collect::<Vec<&V>>(), Vec::<&V>::new());
        assert_eq!(
            map.get_range(8, 12).collect::<Vec<&V>>(),
            vec![&V::new(10, 12, "a")]
        );
        assert_eq!(
            map.get_range(14, 16).collect::<Vec<&V>>(),
            vec![&V::new(12, 15, "b"), &V::new(15, 20, "a")]
        );

        // 10-11, 11-12, 12-15, 15-20
        map.set_small_range(11, V::new(11, 12, "c"));
        assert_eq!(
            map.get_range(9, 15).collect::<Vec<&V>>(),
            vec![
                &V::new(10, 11, "a"),
                &V::new(11, 12, "c"),
                &V::new(12, 15, "b")
            ]
        );

        // 5-20
        map.set_small_range(5, V::new(5, 20, "k"));
        assert_eq!(
            map.get_range(9, 15).collect::<Vec<&V>>(),
            vec![&V::new(5, 20, "k"),]
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
            map.get_range(0, 30).collect::<Vec<&V>>(),
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
            map.get_range(0, 30).collect::<Vec<&V>>(),
            vec![&V::new(0, 1, "a"), &V::new(1, 3, "b"), &V::new(3, 5, "a"),]
        );

        let mut map: VRangeMap = Default::default();
        map.set_small_range(0, V::new(0, 5, "a"));
        map.set_small_range(0, V::new(0, 3, "b"));
        assert_eq!(
            map.get_range(0, 30).collect::<Vec<&V>>(),
            vec![&V::new(0, 3, "b"), &V::new(3, 5, "a"),]
        );

        let mut map: VRangeMap = Default::default();
        map.set_small_range(0, V::new(0, 5, "a"));
        map.set_small_range(3, V::new(3, 5, "b"));
        assert_eq!(
            map.get_range(0, 30).collect::<Vec<&V>>(),
            vec![&V::new(0, 3, "a"), &V::new(3, 5, "b"),]
        );
        map.set_small_range(3, V::new(3, 6, "c"));
        assert_eq!(
            map.get_range(0, 30).collect::<Vec<&V>>(),
            vec![&V::new(0, 3, "a"), &V::new(3, 6, "c")]
        );
    }

    static_assertions::assert_not_impl_any!(RangeMap<usize, Range<usize>>: Sync, Send);
}

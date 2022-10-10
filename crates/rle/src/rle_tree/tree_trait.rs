use std::fmt::Debug;

use num::{traits::AsPrimitive, FromPrimitive, Integer};

use crate::{HasLength, Rle};

use super::node::{InternalNode, LeafNode, Node};

/// The position relative to a certain node.
///
/// - The target may be inside a node, in which case it's at the start/middle/end of a node.
/// - Or it is before/after a node.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Position {
    Before,
    Start,
    Middle,
    // can after and end be merged together?
    End,
    After,
}

impl Position {
    #[inline]
    pub fn from_offset(offset: isize, len: usize) -> Self {
        if offset < 0 {
            Position::Before
        } else if offset == 0 {
            Position::Start
        } else if (offset as usize) < len {
            Position::Middle
        } else if offset as usize == len {
            Position::End
        } else {
            Position::After
        }
    }
}

pub struct FindPosResult<I> {
    pub child_index: usize,
    pub offset: I,
    pub pos: Position,
    pub found: bool,
}

impl<I> FindPosResult<I> {
    pub(crate) fn new(child_index: usize, offset: I, pos: Position) -> Self {
        FindPosResult {
            child_index,
            offset,
            pos,
            found: true,
        }
    }

    pub(crate) fn new_not_found(child_index: usize, new_search_index: I, pos: Position) -> Self {
        FindPosResult {
            child_index,
            offset: new_search_index,
            pos,
            found: false,
        }
    }
}

pub trait RleTreeTrait<T: Rle>: Sized + Debug {
    const MAX_CHILDREN_NUM: usize;
    const MIN_CHILDREN_NUM: usize = Self::MAX_CHILDREN_NUM / 2;
    type Int: num::Integer + Copy + Debug + FromPrimitive;
    type InternalCache: Default + Debug + Eq + Clone;
    type LeafCache: Default + Debug + Eq + Clone;

    fn update_cache_leaf(node: &mut LeafNode<'_, T, Self>);
    fn update_cache_internal(node: &mut InternalNode<'_, T, Self>);

    /// - `child_index` can only equal to children.len() when it's zero
    /// - We need the `offset` so we can perform `find_pos_internal(child, new_search_index)`.
    /// - We need the `pos` to determine whether the child is included or excluded
    /// - If index is at the end of an element, `found` should be true
    /// - If not found, then `found` should be false and `child_index` should be the index of the insert position
    fn find_pos_internal(
        node: &InternalNode<'_, T, Self>,
        index: Self::Int,
    ) -> FindPosResult<Self::Int>;

    /// - `child_index` can only equal to children.len() when it's zero
    /// - if `pos == Middle`, we need to split the node
    /// - We need the third arg to determine whether the child is included or excluded
    /// - If not found, then `found` should be false and `child_index` should be the index of the insert position
    /// - If index is at the end of an element, `found` should be true
    /// - If target index is after last child, then `child_index`  = children.len().wrapping_sub(1), `offset` = children.last().unwrap().len()
    fn find_pos_leaf(node: &LeafNode<'_, T, Self>, index: Self::Int) -> FindPosResult<usize>;

    fn len_leaf(node: &LeafNode<'_, T, Self>) -> Self::Int;
    fn len_internal(node: &InternalNode<'_, T, Self>) -> Self::Int;
    fn check_cache_leaf(_node: &LeafNode<'_, T, Self>) {}
    fn check_cache_internal(_node: &InternalNode<'_, T, Self>) {}
}

#[derive(Debug, Default)]
pub struct CumulateTreeTrait<T: Rle, const MAX_CHILD: usize> {
    _phantom: std::marker::PhantomData<T>,
}

#[derive(Debug, Default)]
pub struct GlobalTreeTrait<T: Rle, const MAX_CHILD: usize> {
    _phantom: std::marker::PhantomData<T>,
}

impl<T: Rle, const MAX_CHILD: usize> RleTreeTrait<T> for CumulateTreeTrait<T, MAX_CHILD> {
    const MAX_CHILDREN_NUM: usize = MAX_CHILD;

    const MIN_CHILDREN_NUM: usize = Self::MAX_CHILDREN_NUM / 2;

    type Int = usize;

    type InternalCache = usize;

    type LeafCache = usize;

    fn update_cache_leaf(node: &mut LeafNode<'_, T, Self>) {
        node.cache = node.children().iter().map(|x| HasLength::len(&**x)).sum();
    }

    fn update_cache_internal(node: &mut InternalNode<'_, T, Self>) {
        node.cache = node.children().iter().map(|x| Node::len(x)).sum();
    }

    fn find_pos_internal(
        node: &InternalNode<'_, T, Self>,
        mut index: Self::Int,
    ) -> FindPosResult<usize> {
        if node.children.is_empty() {
            return FindPosResult::new_not_found(0, 0, Position::Before);
        }

        let mut last_cache = 0;
        for (i, child) in node.children().iter().enumerate() {
            last_cache = match child {
                Node::Internal(x) => {
                    if index <= x.cache {
                        return FindPosResult::new(i, index, get_pos(index, child.len()));
                    }
                    x.cache
                }
                Node::Leaf(x) => {
                    if index <= x.cache {
                        return FindPosResult::new(i, index, get_pos(index, child.len()));
                    }
                    x.cache
                }
            };

            index -= last_cache;
        }

        if index > 0 {
            dbg!(&node);
            assert_eq!(index, 0);
        }
        FindPosResult::new(node.children().len() - 1, last_cache, Position::End)
    }

    fn find_pos_leaf(node: &LeafNode<'_, T, Self>, mut index: Self::Int) -> FindPosResult<usize> {
        if node.children.is_empty() {
            return FindPosResult::new_not_found(0, 0, Position::Before);
        }

        for (i, child) in node.children().iter().enumerate() {
            if index < HasLength::len(&**child) {
                return FindPosResult::new(i, index, get_pos(index, child.len()));
            }

            index -= HasLength::len(&**child);
        }

        FindPosResult::new(
            node.children().len() - 1,
            HasLength::len(&**node.children().last().unwrap()),
            Position::End,
        )
    }

    fn len_leaf(node: &LeafNode<'_, T, Self>) -> usize {
        node.cache
    }

    fn len_internal(node: &InternalNode<'_, T, Self>) -> usize {
        node.cache
    }

    fn check_cache_internal(node: &InternalNode<'_, T, Self>) {
        assert_eq!(node.cache, node.children().iter().map(|x| x.len()).sum());
    }

    fn check_cache_leaf(node: &LeafNode<'_, T, Self>) {
        assert_eq!(node.cache, node.children().iter().map(|x| x.len()).sum());
    }
}

#[inline]
fn get_pos(index: usize, len: usize) -> Position {
    if index == 0 {
        Position::Start
    } else if index == len {
        Position::End
    } else {
        Position::Middle
    }
}

pub trait GlobalIndex:
    Debug + Integer + Copy + Default + FromPrimitive + AsPrimitive<usize>
{
}

impl<T: Debug + Integer + Copy + Default + FromPrimitive + AsPrimitive<usize>> GlobalIndex for T {}

pub trait HasGlobalIndex: HasLength {
    type Int: GlobalIndex;
    fn get_global_start(&self) -> Self::Int;

    #[inline]
    fn get_global_end(&self) -> Self::Int {
        self.get_global_start() + Self::Int::from_usize(self.len()).unwrap()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Default, Copy)]
pub struct Cache<I> {
    start: I,
    end: I,
}

#[inline]
fn get_cache<T: Rle + HasGlobalIndex, const MAX_CHILD: usize>(
    node: &Node<'_, T, GlobalTreeTrait<T, MAX_CHILD>>,
) -> Cache<T::Int> {
    match node {
        Node::Internal(x) => x.cache,
        Node::Leaf(x) => x.cache,
    }
}

impl<T: Rle + HasGlobalIndex, const MAX_CHILD: usize> RleTreeTrait<T>
    for GlobalTreeTrait<T, MAX_CHILD>
{
    const MAX_CHILDREN_NUM: usize = MAX_CHILD;

    const MIN_CHILDREN_NUM: usize = Self::MAX_CHILDREN_NUM / 2;

    type Int = T::Int;

    type InternalCache = Cache<T::Int>;
    type LeafCache = Cache<T::Int>;

    fn update_cache_leaf(node: &mut LeafNode<'_, T, Self>) {
        if node.children.is_empty() {
            node.cache.end = node.cache.start;
            return;
        }

        node.cache.end = node
            .children()
            .iter()
            .map(|x| x.get_global_end())
            .max()
            .unwrap();
        node.cache.start = node.children()[0].get_global_start();
    }

    fn update_cache_internal(node: &mut InternalNode<'_, T, Self>) {
        if node.children.is_empty() {
            return;
        }

        node.cache.end = node
            .children()
            .iter()
            .map(|x| get_cache(x).end)
            .max()
            .unwrap();
        node.cache.start = get_cache(node.children()[0]).start;
    }

    fn find_pos_internal(
        node: &InternalNode<'_, T, Self>,
        index: Self::Int,
    ) -> FindPosResult<Self::Int> {
        for (i, child) in node.children().iter().enumerate() {
            let cache = get_cache(child);
            if index <= cache.end {
                if index < cache.start {
                    return FindPosResult::new_not_found(i, index, Position::Before);
                }

                // prefer Start than End
                if index == cache.end
                    && i + 1 < node.children.len()
                    && index == get_cache(node.children[i + 1]).start
                {
                    return FindPosResult::new(i + 1, index, Position::Start);
                }

                return FindPosResult::new(i, index, get_pos_global(index, cache));
            }
        }

        FindPosResult::new_not_found(
            node.children.len().saturating_sub(1),
            index,
            Position::After,
        )
    }

    fn find_pos_leaf(node: &LeafNode<'_, T, Self>, index: Self::Int) -> FindPosResult<usize> {
        for (i, child) in node.children().iter().enumerate() {
            let cache = Cache {
                start: child.get_global_start(),
                end: child.get_global_end(),
            };

            if index <= cache.end {
                if index < cache.start {
                    return FindPosResult::new_not_found(i, 0, Position::Before);
                }

                // prefer Start than End
                if index == cache.end
                    && i + 1 < node.children.len()
                    && index == node.children[i + 1].get_global_start()
                {
                    return FindPosResult::new(i + 1, 0, Position::Start);
                }

                return FindPosResult::new(
                    i,
                    (index - cache.start).as_(),
                    get_pos_global(index, cache),
                );
            }
        }

        FindPosResult::new_not_found(
            node.children.len().saturating_sub(1),
            node.children().last().unwrap().len(),
            Position::After,
        )
    }

    fn len_leaf(node: &LeafNode<'_, T, Self>) -> Self::Int {
        node.cache.end - node.cache.start
    }

    fn len_internal(node: &InternalNode<'_, T, Self>) -> Self::Int {
        node.cache.end - node.cache.start
    }

    fn check_cache_leaf(node: &LeafNode<'_, T, Self>) {
        assert_eq!(
            node.cache.end,
            node.children()
                .iter()
                .map(|x| x.get_global_end())
                .max()
                .unwrap()
        );
        assert_eq!(node.cache.start, node.children()[0].get_global_start());
    }

    fn check_cache_internal(node: &InternalNode<'_, T, Self>) {
        assert_eq!(
            node.cache.end,
            node.children()
                .iter()
                .map(|x| get_cache(x).end)
                .max()
                .unwrap()
        );
        assert_eq!(node.cache.start, get_cache(node.children()[0]).start);
    }
}

#[inline]
fn get_pos_global<I: Integer>(index: I, cache: Cache<I>) -> Position {
    if index == cache.start {
        Position::Start
    } else if index == cache.end {
        Position::End
    } else {
        Position::Middle
    }
}

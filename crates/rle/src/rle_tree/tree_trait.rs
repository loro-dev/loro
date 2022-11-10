use std::{cmp::Ordering, fmt::Debug, ops::Deref};


use num::{traits::AsPrimitive, FromPrimitive, Integer};

use crate::{rle_trait::HasIndex, HasLength, Rle};

use super::{
    arena::Arena,
    node::{InternalNode, LeafNode, Node},
    HeapMode,
};

/// The position relative to a certain node.
///
/// - The target may be inside a node, in which case it's at the start/middle/end of a node.
/// - Or it is before/after a node.
#[derive(Debug, PartialEq, Eq, Clone, Copy, Hash)]
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

#[derive(Debug, PartialEq, Eq)]
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
    /// The allocation method used for [crate::RleTree].
    /// There are two modes provided:
    ///
    /// - [crate::rle_tree::HeapMode] will use Box to allocate nodes
    /// - [crate::rle_tree::BumpMode] will use [bumpalo] to allocate nodes, where allocation is fast but no deallocation happens before [crate::RleTree] dropped.
    ///
    /// NOTE: Should be cautious when using [crate::rle_tree::BumpMode], T's drop method won't be called in this mode.
    /// So you cannot use smart pointer in [crate::rle_tree::BumpMode] directly. You should wrap it inside [bumpalo]'s Box.
    type Arena: Arena;

    fn update_cache_leaf(node: &mut LeafNode<'_, T, Self>);
    fn update_cache_internal(node: &mut InternalNode<'_, T, Self>);

    /// - `child_index` can only equal to children.len() when index out of range
    /// - We need the `offset` so we can perform `find_pos_internal(child, new_search_index)`.
    /// - We need the `pos` to determine whether the child is included or excluded
    /// - If index is at the end of an element, `found` should be true
    /// - If not found, then `found` should be false and `child_index` should be the index of the insert position
    fn find_pos_internal(
        node: &InternalNode<'_, T, Self>,
        index: Self::Int,
    ) -> FindPosResult<Self::Int>;

    /// - `child_index` can only equal to children.len() when index out of range
    /// - if `pos == Middle`, we need to split the node
    /// - We need the third arg to determine whether the child is included or excluded
    /// - If not found, then `found` should be false and `child_index` should be the index of the insert position
    /// - If index is at the end of an element, `found` should be true
    /// - If target index is after last child, then `child_index`  = children.len().wrapping_sub(1), `offset` = children.last().unwrap().len()
    fn find_pos_leaf(node: &LeafNode<'_, T, Self>, index: Self::Int) -> FindPosResult<usize>;
    /// calculate the index of the child element of a leaf node
    fn get_index(node: &LeafNode<'_, T, Self>, child_index: usize) -> Self::Int;
    fn len_leaf(node: &LeafNode<'_, T, Self>) -> Self::Int;
    fn len_internal(node: &InternalNode<'_, T, Self>) -> Self::Int;
    fn check_cache_leaf(_node: &LeafNode<'_, T, Self>) {}
    fn check_cache_internal(_node: &InternalNode<'_, T, Self>) {}
}

#[derive(Debug, Default)]
pub struct CumulateTreeTrait<T: Rle, const MAX_CHILD: usize, TreeArena: Arena = HeapMode> {
    _phantom: std::marker::PhantomData<(T, TreeArena)>,
}

#[derive(Debug, Default)]
pub struct GlobalTreeTrait<T: Rle, const MAX_CHILD: usize, TreeArena: Arena = HeapMode> {
    _phantom: std::marker::PhantomData<(T, TreeArena)>,
}

impl<T: Rle, const MAX_CHILD: usize, TreeArena: Arena> RleTreeTrait<T>
    for CumulateTreeTrait<T, MAX_CHILD, TreeArena>
{
    const MAX_CHILDREN_NUM: usize = MAX_CHILD;

    const MIN_CHILDREN_NUM: usize = Self::MAX_CHILDREN_NUM / 2;

    type Int = usize;

    type InternalCache = usize;

    type LeafCache = usize;
    type Arena = TreeArena;

    fn update_cache_leaf(node: &mut LeafNode<'_, T, Self>) {
        node.cache = node
            .children()
            .iter()
            .map(|x| HasLength::content_len(x))
            .sum();
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
            last_cache = match child.deref() {
                Node::Internal(x) => {
                    if index <= x.cache {
                        return FindPosResult::new(i, index, Position::get_pos(index, child.len()));
                    }
                    x.cache
                }
                Node::Leaf(x) => {
                    if index <= x.cache {
                        return FindPosResult::new(i, index, Position::get_pos(index, child.len()));
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
            if index < HasLength::content_len(child) {
                return FindPosResult::new(i, index, Position::get_pos(index, child.content_len()));
            }

            index -= HasLength::content_len(child);
        }

        FindPosResult::new(
            node.children().len() - 1,
            HasLength::atom_len(node.children().last().unwrap()),
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
        assert_eq!(
            node.cache,
            node.children().iter().map(|x| x.content_len()).sum()
        );
    }

    fn get_index(node: &LeafNode<'_, T, Self>, mut child_index: usize) -> Self::Int {
        debug_assert!(!node.is_deleted());
        let mut index = 0;
        for i in 0..child_index {
            index += node.children[i].content_len();
        }

        child_index = node.get_index_in_parent().unwrap();
        // SAFETY: parent is valid if node is valid
        let mut node = unsafe { node.parent.as_ref() };
        loop {
            for i in 0..child_index {
                index += node.children[i].len();
            }

            if let Some(parent) = node.parent {
                child_index = node.get_index_in_parent().unwrap();
                // SAFETY: parent is valid if node is valid
                node = unsafe { parent.as_ref() };
            } else {
                break;
            }
        }

        index
    }
}

impl Position {
    pub fn get_pos(index: usize, len: usize) -> Position {
        if index == 0 {
            Position::Start
        } else if index == len {
            Position::End
        } else {
            Position::Middle
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Default, Copy)]
pub struct Cache<I> {
    start: I,
    end: I,
}

#[inline]
fn get_cache<T: Rle + HasIndex, const MAX_CHILD: usize, TreeArena: Arena>(
    node: &Node<'_, T, GlobalTreeTrait<T, MAX_CHILD, TreeArena>>,
) -> Cache<T::Int> {
    match node {
        Node::Internal(x) => x.cache,
        Node::Leaf(x) => x.cache,
    }
}

impl<T: Rle + HasIndex, const MAX_CHILD: usize, TreeArena: Arena> RleTreeTrait<T>
    for GlobalTreeTrait<T, MAX_CHILD, TreeArena>
{
    const MAX_CHILDREN_NUM: usize = MAX_CHILD;

    const MIN_CHILDREN_NUM: usize = Self::MAX_CHILDREN_NUM / 2;

    type Int = T::Int;

    type InternalCache = Cache<T::Int>;
    type LeafCache = Cache<T::Int>;
    type Arena = TreeArena;

    fn update_cache_leaf(node: &mut LeafNode<'_, T, Self>) {
        if node.children.is_empty() {
            node.cache.end = node.cache.start;
            return;
        }

        // TODO: Maybe panic if overlap?
        node.cache.end = node.children().last().unwrap().get_end_index();
        node.cache.start = node.children()[0].get_start_index();
    }

    fn update_cache_internal(node: &mut InternalNode<'_, T, Self>) {
        if node.children.is_empty() {
            return;
        }

        node.cache.end = get_cache(node.children().last().unwrap()).end;
        node.cache.start = get_cache(&node.children()[0]).start;
    }

    fn find_pos_internal(
        node: &InternalNode<'_, T, Self>,
        index: Self::Int,
    ) -> FindPosResult<Self::Int> {
        if node.children.is_empty() || index > node.cache.end {
            return FindPosResult::new_not_found(
                node.children.len().saturating_sub(1),
                index,
                Position::After,
            );
        }

        if index < node.cache.start {
            return FindPosResult::new_not_found(0, index, Position::Before);
        }

        let ans = node
            .children
            .binary_search_by(|x| {
                let cache = get_cache(x);
                if index < cache.start {
                    Ordering::Greater
                } else if index > cache.end {
                    Ordering::Less
                } else {
                    Ordering::Equal
                }
            })
            .map_or_else(
                |x| {
                    FindPosResult::new_not_found(
                        x,
                        index,
                        get_pos_global(index, get_cache(&node.children[x])),
                    )
                },
                |x| {
                    FindPosResult::new(
                        x,
                        index,
                        get_pos_global(index, get_cache(&node.children[x])),
                    )
                },
            );
        if ans.pos == Position::End {
            if ans.child_index + 1 < node.children.len()
                && index == get_cache(&node.children[ans.child_index + 1]).start
            {
                FindPosResult::new(ans.child_index + 1, index, Position::Start)
            } else {
                ans
            }
        } else {
            ans
        }
    }

    fn find_pos_leaf(node: &LeafNode<'_, T, Self>, index: Self::Int) -> FindPosResult<usize> {
        if node.children.is_empty() || index > node.cache.end {
            return FindPosResult::new_not_found(
                node.children.len().saturating_sub(1),
                node.children.last().map(|x| x.atom_len()).unwrap_or(0),
                Position::After,
            );
        }

        if index < node.cache.start {
            return FindPosResult::new_not_found(0, 0, Position::Before);
        }

        let ans = node
            .children
            .binary_search_by(|x| {
                let cache = Cache {
                    start: x.get_start_index(),
                    end: x.get_end_index(),
                };
                if index < cache.start {
                    Ordering::Greater
                } else if index > cache.end {
                    Ordering::Less
                } else {
                    Ordering::Equal
                }
            })
            .map_or_else(
                |x| {
                    FindPosResult::new_not_found(
                        x,
                        0,
                        get_pos_global(
                            index,
                            Cache {
                                start: node.children[x].get_start_index(),
                                end: node.children[x].get_end_index(),
                            },
                        ),
                    )
                },
                |x| {
                    FindPosResult::new(
                        x,
                        (index - node.children[x].get_start_index()).as_(),
                        get_pos_global(
                            index,
                            Cache {
                                start: node.children[x].get_start_index(),
                                end: node.children[x].get_end_index(),
                            },
                        ),
                    )
                },
            );
        if ans.pos == Position::End {
            if ans.child_index + 1 < node.children.len()
                && index == node.children[ans.child_index + 1].get_start_index()
            {
                FindPosResult::new(ans.child_index + 1, 0, Position::Start)
            } else {
                ans
            }
        } else {
            ans
        }
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
                .map(|x| x.get_end_index())
                .max()
                .unwrap()
        );
        assert_eq!(node.cache.start, node.children()[0].get_start_index());
    }

    fn check_cache_internal(node: &InternalNode<'_, T, Self>) {
        if node.children().is_empty() {
            return;
        }

        assert_eq!(
            node.cache.end,
            node.children()
                .iter()
                .map(|x| get_cache(x).end)
                .max()
                .unwrap()
        );
        assert_eq!(node.cache.start, get_cache(&node.children()[0]).start);
    }

    fn get_index(node: &LeafNode<'_, T, Self>, child_index: usize) -> Self::Int {
        node.children[child_index].get_start_index()
    }
}

#[inline]
fn get_pos_global<I: Integer>(index: I, cache: Cache<I>) -> Position {
    if index == cache.start {
        Position::Start
    } else if index == cache.end {
        Position::End
    } else if index < cache.start {
        Position::Before
    } else if index > cache.end {
        Position::After
    } else {
        Position::Middle
    }
}

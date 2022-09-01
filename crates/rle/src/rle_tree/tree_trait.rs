use std::fmt::Debug;

use num::{traits::AsPrimitive, FromPrimitive, Integer};

use crate::{HasLength, Rle};

use super::node::{InternalNode, LeafNode, Node};

#[derive(Debug, PartialEq, Eq)]
pub enum Position {
    Start,
    Middle,
    End,
}

pub trait RleTreeTrait<T: Rle>: Sized + Debug {
    const MAX_CHILDREN_NUM: usize;
    const MIN_CHILDREN_NUM: usize = Self::MAX_CHILDREN_NUM / 2;
    type Int: num::Integer + Copy + Debug + FromPrimitive;
    type InternalCache: Default + Debug + Eq + Clone;
    type LeafCache: Default + Debug + Eq + Clone;

    fn update_cache_leaf(node: &mut LeafNode<'_, T, Self>);
    fn update_cache_internal(node: &mut InternalNode<'_, T, Self>);

    /// returns `(child_index, new_search_index, pos)`
    ///
    /// - We need the second arg so we can perform `find_pos_internal(child, new_search_index)`.
    /// - We need the third arg to determine whether the child is included or excluded
    fn find_pos_internal(
        node: &InternalNode<'_, T, Self>,
        index: Self::Int,
    ) -> (usize, Self::Int, Position);

    /// returns `(index, offset, pos)`
    ///
    /// if `pos == Middle`, we need to split the node
    ///
    /// - We need the third arg to determine whether the child is included or excluded
    fn find_pos_leaf(node: &LeafNode<'_, T, Self>, index: Self::Int) -> (usize, usize, Position);

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
    ) -> (usize, Self::Int, Position) {
        let mut last_cache = 0;
        for (i, child) in node.children().iter().enumerate() {
            last_cache = match child {
                Node::Internal(x) => {
                    if index <= x.cache {
                        return (i, index, get_pos(index, child.len()));
                    }
                    x.cache
                }
                Node::Leaf(x) => {
                    if index <= x.cache {
                        return (i, index, get_pos(index, child.len()));
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
        (node.children().len() - 1, last_cache, Position::End)
    }

    fn find_pos_leaf(
        node: &LeafNode<'_, T, Self>,
        mut index: Self::Int,
    ) -> (usize, usize, Position) {
        for (i, child) in node.children().iter().enumerate() {
            if index < HasLength::len(&**child) {
                return (i, index, get_pos(index, child.len()));
            }

            index -= HasLength::len(&**child);
        }

        (
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

pub trait HasGlobalIndex: HasLength {
    type Int: Debug + Integer + Copy + Default + FromPrimitive + AsPrimitive<usize>;
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
        node.cache.end = node
            .children()
            .iter()
            .map(|x| x.get_global_end())
            .max()
            .unwrap();
        node.cache.start = node.children()[0].get_global_start();
    }

    fn update_cache_internal(node: &mut InternalNode<'_, T, Self>) {
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
    ) -> (usize, Self::Int, Position) {
        for (i, child) in node.children().iter().enumerate() {
            let cache = get_cache(child);
            if index <= cache.end {
                assert!(index >= cache.start);
                return (i, index, get_pos_global(index, cache));
            }
        }

        unreachable!();
    }

    fn find_pos_leaf(node: &LeafNode<'_, T, Self>, index: Self::Int) -> (usize, usize, Position) {
        for (i, child) in node.children().iter().enumerate() {
            let cache = Cache {
                start: child.get_global_start(),
                end: child.get_global_end(),
            };
            if index <= cache.end {
                assert!(index >= cache.start);
                return (i, (index - cache.start).as_(), get_pos_global(index, cache));
            }
        }

        unreachable!();
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

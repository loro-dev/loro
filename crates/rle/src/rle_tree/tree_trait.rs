use std::fmt::Debug;

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
    type Int: num::Integer + Copy + Debug;
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

    fn len_leaf(node: &LeafNode<'_, T, Self>) -> usize;
    fn len_internal(node: &InternalNode<'_, T, Self>) -> usize;
    fn check_cache_leaf(_node: &LeafNode<'_, T, Self>) {}
    fn check_cache_internal(_node: &InternalNode<'_, T, Self>) {}
}

#[derive(Debug, Default)]
pub struct CumulateTreeTrait<T: Rle, const MAX_CHILD: usize> {
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
                        return (i, index, get_pos(index, *child));
                    }
                    x.cache
                }
                Node::Leaf(x) => {
                    if index <= x.cache {
                        return (i, index, get_pos(index, *child));
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
                return (i, index, get_pos(index, &**child));
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

fn get_pos<T: HasLength>(index: usize, child: &T) -> Position {
    if index == 0 {
        Position::Start
    } else if index == child.len() {
        Position::End
    } else {
        Position::Middle
    }
}

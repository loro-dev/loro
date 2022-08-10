use std::fmt::Debug;

use crate::Rle;

use super::node::{InternalNode, LeafNode, Node};

#[derive(Debug, PartialEq, Eq)]
pub enum Position {
    Start,
    Middle,
    End,
}

pub trait RleTreeTrait<T: Rle>: Sized {
    const MAX_CHILDREN_NUM: usize;
    const MIN_CHILDREN_NUM: usize = Self::MAX_CHILDREN_NUM / 2;
    type Int: num::Integer + Copy + Debug;
    type InternalCache: Default + Debug;
    type LeafCache: Default + Debug;

    fn update_cache_leaf(node: &mut LeafNode<'_, T, Self>);
    fn update_cache_internal(node: &mut InternalNode<'_, T, Self>);

    /// returns `(child_index, new_search_index, pos)`
    ///
    /// - We need the second arg so we can perform `find_pos_internal(child, new_search_index)`.
    /// - We need the third arg to determine whether the child is included or excluded
    fn find_pos_internal(
        node: &mut InternalNode<'_, T, Self>,
        index: Self::Int,
    ) -> (usize, Self::Int, Position);

    /// returns `(index, offset, pos)`
    ///
    /// if `pos == Middle`, we need to split the node
    ///
    /// - We need the third arg to determine whether the child is included or excluded
    fn find_pos_leaf(
        node: &mut LeafNode<'_, T, Self>,
        index: Self::Int,
    ) -> (usize, usize, Position);

    fn len_leaf(node: &LeafNode<'_, T, Self>) -> usize;
    fn len_internal(node: &InternalNode<'_, T, Self>) -> usize;
}

use std::fmt::Debug;

use crate::Rle;

use super::node::{InternalNode, LeafNode, Node};

pub trait RleTreeTrait<T: Rle>: Sized {
    const MAX_CHILDREN_NUM: usize;
    const MIN_CHILDREN_NUM: usize = Self::MAX_CHILDREN_NUM / 2;
    type Int: num::Integer + Copy;
    type InternalCache: Default + Debug;
    type LeafCache: Default + Debug;

    fn update_cache_leaf(node: &mut LeafNode<'_, T, Self>);
    fn update_cache_internal(node: &mut InternalNode<'_, T, Self>);
    fn find_insert_pos_internal(
        node: &mut InternalNode<'_, T, Self>,
        index: Self::Int,
    ) -> (usize, Self::Int);
    /// returns (index, offset)
    /// if 0 < offset < children[index].len(), we need to split the node
    fn find_insert_pos_leaf(node: &mut LeafNode<'_, T, Self>, index: Self::Int) -> (usize, usize);

    fn len_leaf(node: &LeafNode<'_, T, Self>) -> usize;
    fn len_internal(node: &InternalNode<'_, T, Self>) -> usize;
}

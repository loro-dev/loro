use crate::Rle;

use super::node::{InternalNode, Node};

pub trait RleTreeTrait<T: Rle>: Sized {
    type Int: num::Integer;
    type InternalCache;

    fn update_cache();
    fn min_children() -> usize;

    #[inline]
    fn max_children() -> usize {
        Self::min_children() * 2
    }

    fn before_insert_internal(node: InternalNode<'_, T, Self>);
    fn find_insert_pos_internal(node: InternalNode<'_, T, Self>, index: Self::Int) -> usize;
}

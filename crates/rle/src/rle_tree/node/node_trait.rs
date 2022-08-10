use crate::{rle_tree::tree_trait::RleTreeTrait, Rle};

use super::{BumpBox, InternalNode, Node};

pub(crate) trait NodeTrait<'a, T: Rle, A: RleTreeTrait<T>> {
    type Child;

    fn to_node(node: BumpBox<'a, Self>) -> Node<'a, T, A>;
    fn delete(&mut self, from: Option<A::Int>, to: Option<A::Int>)
        -> Result<(), BumpBox<'a, Self>>;
    fn _insert_with_split(
        &mut self,
        child_index: usize,
        new: Self::Child,
    ) -> Result<(), BumpBox<'a, Self>>;
}

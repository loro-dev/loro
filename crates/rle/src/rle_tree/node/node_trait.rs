use crate::{rle_tree::tree_trait::RleTreeTrait, Rle};

use super::{BumpBox, InternalNode, LeafNode, Node};

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

impl<'a, T: Rle, A: RleTreeTrait<T>> From<BumpBox<'a, InternalNode<'a, T, A>>> for Node<'a, T, A> {
    fn from(node: BumpBox<'a, InternalNode<'a, T, A>>) -> Self {
        Node::Internal(node)
    }
}

impl<'a, T: Rle, A: RleTreeTrait<T>> From<BumpBox<'a, LeafNode<'a, T, A>>> for Node<'a, T, A> {
    fn from(node: BumpBox<'a, LeafNode<'a, T, A>>) -> Self {
        Node::Leaf(node)
    }
}

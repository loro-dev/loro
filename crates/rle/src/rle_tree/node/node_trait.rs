use crate::{rle_tree::tree_trait::RleTreeTrait, Rle};

use super::{BumpBox, InternalNode, LeafNode, Node};
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

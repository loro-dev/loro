use crate::{rle_tree::tree_trait::RleTreeTrait, Rle};

use super::{InternalNode, LeafNode, Node};
impl<'a, T: Rle, A: RleTreeTrait<T>> From<InternalNode<'a, T, A>> for Node<'a, T, A> {
    fn from(node: InternalNode<'a, T, A>) -> Self {
        Node::Internal(node)
    }
}

impl<'a, T: Rle, A: RleTreeTrait<T>> From<LeafNode<'a, T, A>> for Node<'a, T, A> {
    fn from(node: LeafNode<'a, T, A>) -> Self {
        Node::Leaf(node)
    }
}

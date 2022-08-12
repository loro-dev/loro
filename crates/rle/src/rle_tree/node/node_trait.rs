use crate::{rle_tree::tree_trait::RleTreeTrait, Rle};

use super::{InternalNode, LeafNode, Node};
impl<'a, T: Rle, A: RleTreeTrait<T>> From<&'a mut InternalNode<'a, T, A>> for Node<'a, T, A> {
    fn from(node: &'a mut InternalNode<'a, T, A>) -> Self {
        Node::Internal(node)
    }
}

impl<'a, T: Rle, A: RleTreeTrait<T>> From<&'a mut LeafNode<'a, T, A>> for Node<'a, T, A> {
    fn from(node: &'a mut LeafNode<'a, T, A>) -> Self {
        Node::Leaf(node)
    }
}

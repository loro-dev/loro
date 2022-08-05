pub(self) use bumpalo::collections::vec::Vec as BumpVec;
use std::marker::{PhantomData, PhantomPinned};

use crate::Rle;
use bumpalo::Bump;
use tree_trait::RleTreeTrait;

use self::node::{InternalNode, Node};

mod node;
mod tree_trait;

#[derive(Debug)]
pub struct RleTree<'a, T: Rle, A: RleTreeTrait<T>> {
    bump: &'a Bump,
    node: Node<'a, T, A>,
    _pin: PhantomPinned,
    _a: PhantomData<(A, T)>,
}

impl<'a, T: Rle, A: RleTreeTrait<T>> RleTree<'a, T, A> {
    pub fn new(bump: &'a Bump) -> Self {
        Self {
            bump,
            node: Node::Internal(InternalNode::new(bump)),
            _pin: PhantomPinned,
            _a: PhantomData,
        }
    }

    fn insert(&mut self, index: A::Int, value: T) {
        self.node.insert(index, value);
    }

    /// return a cursor to the tree
    fn get(&self, index: A::Int) {
        todo!()
    }

    fn iter(&self) {
        todo!()
    }

    fn delete_range(&mut self, from: A::Int, to: A::Int) {
        todo!()
    }

    fn iter_range(&self, from: A::Int, to: A::Int) {
        todo!()
    }

    #[cfg(test)]
    fn debug_check(&self) {
        todo!()
    }
}

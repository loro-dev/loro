use self::node::{InternalNode, Node};
use crate::{HasLength, Rle};
pub(self) use bumpalo::collections::vec::Vec as BumpVec;
use bumpalo::Bump;
use ouroboros::self_referencing;
use std::marker::{PhantomData, PhantomPinned};
use tree_trait::RleTreeTrait;
mod iter;
mod node;
#[cfg(test)]
mod test;
mod tree_trait;

#[derive(Debug)]
pub struct RleTreeRaw<'a, T: Rle, A: RleTreeTrait<T>> {
    node: Node<'a, T, A>,
    _pin: PhantomPinned,
    _a: PhantomData<(A, T)>,
}

#[self_referencing]
pub struct RleTree<T: Rle + 'static, A: RleTreeTrait<T> + 'static> {
    bump: Bump,
    #[borrows(bump)]
    tree: &'this mut RleTreeRaw<'this, T, A>,
}

impl<T: Rle + 'static, A: RleTreeTrait<T> + 'static> Default for RleTree<T, A> {
    fn default() -> Self {
        RleTreeBuilder {
            bump: Bump::new(),
            tree_builder: |bump| bump.alloc(RleTreeRaw::new(bump)),
        }
        .build()
    }
}

impl<'a, T: Rle, A: RleTreeTrait<T>> RleTreeRaw<'a, T, A> {
    #[inline]
    fn new(bump: &'a Bump) -> Self {
        Self {
            node: Node::Internal(bump.alloc(InternalNode::new(bump, None))),
            _pin: PhantomPinned,
            _a: PhantomData,
        }
    }

    #[inline]
    pub fn insert(&mut self, index: A::Int, value: T) {
        self.node
            .as_internal_mut()
            .unwrap()
            .insert(index, value)
            .unwrap();
    }

    /// return a cursor to the tree
    pub fn get(&self, _index: A::Int) {
        todo!()
    }

    pub fn iter(&self) -> iter::Iter<'_, 'a, T, A> {
        iter::Iter::new(self.node.get_first_leaf())
    }

    pub fn delete_range(&mut self, start: Option<A::Int>, end: Option<A::Int>) {
        self.node.as_internal_mut().unwrap().delete(start, end);
    }

    pub fn iter_range(&self, _from: A::Int, _to: A::Int) {
        todo!()
    }

    #[cfg(test)]
    fn debug_check(&mut self) {
        self.node.as_internal_mut().unwrap().check();
    }
}

impl<'a, T: Rle, A: RleTreeTrait<T>> HasLength for RleTreeRaw<'a, T, A> {
    fn len(&self) -> usize {
        self.node.len()
    }
}

use self::node::{InternalNode, Node};
use crate::Rle;
pub(self) use bumpalo::collections::vec::Vec as BumpVec;
use bumpalo::Bump;
pub use cursor::{SafeCursor, SafeCursorMut};
use ouroboros::self_referencing;
use std::marker::{PhantomData, PhantomPinned};
use tree_trait::RleTreeTrait;

mod cursor;
mod iter;
pub mod node;
#[cfg(test)]
mod test;
pub mod tree_trait;

#[derive(Debug)]
pub struct RleTreeRaw<'a, T: Rle, A: RleTreeTrait<T>> {
    node: Node<'a, T, A>,
    _pin: PhantomPinned,
    _a: PhantomData<(A, T)>,
}

#[self_referencing]
#[derive(Debug)]
pub struct RleTree<T: Rle + 'static, A: RleTreeTrait<T> + 'static> {
    bump: Bump,
    #[borrows(bump)]
    pub tree: &'this mut RleTreeRaw<'this, T, A>,
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
            node: Node::Internal(InternalNode::new(bump, None)),
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
    #[inline]
    pub fn get<'b>(&'b self, mut index: A::Int) -> SafeCursor<'a, 'b, T, A> {
        let mut node = &self.node;
        loop {
            match node {
                Node::Internal(internal_node) => {
                    let (child_index, next, _) = A::find_pos_internal(internal_node, index);
                    node = internal_node.children[child_index];
                    index = next;
                }
                Node::Leaf(leaf) => {
                    let (child_index, _, _) = A::find_pos_leaf(leaf, index);
                    return SafeCursor::new(leaf.into(), child_index);
                }
            }
        }
    }

    #[inline]
    pub fn get_mut<'b>(&'b mut self, index: A::Int) -> SafeCursorMut<'a, 'b, T, A> {
        let cursor = self.get(index);
        SafeCursorMut(cursor.0)
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

    pub fn debug_check(&mut self) {
        self.node.as_internal_mut().unwrap().check();
    }
}

impl<'a, T: Rle, A: RleTreeTrait<T>> RleTreeRaw<'a, T, A> {
    #[inline]
    pub fn len(&self) -> A::Int {
        self.node.len()
    }
}

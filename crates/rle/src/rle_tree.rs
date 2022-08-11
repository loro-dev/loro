use self::node::{InternalNode, Node};
use crate::{HasLength, Rle};
pub(self) use bumpalo::boxed::Box as BumpBox;
pub(self) use bumpalo::collections::vec::Vec as BumpVec;
use bumpalo::Bump;
use owning_ref::OwningRefMut;
use std::marker::{PhantomData, PhantomPinned};
use tree_trait::RleTreeTrait;
mod fixed_size_vec;
mod iter;
mod node;
#[cfg(test)]
mod test;
mod tree_trait;

#[derive(Debug)]
pub struct RleTreeRaw<'a, T: Rle, A: RleTreeTrait<T>> {
    bump: &'a Bump,
    node: Node<'a, T, A>,
    _pin: PhantomPinned,
    _a: PhantomData<(A, T)>,
}

#[allow(unused)]
type TreeRef<T, A> =
    OwningRefMut<Box<(Box<Bump>, RleTreeRaw<'static, T, A>)>, RleTreeRaw<'static, T, A>>;

pub struct RleTree<T: Rle + 'static, A: RleTreeTrait<T> + 'static> {
    tree: TreeRef<T, A>,
}

impl<T: Rle + 'static, A: RleTreeTrait<T> + 'static> RleTree<T, A> {
    pub fn new() -> Self {
        let bump = Box::new(Bump::new());
        let tree = RleTreeRaw::new(unsafe { &*(&*bump as *const _) });
        let m = OwningRefMut::new(Box::new((bump, tree)));
        let tree = m.map_mut(|(_, tree)| tree);
        Self { tree }
    }

    pub fn get_ref(&self) -> &RleTreeRaw<'static, T, A> {
        self.tree.as_ref()
    }

    pub fn get_mut(&mut self) -> &mut RleTreeRaw<'static, T, A> {
        self.tree.as_mut()
    }
}

impl<'a, T: Rle, A: RleTreeTrait<T>> RleTreeRaw<'a, T, A> {
    #[inline]
    fn new(bump: &'a Bump) -> Self {
        Self {
            bump,
            node: Node::Internal(BumpBox::new_in(InternalNode::new(bump, None), bump)),
            _pin: PhantomPinned,
            _a: PhantomData,
        }
    }

    #[inline]
    pub fn insert(&mut self, index: A::Int, value: T) {
        match self.node {
            Node::Internal(ref mut node) => {
                node.insert(index, value).unwrap();
            }
            _ => {
                unreachable!()
            }
        }
    }

    /// return a cursor to the tree
    pub fn get(&self, index: A::Int) {
        todo!()
    }

    pub fn iter(&self) -> iter::Iter<'_, 'a, T, A> {
        iter::Iter::new(self.node.get_first_leaf())
    }

    pub fn delete_range(&mut self, start: A::Int, end: A::Int) {
        self.node
            .as_internal_mut()
            .unwrap()
            .delete(Some(start), Some(end));
    }

    pub fn iter_range(&self, from: A::Int, to: A::Int) {
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

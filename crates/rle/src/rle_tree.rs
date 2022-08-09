pub(self) use bumpalo::boxed::Box as BumpBox;
pub(self) use bumpalo::collections::vec::Vec as BumpVec;
use owning_ref::OwningRefMut;
use std::marker::{PhantomData, PhantomPinned};

use crate::Rle;
use bumpalo::Bump;
use tree_trait::RleTreeTrait;

use self::node::{InternalNode, Node};

mod fixed_size_vec;
mod node;
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
                node.insert(index, value);
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

    pub fn iter(&self) {
        todo!()
    }

    pub fn delete_range(&mut self, from: A::Int, to: A::Int) {
        todo!()
    }

    pub fn iter_range(&self, from: A::Int, to: A::Int) {
        todo!()
    }

    #[cfg(test)]
    fn debug_check(&self) {
        todo!()
    }
}

/// compile test
#[cfg(test)]
#[test]
fn test() {
    use std::ops::Range;

    struct Trait;
    impl RleTreeTrait<Range<usize>> for Trait {
        const MAX_CHILDREN_NUM: usize = 4;
        type Int = usize;
        type InternalCache = ();

        fn find_insert_pos_internal(
            _: &mut InternalNode<'_, Range<usize>, Self>,
            _: Self::Int,
        ) -> usize {
            todo!()
        }

        const MIN_CHILDREN_NUM: usize = Self::MAX_CHILDREN_NUM / 2;

        fn update_cache_leaf(node: &mut node::LeafNode<'_, Range<usize>, Self>) {
            todo!()
        }

        fn update_cache_internal(node: &mut InternalNode<'_, Range<usize>, Self>) {
            todo!()
        }

        fn find_insert_pos_leaf(
            node: &mut node::LeafNode<'_, Range<usize>, Self>,
            index: Self::Int,
        ) -> (usize, usize) {
            todo!()
        }
    }
    let mut t: RleTree<Range<usize>, Trait> = RleTree::new();
    let tree = t.get_mut();
    tree.insert(10, 0..5);
}

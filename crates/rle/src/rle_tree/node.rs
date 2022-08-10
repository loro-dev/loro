use std::{
    marker::{PhantomData, PhantomPinned},
    pin::Pin,
    ptr::NonNull,
};

use crate::{HasLength, Rle};

use super::{
    fixed_size_vec::FixedSizedVec, tree_trait::RleTreeTrait, BumpBox, BumpVec, RleTreeRaw,
};
use bumpalo::Bump;
use enum_as_inner::EnumAsInner;
mod internal_impl;
mod leaf_impl;
pub(crate) mod node_trait;

#[derive(Debug, EnumAsInner)]
pub enum Node<'a, T: Rle, A: RleTreeTrait<T>> {
    Internal(BumpBox<'a, InternalNode<'a, T, A>>),
    Leaf(BumpBox<'a, LeafNode<'a, T, A>>),
}

#[derive(Debug)]
pub struct InternalNode<'a, T: Rle, A: RleTreeTrait<T>> {
    bump: &'a Bump,
    parent: Option<NonNull<InternalNode<'a, T, A>>>,
    pub(super) children: FixedSizedVec<'a, Node<'a, T, A>>,
    pub cache: A::InternalCache,
    _pin: PhantomPinned,
    _a: PhantomData<A>,
}

#[derive(Debug)]
pub struct LeafNode<'a, T: Rle, A: RleTreeTrait<T>> {
    bump: &'a Bump,
    parent: NonNull<InternalNode<'a, T, A>>,
    pub(super) children: FixedSizedVec<'a, T>,
    prev: Option<NonNull<LeafNode<'a, T, A>>>,
    next: Option<NonNull<LeafNode<'a, T, A>>>,
    pub cache: A::LeafCache,
    _pin: PhantomPinned,
    _a: PhantomData<A>,
}

impl<'a, T: Rle, A: RleTreeTrait<T>> Node<'a, T, A> {
    fn new_internal(bump: &'a Bump) -> Self {
        Self::Internal(BumpBox::new_in(InternalNode::new(bump, None), bump))
    }

    fn new_leaf(bump: &'a Bump, parent: NonNull<InternalNode<'a, T, A>>) -> Self {
        Self::Leaf(BumpBox::new_in(LeafNode::new(bump, parent), bump))
    }

    pub fn get_first_leaf(&self) -> Option<&LeafNode<'a, T, A>> {
        match self {
            Self::Internal(node) => node
                .children
                .get(0)
                .and_then(|child| child.get_first_leaf()),
            Self::Leaf(node) => Some(node),
        }
    }
}

impl<'a, T: Rle, A: RleTreeTrait<T>> HasLength for Node<'a, T, A> {
    #[inline]
    fn len(&self) -> usize {
        match self {
            Node::Internal(node) => node.len(),
            Node::Leaf(node) => node.len(),
        }
    }
}

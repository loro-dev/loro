use std::marker::{PhantomData, PhantomPinned};

use crate::Rle;

use super::{tree_trait::RleTreeTrait, BumpVec, RleTree};
use bumpalo::Bump;
use enum_as_inner::EnumAsInner;
mod internal_impl;
mod leaf_impl;

#[derive(Debug, EnumAsInner)]
pub enum Node<'a, T: Rle, A: RleTreeTrait<T>> {
    Internal(InternalNode<'a, T, A>),
    Leaf(LeafNode<'a, T, A>),
}

#[derive(Debug)]
pub struct InternalNode<'a, T: Rle, A: RleTreeTrait<T>> {
    bump: &'a Bump,
    parent: Option<&'a InternalNode<'a, T, A>>,
    children: BumpVec<'a, Node<'a, T, A>>,
    _pin: PhantomPinned,
    _a: PhantomData<A>,
}

#[derive(Debug)]
pub struct LeafNode<'a, T: Rle, A: RleTreeTrait<T>> {
    bump: &'a Bump,
    parent: &'a InternalNode<'a, T, A>,
    children: BumpVec<'a, T>,
    prev: Option<&'a LeafNode<'a, T, A>>,
    next: Option<&'a LeafNode<'a, T, A>>,
    _pin: PhantomPinned,
    _a: PhantomData<A>,
}

impl<'a, T: Rle, A: RleTreeTrait<T>> Node<'a, T, A> {
    pub(super) fn insert(&mut self, index: A::Int, value: T) {
        match self {
            Node::Internal(node) => {}
            Node::Leaf(node) => {}
        }
    }
}

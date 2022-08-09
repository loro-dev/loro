use std::{
    marker::{PhantomData, PhantomPinned},
    pin::Pin,
    ptr::NonNull,
};

use crate::Rle;

use super::{
    fixed_size_vec::FixedSizedVec, tree_trait::RleTreeTrait, BumpBox, BumpVec, RleTreeRaw,
};
use bumpalo::Bump;
use enum_as_inner::EnumAsInner;
mod internal_impl;
mod leaf_impl;

#[derive(Debug, EnumAsInner)]
pub enum Node<'a, T: Rle, A: RleTreeTrait<T>> {
    Internal(BumpBox<'a, InternalNode<'a, T, A>>),
    Leaf(BumpBox<'a, LeafNode<'a, T, A>>),
}

#[derive(Debug)]
pub struct InternalNode<'a, T: Rle, A: RleTreeTrait<T>> {
    bump: &'a Bump,
    parent: Option<NonNull<InternalNode<'a, T, A>>>,
    children: FixedSizedVec<'a, Node<'a, T, A>>,
    cache: A::InternalCache,
    _pin: PhantomPinned,
    _a: PhantomData<A>,
}

#[derive(Debug)]
pub struct LeafNode<'a, T: Rle, A: RleTreeTrait<T>> {
    bump: &'a Bump,
    parent: NonNull<InternalNode<'a, T, A>>,
    children: FixedSizedVec<'a, T>,
    prev: Option<NonNull<LeafNode<'a, T, A>>>,
    next: Option<NonNull<LeafNode<'a, T, A>>>,
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
}

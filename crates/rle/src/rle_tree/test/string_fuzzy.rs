use std::ops::{Deref, DerefMut};

use crate::{rle_tree::tree_trait::RleTreeTrait, HasLength, Mergable, Sliceable};

#[derive(Debug)]
struct CustomString(String);
impl Deref for CustomString {
    type Target = String;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for CustomString {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl HasLength for CustomString {
    fn len(&self) -> usize {
        self.0.len()
    }
}

impl Mergable for CustomString {
    fn is_mergable(&self, other: &Self, _conf: &()) -> bool
    where
        Self: Sized,
    {
        self.len() + other.len() < 16
    }

    fn merge(&mut self, other: &Self, _conf: &())
    where
        Self: Sized,
    {
        self.push_str(other.as_str())
    }
}

impl Sliceable for CustomString {
    fn slice(&self, from: usize, to: usize) -> Self {
        CustomString(self.0.slice(from, to))
    }
}

#[derive(Debug)]
struct StringTreeTrait;
impl RleTreeTrait<CustomString> for StringTreeTrait {
    const MAX_CHILDREN_NUM: usize = 4;

    const MIN_CHILDREN_NUM: usize = Self::MAX_CHILDREN_NUM / 2;

    type Int = usize;

    type InternalCache = usize;

    type LeafCache = usize;

    fn update_cache_leaf(node: &mut crate::rle_tree::node::LeafNode<'_, CustomString, Self>) {
        todo!()
    }

    fn update_cache_internal(
        node: &mut crate::rle_tree::node::InternalNode<'_, CustomString, Self>,
    ) {
        todo!()
    }

    fn find_pos_internal(
        node: &mut crate::rle_tree::node::InternalNode<'_, CustomString, Self>,
        index: Self::Int,
    ) -> (usize, Self::Int, crate::rle_tree::tree_trait::Position) {
        todo!()
    }

    fn find_pos_leaf(
        node: &mut crate::rle_tree::node::LeafNode<'_, CustomString, Self>,
        index: Self::Int,
    ) -> (usize, usize, crate::rle_tree::tree_trait::Position) {
        todo!()
    }

    fn len_leaf(node: &crate::rle_tree::node::LeafNode<'_, CustomString, Self>) -> usize {
        todo!()
    }

    fn len_internal(node: &crate::rle_tree::node::InternalNode<'_, CustomString, Self>) -> usize {
        todo!()
    }
}

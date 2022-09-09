use std::{marker::PhantomData, ptr::NonNull};

use crate::{Rle, RleTreeTrait};

use super::{node::LeafNode, tree_trait::Position};

pub struct UnsafeCursor<'tree, 'bump, T: Rle, A: RleTreeTrait<T>> {
    pub(crate) leaf: NonNull<LeafNode<'bump, T, A>>,
    pub(crate) index: usize,
    pub(crate) pos: Position,
    _phantom: PhantomData<&'tree usize>,
}

impl<'tree, 'bump, T: Rle, A: RleTreeTrait<T>> Clone for UnsafeCursor<'tree, 'bump, T, A> {
    #[inline]
    fn clone(&self) -> Self {
        Self {
            leaf: self.leaf,
            index: self.index,
            pos: self.pos,
            _phantom: Default::default(),
        }
    }
}

impl<'tree, 'bump: 'tree, T: Rle, A: RleTreeTrait<T>> Copy for UnsafeCursor<'tree, 'bump, T, A> {}

#[repr(transparent)]
pub struct SafeCursor<'tree, 'bump, T: Rle, A: RleTreeTrait<T>>(
    pub(crate) UnsafeCursor<'tree, 'bump, T, A>,
);

#[repr(transparent)]
pub struct SafeCursorMut<'tree, 'bump, T: Rle, A: RleTreeTrait<T>>(
    pub(crate) UnsafeCursor<'tree, 'bump, T, A>,
);

impl<'tree, 'bump: 'tree, T: Rle, A: RleTreeTrait<T>> Clone for SafeCursor<'tree, 'bump, T, A> {
    fn clone(&self) -> Self {
        Self(self.0)
    }
}

impl<'tree, 'bump: 'tree, T: Rle, A: RleTreeTrait<T>> Copy for SafeCursor<'tree, 'bump, T, A> {}

impl<'tree, 'bump: 'tree, T: Rle, A: RleTreeTrait<T>> UnsafeCursor<'tree, 'bump, T, A> {
    #[inline]
    pub(crate) fn new(leaf: NonNull<LeafNode<'bump, T, A>>, index: usize, pos: Position) -> Self {
        Self {
            leaf,
            index,
            pos,
            _phantom: PhantomData,
        }
    }

    #[inline]
    pub unsafe fn as_ref(&self) -> &'tree T {
        self.leaf.as_ref().children[self.index]
    }

    #[inline]
    unsafe fn as_mut(&mut self) -> &'tree mut T {
        self.leaf.as_mut().children[self.index]
    }

    #[inline]
    unsafe fn update_cache(&mut self) {
        let leaf = self.leaf.as_mut();
        A::update_cache_leaf(leaf);
        let mut node = leaf.parent.as_mut();
        loop {
            A::update_cache_internal(node);
            match node.parent {
                Some(mut parent) => node = parent.as_mut(),
                None => return,
            }
        }
    }

    pub unsafe fn next(&self) -> Option<Self> {
        let leaf = self.leaf.as_ref();
        if leaf.children.len() > self.index + 1 {
            return Some(Self::new(self.leaf, self.index + 1, self.pos));
        }

        leaf.next.map(|next| Self::new(next, 0, self.pos))
    }

    pub unsafe fn prev(&self) -> Option<Self> {
        let leaf = self.leaf.as_ref();
        if self.index > 0 {
            return Some(Self::new(self.leaf, self.index - 1, self.pos));
        }

        leaf.prev
            .map(|prev| Self::new(prev, prev.as_ref().children.len() - 1, self.pos))
    }
}

impl<'tree, 'bump: 'tree, T: Rle, A: RleTreeTrait<T>> AsRef<T> for SafeCursor<'tree, 'bump, T, A> {
    #[inline]
    fn as_ref(&self) -> &'tree T {
        unsafe { self.0.as_ref() }
    }
}

impl<'tree, 'bump: 'tree, T: Rle, A: RleTreeTrait<T>> SafeCursor<'tree, 'bump, T, A> {
    #[inline]
    pub fn as_tree_ref(&self) -> &'tree T {
        unsafe { self.0.as_ref() }
    }

    #[inline]
    pub fn next(&self) -> Option<Self> {
        unsafe { self.0.next().map(|x| Self(x)) }
    }

    #[inline]
    pub fn prev(&self) -> Option<Self> {
        unsafe { self.0.prev().map(|x| Self(x)) }
    }

    #[inline]
    pub fn leaf(&self) -> &'tree LeafNode<'bump, T, A> {
        unsafe { self.0.leaf.as_ref() }
    }

    #[inline]
    pub fn index(&self) -> usize {
        self.0.index
    }
}

impl<'tree, 'bump: 'tree, T: Rle, A: RleTreeTrait<T>> SafeCursor<'tree, 'bump, T, A> {
    #[inline]
    pub(crate) fn new(leaf: NonNull<LeafNode<'bump, T, A>>, index: usize, pos: Position) -> Self {
        Self(UnsafeCursor::new(leaf, index, pos))
    }
}

impl<'tree, 'bump: 'tree, T: Rle, A: RleTreeTrait<T>> AsRef<T>
    for SafeCursorMut<'tree, 'bump, T, A>
{
    #[inline]
    fn as_ref(&self) -> &T {
        unsafe { self.0.as_ref() }
    }
}

impl<'tree, 'bump: 'tree, T: Rle, A: RleTreeTrait<T>> SafeCursorMut<'tree, 'bump, T, A> {
    #[inline]
    pub fn as_ref_(&self) -> &'tree T {
        unsafe { self.0.as_ref() }
    }

    #[inline]
    pub fn leaf(&self) -> &'tree LeafNode<'bump, T, A> {
        unsafe { self.0.leaf.as_ref() }
    }

    #[inline]
    pub fn child_index(&self) -> usize {
        self.0.index
    }
}

impl<'tree, 'bump: 'tree, T: Rle, A: RleTreeTrait<T>> SafeCursorMut<'tree, 'bump, T, A> {
    #[inline]
    pub(crate) fn new(leaf: NonNull<LeafNode<'bump, T, A>>, index: usize, pos: Position) -> Self {
        Self(UnsafeCursor::new(leaf, index, pos))
    }

    #[inline]
    fn as_mut_(&mut self) -> &'tree mut T {
        unsafe { self.0.as_mut() }
    }

    #[inline]
    pub fn update_cache_recursively(&mut self) {
        let leaf = unsafe { self.0.leaf.as_mut() };
        A::update_cache_leaf(leaf);
        let mut node = unsafe { leaf.parent.as_mut() };
        loop {
            A::update_cache_internal(node);
            match node.parent {
                Some(mut parent) => node = unsafe { parent.as_mut() },
                None => return,
            }
        }
    }
}

impl<'tree, 'bump: 'tree, T: Rle, A: RleTreeTrait<T>> AsMut<T>
    for SafeCursorMut<'tree, 'bump, T, A>
{
    #[inline]
    fn as_mut(&mut self) -> &mut T {
        unsafe { self.0.as_mut() }
    }
}

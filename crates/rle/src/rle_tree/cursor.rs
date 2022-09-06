use std::{marker::PhantomData, ptr::NonNull};

use crate::{Rle, RleTreeTrait};

use super::{node::LeafNode, RleTreeRaw};

pub struct UnsafeCursor<'a, Tree, T: Rle, A: RleTreeTrait<T>> {
    pub(crate) leaf: NonNull<LeafNode<'a, T, A>>,
    pub(crate) index: usize,
    _phantom: PhantomData<Tree>,
}

pub struct SafeCursor<'a, 'b, T: Rle, A: RleTreeTrait<T>>(
    pub(crate) UnsafeCursor<'a, &'b RleTreeRaw<'a, T, A>, T, A>,
);

pub struct SafeCursorMut<'a, 'b, T: Rle, A: RleTreeTrait<T>>(
    pub(crate) UnsafeCursor<'a, &'b RleTreeRaw<'a, T, A>, T, A>,
);

impl<'a, Tree, T: Rle, A: RleTreeTrait<T>> UnsafeCursor<'a, Tree, T, A> {
    #[inline]
    pub(crate) fn new(leaf: NonNull<LeafNode<'a, T, A>>, index: usize) -> Self {
        Self {
            leaf,
            index,
            _phantom: PhantomData,
        }
    }

    #[inline]
    unsafe fn as_ref(&self) -> &T {
        self.leaf.as_ref().children[self.index]
    }

    #[inline]
    unsafe fn as_mut(&mut self) -> &mut T {
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
}

impl<'a, 'b, T: Rle, A: RleTreeTrait<T>> AsRef<T> for SafeCursor<'a, 'b, T, A> {
    #[inline]
    fn as_ref(&self) -> &T {
        unsafe { self.0.as_ref() }
    }
}

impl<'a, 'b, T: Rle, A: RleTreeTrait<T>> SafeCursor<'a, 'b, T, A> {
    #[inline]
    pub(crate) fn new(leaf: NonNull<LeafNode<'a, T, A>>, index: usize) -> Self {
        Self(UnsafeCursor::new(leaf, index))
    }
}

impl<'a, 'b, T: Rle, A: RleTreeTrait<T>> AsRef<T> for SafeCursorMut<'a, 'b, T, A> {
    #[inline]
    fn as_ref(&self) -> &T {
        unsafe { self.0.as_ref() }
    }
}

impl<'a, 'b, T: Rle, A: RleTreeTrait<T>> SafeCursorMut<'a, 'b, T, A> {
    #[inline]
    pub(crate) fn new(leaf: NonNull<LeafNode<'a, T, A>>, index: usize) -> Self {
        Self(UnsafeCursor::new(leaf, index))
    }
}

impl<'a, 'b, T: Rle, A: RleTreeTrait<T>> AsMut<T> for SafeCursorMut<'a, 'b, T, A> {
    #[inline]
    fn as_mut(&mut self) -> &mut T {
        unsafe { self.0.as_mut() }
    }
}

impl<'a, 'b, T: Rle, A: RleTreeTrait<T>> SafeCursorMut<'a, 'b, T, A> {
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

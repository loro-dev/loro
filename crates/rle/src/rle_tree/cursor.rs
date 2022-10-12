use std::{hash::Hash, marker::PhantomData, ops::Deref, ptr::NonNull};

use crate::{Rle, RleTreeTrait};

use super::{node::LeafNode, tree_trait::Position};

/// when len > 0, it acts as a selection. When iterating the tree, the len should be the size of the element.
#[derive(Debug)]
pub struct UnsafeCursor<'tree, T: Rle, A: RleTreeTrait<T>> {
    pub leaf: NonNull<LeafNode<'tree, T, A>>,
    pub index: usize,
    pub offset: usize,
    // TODO: considering remove this field, use a getter function instead
    pub pos: Position,
    pub len: usize,
    _phantom: PhantomData<&'tree usize>,
}

impl<'tree, T: Rle, A: RleTreeTrait<T>> Hash for UnsafeCursor<'tree, T, A> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.leaf.hash(state);
        self.index.hash(state);
        self.offset.hash(state);
        self.pos.hash(state);
        self.len.hash(state);
    }
}

impl<'tree, T: Rle, A: RleTreeTrait<T>> PartialEq for UnsafeCursor<'tree, T, A> {
    fn eq(&self, other: &Self) -> bool {
        self.leaf == other.leaf
            && self.index == other.index
            && self.offset == other.offset
            && self.pos == other.pos
            && self.len == other.len
    }
}

impl<'tree, T: Rle, A: RleTreeTrait<T>> Eq for UnsafeCursor<'tree, T, A> {}

impl<'tree, T: Rle, A: RleTreeTrait<T>> Clone for UnsafeCursor<'tree, T, A> {
    #[inline]
    fn clone(&self) -> Self {
        Self {
            leaf: self.leaf,
            index: self.index,
            pos: self.pos,
            offset: self.offset,
            len: self.len,
            _phantom: Default::default(),
        }
    }
}

impl<'tree, T: Rle, A: RleTreeTrait<T>> Copy for UnsafeCursor<'tree, T, A> {}

#[repr(transparent)]
#[derive(Debug)]
pub struct SafeCursor<'tree, T: Rle, A: RleTreeTrait<T>>(pub(crate) UnsafeCursor<'tree, T, A>);

#[repr(transparent)]
#[derive(Debug)]
pub struct SafeCursorMut<'tree, T: Rle, A: RleTreeTrait<T>>(pub(crate) UnsafeCursor<'tree, T, A>);

impl<'tree, T: Rle, A: RleTreeTrait<T>> Clone for SafeCursor<'tree, T, A> {
    fn clone(&self) -> Self {
        Self(self.0)
    }
}

impl<'tree, T: Rle, A: RleTreeTrait<T>> Copy for SafeCursor<'tree, T, A> {}

impl<'tree, T: Rle, A: RleTreeTrait<T>> UnsafeCursor<'tree, T, A> {
    #[inline]
    pub fn new(
        leaf: NonNull<LeafNode<'tree, T, A>>,
        index: usize,
        offset: usize,
        pos: Position,
        len: usize,
    ) -> Self {
        Self {
            leaf,
            index,
            pos,
            offset,
            len,
            _phantom: PhantomData,
        }
    }
}

impl<'tree, T: Rle, A: RleTreeTrait<T>> UnsafeCursor<'tree, T, A> {
    /// # Safety
    ///
    /// we need to make sure that the cursor is still valid
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

    /// # Safety
    ///
    /// we need to make sure that the cursor is still valid
    pub unsafe fn insert_notify<F>(mut self, value: T, notify: &mut F)
    where
        F: FnMut(&T, *mut LeafNode<'_, T, A>),
    {
        let leaf = self.leaf.as_mut();
        let result = leaf.insert_at_pos(self.pos, self.index, self.offset, value, notify);
        let mut node = leaf.parent.as_mut();
        if let Err(new) = result {
            let mut result = node.insert_at_pos(leaf.get_index_in_parent().unwrap() + 1, new);
            while let Err(new) = result {
                let old_node_index = node.get_index_in_parent().unwrap();
                // result is err, so we're sure parent is valid
                node = node.parent.unwrap().as_mut();
                result = node.insert_at_pos(old_node_index + 1, new);
            }
        } else {
            A::update_cache_internal(node);
        }

        while node.parent.is_some() {
            node = node.parent.unwrap().as_mut();
            A::update_cache_internal(node);
        }
    }

    /// # Safety
    ///
    /// we need to make sure that the cursor is still valid
    pub unsafe fn next_elem_start(&self) -> Option<Self> {
        let leaf = self.leaf.as_ref();
        if leaf.children.len() > self.index + 1 {
            return Some(Self::new(self.leaf, self.index + 1, 0, Position::Start, 0));
        }

        leaf.next
            .map(|next| Self::new(next, 0, 0, Position::Start, 0))
    }

    /// # Safety
    ///
    /// we need to make sure that the cursor is still valid
    pub unsafe fn prev_elem_end(&self) -> Option<Self> {
        let leaf = self.leaf.as_ref();
        if self.index > 0 {
            return Some(Self::new(self.leaf, self.index - 1, 0, Position::Start, 0));
        }

        leaf.prev.map(|prev| {
            Self::new(
                prev,
                prev.as_ref().children.len() - 1,
                0,
                Position::Start,
                0,
            )
        })
    }

    /// move cursor forward
    ///
    /// # Safety
    ///
    /// self should still be valid pointer
    unsafe fn shift(mut self, mut shift: usize) -> Option<Self> {
        if shift == 0 {
            return Some(self);
        }

        let mut leaf = self.leaf.as_ref();
        while shift > 0 {
            let diff = leaf.children[self.index].content_len() - self.offset;
            #[cfg(test)]
            {
                leaf.check();
            }
            match shift.cmp(&diff) {
                std::cmp::Ordering::Less => {
                    self.offset += shift;
                    self.pos = Position::Middle;
                    return Some(self);
                }
                std::cmp::Ordering::Equal => {
                    self.offset = leaf.children[self.index].content_len();
                    self.pos = Position::End;
                    return Some(self);
                }
                std::cmp::Ordering::Greater => {
                    shift -= diff;
                    if self.index == leaf.children.len() - 1 {
                        leaf = leaf.next()?;
                        self.leaf = leaf.into();
                        self.index = 0;
                        self.offset = 0;
                        self.pos = Position::Start;
                    } else {
                        self.index += 1;
                        self.offset = 0;
                        self.pos = Position::Start;
                    }
                }
            }
        }

        None
    }

    /// move cursor forward
    ///
    /// # Safety
    ///
    /// self should still be valid pointer
    pub unsafe fn update_with_split<F, U>(mut self, update_fn: U, notify: &mut F)
    where
        F: for<'a> FnMut(&T, *mut LeafNode<'a, T, A>),
        U: FnOnce(&mut T),
    {
        let leaf = self.leaf.as_mut();
        let result = leaf.update_at_pos(
            self.pos,
            self.index,
            self.offset,
            self.len,
            update_fn,
            notify,
        );
        let mut node = leaf.parent.as_mut();
        if let Err(new) = result {
            let mut result = node.insert_at_pos(leaf.get_index_in_parent().unwrap() + 1, new);
            while let Err(new) = result {
                let old_node_index = node.get_index_in_parent().unwrap();
                // result is err, so we're sure parent is valid
                node = node.parent.unwrap().as_mut();
                result = node.insert_at_pos(old_node_index + 1, new);
            }
        } else {
            A::update_cache_internal(node);
        }

        while node.parent.is_some() {
            node = node.parent.unwrap().as_mut();
            A::update_cache_internal(node);
        }
    }
}

impl<'tree, T: Rle, A: RleTreeTrait<T>> AsRef<T> for SafeCursor<'tree, T, A> {
    #[inline]
    fn as_ref(&self) -> &'tree T {
        // SAFETY: SafeCursor is a shared reference to the tree
        unsafe { self.0.as_ref() }
    }
}

impl<'tree, T: Rle, A: RleTreeTrait<T>> SafeCursor<'tree, T, A> {
    /// # Safety
    ///
    /// Users should make sure aht leaf is pointing to a valid LeafNode with 'bump lifetime, and index is inbound
    #[inline]
    pub unsafe fn new(
        leaf: NonNull<LeafNode<'tree, T, A>>,
        index: usize,
        offset: usize,
        pos: Position,
        len: usize,
    ) -> Self {
        Self(UnsafeCursor::new(leaf, index, offset, pos, len))
    }
}

impl<'tree, T: Rle, A: RleTreeTrait<T>> SafeCursor<'tree, T, A> {
    #[inline]
    pub fn as_tree_ref(&self) -> &'tree T {
        // SAFETY: SafeCursor is a shared reference to the tree
        unsafe { self.0.as_ref() }
    }

    #[inline]
    pub fn next_elem_start(&self) -> Option<Self> {
        // SAFETY: SafeCursor is a shared reference to the tree
        unsafe { self.0.next_elem_start().map(|x| Self(x)) }
    }

    #[inline]
    pub fn prev_elem_end(&self) -> Option<Self> {
        // SAFETY: SafeCursor is a shared reference to the tree
        unsafe { self.0.prev_elem_end().map(|x| Self(x)) }
    }

    #[inline]
    pub fn leaf(&self) -> &'tree LeafNode<'tree, T, A> {
        // SAFETY: SafeCursor has shared reference lifetime to the tree
        unsafe { self.0.leaf.as_ref() }
    }

    #[inline]
    pub fn index(&self) -> usize {
        self.0.index
    }

    #[inline]
    pub fn pos(&self) -> Position {
        self.0.pos
    }

    #[inline]
    pub fn offset(&self) -> usize {
        self.0.offset
    }

    #[inline]
    pub fn unwrap(self) -> UnsafeCursor<'tree, T, A> {
        self.0
    }
}

impl<'tree, 'bump: 'tree, T: Rle, A: RleTreeTrait<T>> AsRef<T> for SafeCursorMut<'tree, T, A> {
    #[inline]
    fn as_ref(&self) -> &T {
        // SAFETY: SafeCursorMut is a exclusive reference to the tree
        unsafe { self.0.as_ref() }
    }
}

impl<'tree, T: Rle, A: RleTreeTrait<T>> SafeCursorMut<'tree, T, A> {
    #[inline]
    pub fn as_ref_(&self) -> &'tree T {
        // SAFETY: SafeCursorMut is a exclusive reference to the tree
        unsafe { self.0.as_ref() }
    }

    #[inline]
    pub fn leaf(&self) -> &'tree LeafNode<'tree, T, A> {
        // SAFETY: SafeCursorMut is a exclusive reference to the tree
        unsafe { self.0.leaf.as_ref() }
    }

    #[inline]
    pub fn leaf_mut(&mut self) -> &'tree mut LeafNode<'tree, T, A> {
        // SAFETY: SafeCursorMut is a exclusive reference to the tree
        unsafe { self.0.leaf.as_mut() }
    }

    #[inline]
    pub fn child_index(&self) -> usize {
        self.0.index
    }
}

impl<'tree, T: Rle, A: RleTreeTrait<T>> SafeCursorMut<'tree, T, A> {
    /// # Safety
    ///
    /// User must be sure that there is not exclusive reference to the tree and leaf pointer is valid
    #[inline]
    pub unsafe fn new(
        leaf: NonNull<LeafNode<'tree, T, A>>,
        index: usize,
        offset: usize,
        pos: Position,
        len: usize,
    ) -> Self {
        Self(UnsafeCursor::new(leaf, index, offset, pos, len))
    }

    #[inline]
    fn as_tree_mut(&mut self) -> &'tree mut T {
        // SAFETY: SafeCursorMut is a exclusive reference to the tree
        unsafe { self.0.as_mut() }
    }

    #[inline]
    pub fn update_cache_recursively(&mut self) {
        // SAFETY: SafeCursorMut is a exclusive reference to the tree
        unsafe {
            let leaf = self.0.leaf.as_mut();
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

    #[inline]
    pub fn unwrap(self) -> UnsafeCursor<'tree, T, A> {
        self.0
    }

    #[inline]
    pub fn next_elem_start(&self) -> Option<Self> {
        // SAFETY: SafeCursorMut is a exclusive reference to the tree so we are safe to
        // get a reference to the element
        unsafe { self.0.next_elem_start().map(|x| Self(x)) }
    }

    #[inline]
    pub fn prev_elem_end(&self) -> Option<Self> {
        // SAFETY: SafeCursorMut is a exclusive reference to the tree so we are safe to
        // get a reference to the element
        unsafe { self.0.prev_elem_end().map(|x| Self(x)) }
    }

    #[inline]
    pub fn index(&self) -> usize {
        self.0.index
    }

    #[inline]
    pub fn pos(&self) -> Position {
        self.0.pos
    }

    #[inline]
    pub fn offset(&self) -> usize {
        self.0.offset
    }

    /// self should be moved here, because after mutating self should be invalidate
    pub fn insert_before_notify<F>(mut self, value: T, notify: &mut F)
    where
        F: FnMut(&T, *mut LeafNode<'_, T, A>),
    {
        // SAFETY: we know the cursor is a valid pointer
        unsafe { self.0.insert_notify(value, notify) }
    }

    /// self should be moved here, because after mutating self should be invalidate
    pub fn insert_after_notify<F>(self, value: T, notify: &mut F)
    where
        F: FnMut(&T, *mut LeafNode<'_, T, A>),
    {
        // SAFETY: we know the cursor is a valid pointer
        unsafe {
            self.0
                .shift(self.0.len)
                .unwrap()
                .insert_notify(value, notify)
        }
    }

    /// insert to the cursor start position with shift in offset. `shift` is based on the content_len.
    ///
    /// self should be moved here, because after mutating self should be invalidate
    pub fn insert_shift_notify<F>(self, value: T, shift: usize, notify: &mut F)
    where
        F: FnMut(&T, *mut LeafNode<'_, T, A>),
    {
        // SAFETY: we know the cursor is a valid pointer
        unsafe { self.0.shift(shift).unwrap().insert_notify(value, notify) }
    }

    pub fn update_with_split<F, U>(self, update: U, notify: &mut F)
    where
        F: for<'a> FnMut(&T, *mut LeafNode<'a, T, A>),
        U: FnOnce(&mut T),
    {
        // SAFETY: we know the cursor is a valid pointer
        unsafe { self.0.update_with_split(update, notify) }
    }
}

impl<'tree, T: Rle, A: RleTreeTrait<T>> AsMut<T> for SafeCursorMut<'tree, T, A> {
    #[inline]
    fn as_mut(&mut self) -> &mut T {
        // SAFETY: SafeCursorMut is a exclusive reference to the tree so we are safe to
        // get a exclusive reference to the element
        unsafe { self.0.as_mut() }
    }
}

impl<'tree, T: Rle, A: RleTreeTrait<T>> Deref for SafeCursor<'tree, T, A> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.as_ref()
    }
}

impl<'tree, T: Rle, A: RleTreeTrait<T>> Deref for SafeCursorMut<'tree, T, A> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.as_ref()
    }
}

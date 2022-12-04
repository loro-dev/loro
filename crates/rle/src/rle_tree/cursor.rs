use std::{hash::Hash, marker::PhantomData, ptr::NonNull};

use crdt_list::crdt::GetOp;
use num::FromPrimitive;

use crate::{HasLength, Rle, RleTreeTrait};

use super::{node::LeafNode, tree_trait::Position};

/// when len > 0, it acts as a selection. When iterating the tree, the len should be the size of the element.
#[derive(Debug)]
pub struct UnsafeCursor<'tree, T: Rle, A: RleTreeTrait<T> + 'tree> {
    pub leaf: NonNull<LeafNode<'tree, T, A>>,
    pub index: usize,
    pub offset: usize,
    // TODO: considering remove this field, use a getter function instead
    pub pos: Position,
    pub len: usize,
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
        }
    }
}

#[derive(Debug)]
pub struct Im;
#[derive(Debug)]
pub struct Mut;

#[derive(Debug)]
#[repr(transparent)]
pub struct RawSafeCursor<'tree, T: Rle, A: RleTreeTrait<T>, State>(
    pub(crate) UnsafeCursor<'tree, T, A>,
    PhantomData<State>,
);

pub type SafeCursor<'tree, T, A> = RawSafeCursor<'tree, T, A, Im>;
pub type SafeCursorMut<'tree, T, A> = RawSafeCursor<'tree, T, A, Mut>;

impl<'tree, T: Rle, A: RleTreeTrait<T>> Clone for SafeCursor<'tree, T, A> {
    fn clone(&self) -> Self {
        Self(self.0.clone(), PhantomData)
    }
}

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
        }
    }
}

impl<'tree, T: Rle, A: RleTreeTrait<T>> UnsafeCursor<'tree, T, A> {
    /// # Safety
    ///
    /// we need to make sure that the cursor is still valid
    #[inline]
    pub unsafe fn as_ref(&self) -> &'tree T {
        &self.leaf.as_ref().children[self.index]
    }

    #[inline]
    unsafe fn as_mut(&mut self) -> &'tree mut T {
        &mut self.leaf.as_mut().children[self.index]
    }

    /// # Safety
    ///
    /// we need to make sure that the cursor is still valid
    pub unsafe fn insert_notify<F>(mut self, value: T, notify: &mut F)
    where
        F: FnMut(&T, *mut LeafNode<'_, T, A>),
    {
        let update = A::value_to_update(&value);
        let leaf = self.leaf.as_mut();
        let mut node = leaf.parent.as_mut();
        // println!("insert cursor {:?}", self);
        // println!("insert value {:?}", value);
        // dbg!(&leaf);
        let result = leaf.insert_at_pos(self.pos, self.index, self.offset, value, notify, false);
        // dbg!(&leaf);
        let self_index = leaf.get_index_in_parent().unwrap();
        let leaf = &mut node.children[self_index];
        leaf.cache = leaf.node.cache();
        match result {
            Ok(hint) => {
                A::update_cache_internal(node, Some(hint));
            }
            Err((hint, new)) => {
                A::update_cache_internal(node, Some(hint));
                let mut result = node.insert_at_pos(self_index + 1, new);
                while let Err((update, new)) = result {
                    let old_node_index = node.get_index_in_parent().unwrap();
                    // result is err, so we're sure parent is valid
                    node = node.parent.unwrap().as_mut();
                    result = node.insert_at_pos(old_node_index + 1, new);
                }
            }
        }

        while node.parent.is_some() {
            let index = node.get_index_in_parent().unwrap();
            node = node.parent.unwrap().as_mut();
            node.children[index].cache += update;
            A::update_cache_internal(node, Some(update));
        }
        node.check();
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
    pub unsafe fn prev_elem(&self) -> Option<Self> {
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

    /// # Safety
    ///
    /// we need to make sure that the cursor is still valid
    pub unsafe fn get_sliced(&self) -> T {
        self.as_ref().slice(self.offset, self.offset + self.len)
    }

    /// # Safety
    ///
    /// we need to make sure that the leaf is still valid
    pub unsafe fn get_index(&self) -> A::Int {
        let leaf = self.leaf.as_ref();
        let index = A::get_index(leaf, self.index);
        let item = self.as_ref();
        if item.content_len() == 0 {
            index
        } else {
            index + A::Int::from_usize(self.offset).unwrap()
        }
    }

    /// move cursor forward
    ///
    /// # Safety
    ///
    /// self should still be valid pointer
    pub unsafe fn shift(mut self, mut shift: usize) -> Option<Self> {
        if shift == 0 {
            return Some(self);
        }

        let mut leaf = self.leaf.as_ref();
        if self.len < shift {
            self.len = 0;
        } else {
            self.len -= shift;
        }

        while shift > 0 {
            let diff = leaf.children[self.index].atom_len() - self.offset;
            match shift.cmp(&diff) {
                std::cmp::Ordering::Less => {
                    self.offset += shift;
                    self.pos = Position::Middle;
                    return Some(self);
                }
                std::cmp::Ordering::Equal => {
                    self.offset = leaf.children[self.index].atom_len();
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
            let mut result = node.insert_at_pos(leaf.get_index_in_parent().unwrap() + 1, new.1);
            while let Err((update, new)) = result {
                let old_node_index = node.get_index_in_parent().unwrap();
                // result is err, so we're sure parent is valid
                node = node.parent.unwrap().as_mut();
                result = node.insert_at_pos(old_node_index + 1, new);
            }
        } else {
            // TODO: Perf
            A::update_cache_internal(node, None);
        }

        while node.parent.is_some() {
            node = node.parent.unwrap().as_mut();
            // TODO: Perf
            A::update_cache_internal(node, None);
        }
    }
}

impl<'tree, T: Rle, A: RleTreeTrait<T>, M> AsRef<T> for RawSafeCursor<'tree, T, A, M> {
    #[inline]
    fn as_ref(&self) -> &'tree T {
        // SAFETY: SafeCursor is a shared reference to the tree
        unsafe { self.0.as_ref() }
    }
}

impl<'tree, T: Rle, A: RleTreeTrait<T>, M> RawSafeCursor<'tree, T, A, M> {
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
        Self(
            UnsafeCursor::new(leaf, index, offset, pos, len),
            PhantomData,
        )
    }
}

impl<'tree, T: Rle, A: RleTreeTrait<T>> RawSafeCursor<'tree, T, A, Im> {
    #[inline]
    pub fn from_leaf(
        leaf: &LeafNode<'tree, T, A>,
        index: usize,
        offset: usize,
        pos: Position,
        len: usize,
    ) -> Self {
        Self(
            UnsafeCursor::new(leaf.into(), index, offset, pos, len),
            PhantomData,
        )
    }
}

impl<'tree, T: Rle, A: RleTreeTrait<T>> RawSafeCursor<'tree, T, A, Mut> {
    #[inline]
    pub fn from_leaf(
        leaf: &mut LeafNode<'tree, T, A>,
        index: usize,
        offset: usize,
        pos: Position,
        len: usize,
    ) -> Self {
        Self(
            UnsafeCursor::new(leaf.into(), index, offset, pos, len),
            PhantomData,
        )
    }
}

impl<'tree, T: Rle, A: RleTreeTrait<T>, M> RawSafeCursor<'tree, T, A, M> {
    #[inline]
    pub fn as_tree_ref(&self) -> &'tree T {
        // SAFETY: SafeCursor is a shared reference to the tree
        unsafe { self.0.as_ref() }
    }

    #[inline]
    pub fn as_tree_mut(&mut self) -> &'tree mut T {
        // SAFETY: SafeCursor is a shared reference to the tree
        unsafe { self.0.as_mut() }
    }

    #[inline]
    pub fn next_elem_start(&self) -> Option<Self> {
        // SAFETY: SafeCursor is a shared reference to the tree
        unsafe { self.0.next_elem_start().map(|x| Self(x, PhantomData)) }
    }

    #[inline]
    pub fn prev_elem(&self) -> Option<Self> {
        // SAFETY: SafeCursor is a shared reference to the tree
        unsafe { self.0.prev_elem().map(|x| Self(x, PhantomData)) }
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

    #[inline]
    pub fn shift(self, shift: usize) -> Option<Self> {
        // SAFETY: SafeCursor is a shared reference to the tree
        unsafe { Some(Self(self.0.shift(shift)?, PhantomData)) }
    }

    pub fn get_sliced(&self) -> T {
        self.as_ref()
            .slice(self.0.offset, self.0.offset + self.0.len)
    }
}

impl<'tree, T: Rle, A: RleTreeTrait<T>, M> GetOp for RawSafeCursor<'tree, T, A, M> {
    type Target = T;

    fn get_op(&self) -> Self::Target {
        self.as_ref()
            .slice(self.offset(), self.offset() + self.content_len())
    }
}

impl<'tree, T: Rle, A: RleTreeTrait<T>, M> HasLength for RawSafeCursor<'tree, T, A, M> {
    fn content_len(&self) -> usize {
        self.0.len
    }
}

impl<'tree, T: Rle, A: RleTreeTrait<T>> SafeCursorMut<'tree, T, A> {
    #[inline(always)]
    pub unsafe fn from(cursor: UnsafeCursor<'tree, T, A>) -> Self {
        Self(cursor, PhantomData)
    }

    #[inline]
    pub fn update_cache_recursively(&mut self) {
        // SAFETY: SafeCursorMut is a exclusive reference to the tree
        unsafe {
            let leaf = self.0.leaf.as_mut();
            let mut update = A::update_cache_leaf(leaf);
            let mut node = leaf.parent.as_mut();
            loop {
                update = A::update_cache_internal(node, Some(update));
                match node.parent {
                    Some(mut parent) => node = parent.as_mut(),
                    None => return,
                }
            }
        }
    }

    /// self should be moved here, because after mutating self should be invalidate
    pub fn insert_before_notify<F>(self, value: T, notify: &mut F)
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
        let len = self.0.len;
        // SAFETY: we know the cursor is a valid pointer
        unsafe { self.0.shift(len).unwrap().insert_notify(value, notify) }
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

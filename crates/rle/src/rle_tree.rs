use self::node::{InternalNode, LeafNode, Node};
use crate::Rle;
pub(self) use bumpalo::collections::vec::Vec as BumpVec;
use bumpalo::Bump;
pub use cursor::{SafeCursor, SafeCursorMut};
use ouroboros::self_referencing;
use std::marker::{PhantomData, PhantomPinned};
use tree_trait::RleTreeTrait;

mod cursor;
pub mod iter;
pub mod node;
pub mod nonnull;
#[cfg(test)]
mod test;
pub mod tree_trait;

#[derive(Debug)]
pub struct RleTreeRaw<'a, T: Rle, A: RleTreeTrait<T>> {
    node: Node<'a, T, A>,
    _pin: PhantomPinned,
    _a: PhantomData<(A, T)>,
}

#[self_referencing]
#[derive(Debug)]
pub struct RleTree<T: Rle + 'static, A: RleTreeTrait<T> + 'static> {
    bump: Bump,
    #[borrows(bump)]
    pub tree: &'this mut RleTreeRaw<'this, T, A>,
}

impl<T: Rle + 'static, A: RleTreeTrait<T> + 'static> Default for RleTree<T, A> {
    fn default() -> Self {
        RleTreeBuilder {
            bump: Bump::new(),
            tree_builder: |bump| bump.alloc(RleTreeRaw::new(bump)),
        }
        .build()
    }
}

impl<'a, T: Rle, A: RleTreeTrait<T>> RleTreeRaw<'a, T, A> {
    #[inline]
    fn new(bump: &'a Bump) -> Self {
        Self {
            node: Node::Internal(InternalNode::new(bump, None)),
            _pin: PhantomPinned,
            _a: PhantomData,
        }
    }

    #[inline]
    pub fn insert(&mut self, index: A::Int, value: T) {
        self.node
            .as_internal_mut()
            .unwrap()
            .insert(index, value, &mut |_a, _b| {})
            .unwrap();
    }

    /// `notify` would be invoke if a new element is inserted/moved to a new leaf node.
    #[inline]
    pub fn insert_notify<F>(&mut self, index: A::Int, value: T, notify: &mut F)
    where
        F: FnMut(&T, *mut LeafNode<'_, T, A>),
    {
        self.node
            .as_internal_mut()
            .unwrap()
            .insert(index, value, notify)
            .unwrap();
    }

    /// return a cursor at the given index
    #[inline]
    pub fn get<'b>(&'b self, mut index: A::Int) -> Option<SafeCursor<'a, 'b, T, A>> {
        let mut node = &self.node;
        loop {
            match node {
                Node::Internal(internal_node) => {
                    let result = A::find_pos_internal(internal_node, index);
                    if !result.found {
                        return None;
                    }

                    node = internal_node.children[result.child_index];
                    index = result.offset;
                }
                Node::Leaf(leaf) => {
                    let result = A::find_pos_leaf(leaf, index);
                    if !result.found {
                        return None;
                    }

                    return Some(SafeCursor::new(leaf.into(), result.child_index));
                }
            }
        }
    }

    /// return the first valid cursor after the given index
    #[inline]
    pub fn get_cursor_after<'b>(&'b self, mut index: A::Int) -> Option<SafeCursor<'a, 'b, T, A>> {
        let mut node = &self.node;
        loop {
            match node {
                Node::Internal(internal_node) => {
                    let result = A::find_pos_internal(internal_node, index);
                    if result.child_index >= internal_node.children.len() {
                        return None;
                    }

                    node = internal_node.children[result.child_index];
                    index = result.offset;
                }
                Node::Leaf(leaf) => {
                    let result = A::find_pos_leaf(leaf, index);
                    if result.child_index >= leaf.children.len() {
                        return None;
                    }

                    return Some(SafeCursor::new(leaf.into(), result.child_index));
                }
            }
        }
    }

    #[inline]
    pub fn get_mut<'b>(&'b mut self, index: A::Int) -> Option<SafeCursorMut<'a, 'b, T, A>> {
        let cursor = self.get(index);
        cursor.map(|x| SafeCursorMut(x.0))
    }

    pub fn iter(&self) -> iter::Iter<'_, 'a, T, A> {
        iter::Iter::new(self.node.get_first_leaf())
    }

    pub fn delete_range(&mut self, start: Option<A::Int>, end: Option<A::Int>) {
        self.node
            .as_internal_mut()
            .unwrap()
            .delete(start, end, &mut |_, _| {});
    }

    pub fn delete_range_notify<F>(
        &mut self,
        start: Option<A::Int>,
        end: Option<A::Int>,
        notify: &mut F,
    ) where
        F: FnMut(&T, *mut LeafNode<'_, T, A>),
    {
        self.node
            .as_internal_mut()
            .unwrap()
            .delete(start, end, notify);
    }

    pub fn iter_range(&self, start: A::Int, end: Option<A::Int>) -> iter::Iter<'_, 'a, T, A> {
        let cursor_from = self.get_cursor_after(start);
        if cursor_from.is_none() {
            return iter::Iter::new(None);
        }

        let cursor_from = cursor_from.unwrap();
        unsafe {
            if end.is_none() || end.unwrap() >= self.len() {
                iter::Iter::new_with_end(
                    cursor_from.0.leaf.as_ref(),
                    cursor_from.0.index,
                    None,
                    None,
                )
            } else if let Some(cursor_to) = self.get_cursor_after(end.unwrap()) {
                let node = cursor_from.0.leaf.as_ref();
                let end_node = cursor_to.0.leaf.as_ref();
                let mut end_index = cursor_to.0.index;
                if std::ptr::eq(node, end_node) && end_index == cursor_from.0.index {
                    end_index += 1;
                }

                iter::Iter::new_with_end(
                    cursor_from.0.leaf.as_ref(),
                    cursor_from.0.index,
                    Some(end_node),
                    Some(end_index),
                )
            } else {
                iter::Iter::new_with_end(
                    cursor_from.0.leaf.as_ref(),
                    cursor_from.0.index,
                    None,
                    None,
                )
            }
        }
    }

    pub fn debug_check(&mut self) {
        self.node.as_internal_mut().unwrap().check();
    }
}

impl<'a, T: Rle, A: RleTreeTrait<T>> RleTreeRaw<'a, T, A> {
    #[inline]
    pub fn len(&self) -> A::Int {
        self.node.len()
    }
}

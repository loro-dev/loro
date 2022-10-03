use self::node::{InternalNode, LeafNode, Node};
use crate::Rle;
pub(self) use bumpalo::collections::vec::Vec as BumpVec;
use bumpalo::Bump;
pub use cursor::{SafeCursor, SafeCursorMut, UnsafeCursor};
use num::FromPrimitive;
use ouroboros::self_referencing;
use std::marker::{PhantomData, PhantomPinned};
pub use tree_trait::Position;
use tree_trait::RleTreeTrait;

mod cursor;
pub mod iter;
pub mod node;
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

impl<'bump, T: Rle, A: RleTreeTrait<T>> RleTreeRaw<'bump, T, A> {
    #[inline]
    fn new(bump: &'bump Bump) -> Self {
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
    pub fn get<'tree>(&'tree self, mut index: A::Int) -> Option<SafeCursor<'tree, 'bump, T, A>> {
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

                    // SAFETY: result is valid
                    return Some(unsafe {
                        SafeCursor::new(leaf.into(), result.child_index, result.offset, result.pos)
                    });
                }
            }
        }
    }

    /// return the first valid cursor after the given index
    #[inline]
    fn get_cursor_ge<'b>(&'b self, mut index: A::Int) -> Option<SafeCursor<'b, 'bump, T, A>> {
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

                    // SAFETY: result is valid
                    return Some(unsafe {
                        SafeCursor::new(leaf.into(), result.child_index, result.offset, result.pos)
                    });
                }
            }
        }
    }

    #[inline]
    pub fn get_mut<'b>(&'b mut self, index: A::Int) -> Option<SafeCursorMut<'b, 'bump, T, A>> {
        let cursor = self.get(index);
        cursor.map(|x| SafeCursorMut(x.0))
    }

    #[inline]
    pub fn iter(&self) -> iter::Iter<'_, 'bump, T, A> {
        iter::Iter::new(self.node.get_first_leaf())
    }

    #[inline]
    pub fn iter_mut(&mut self) -> iter::IterMut<'_, 'bump, T, A> {
        iter::IterMut::new(self.node.get_first_leaf_mut())
    }

    #[inline]
    pub fn empty(&self) -> bool {
        self.len() == A::Int::from_usize(0).unwrap()
    }

    pub fn iter_mut_in<'tree>(
        &'tree mut self,
        start: Option<SafeCursor<'tree, 'bump, T, A>>,
        end: Option<SafeCursor<'tree, 'bump, T, A>>,
    ) -> iter::IterMut<'tree, 'bump, T, A> {
        if self.empty() || (start.is_none() && end.is_none()) {
            self.iter_mut()
        } else {
            // SAFETY: this is safe because we know there are at least one element in the tree
            let start = start.unwrap_or_else(|| unsafe {
                SafeCursor::new(
                    self.node.get_first_leaf().unwrap().into(),
                    0,
                    0,
                    Position::Start,
                )
            });

            let start: SafeCursorMut<'tree, 'bump, T, A> = SafeCursorMut(start.0);
            iter::IterMut::from_cursor(start, end).unwrap_or_else(|| self.iter_mut())
        }
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

    pub fn iter_range(&self, start: A::Int, end: Option<A::Int>) -> iter::Iter<'_, 'bump, T, A> {
        let cursor_from = self.get_cursor_ge(start);
        if cursor_from.is_none() {
            return iter::Iter::new(None);
        }

        let cursor_from = cursor_from.unwrap();
        if let Some(ans) = {
            if let Some(end) = end {
                let cursor_to = self.get_cursor_ge(end);
                iter::Iter::from_cursor(cursor_from, cursor_to)
            } else {
                None
            }
        } {
            ans
        } else {
            iter::Iter::from_cursor(cursor_from, None).unwrap()
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

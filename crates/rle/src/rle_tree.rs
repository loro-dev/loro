use std::{collections::HashMap, ptr::NonNull};

use self::node::{InternalNode, LeafNode, Node};
use crate::Rle;
pub(self) use bumpalo::collections::vec::Vec as BumpVec;
use bumpalo::Bump;
pub use cursor::{SafeCursor, SafeCursorMut, UnsafeCursor};
use fxhash::FxHashMap;
use num::FromPrimitive;
use ouroboros::self_referencing;
pub use tree_trait::Position;
use tree_trait::RleTreeTrait;

mod cursor;
pub mod iter;
pub mod node;
#[cfg(test)]
mod test;
pub mod tree_trait;

#[self_referencing]
#[derive(Debug)]
pub struct RleTree<T: Rle + 'static, A: RleTreeTrait<T> + 'static> {
    bump: Bump,
    #[borrows(bump)]
    node: &'this mut Node<'this, T, A>,
}

impl<T: Rle + 'static, A: RleTreeTrait<T> + 'static> Default for RleTree<T, A> {
    fn default() -> Self {
        RleTreeBuilder {
            bump: Bump::new(),
            node_builder: |bump| bump.alloc(Node::Internal(InternalNode::new(bump, None))),
        }
        .build()
    }
}

impl<T: Rle, A: RleTreeTrait<T>> RleTree<T, A> {
    pub fn insert_at_first<F>(&mut self, value: T, notify: &mut F)
    where
        F: FnMut(&T, *mut LeafNode<'_, T, A>),
    {
        if let Some(value) = self.with_node_mut(|node| {
            let leaf = node.get_first_leaf();
            if let Some(leaf) = leaf {
                // SAFETY: we have exclusive ref to the tree
                let cursor = unsafe { SafeCursorMut::new(leaf.into(), 0, 0, Position::Start, 0) };
                cursor.insert_before_notify(value, notify);
                None
            } else {
                Some(value)
            }
        }) {
            self.insert_notify(A::Int::from_u8(0).unwrap(), value, notify);
        }
    }

    #[inline]
    pub fn insert(&mut self, index: A::Int, value: T) {
        self.with_node_mut(|node| {
            node.as_internal_mut()
                .unwrap()
                .insert(index, value, &mut |_a, _b| {})
                .unwrap();
        })
    }

    /// `notify` would be invoke if a new element is inserted/moved to a new leaf node.
    #[inline]
    pub fn insert_notify<F>(&mut self, index: A::Int, value: T, notify: &mut F)
    where
        F: FnMut(&T, *mut LeafNode<'_, T, A>),
    {
        self.with_node_mut(|node| {
            node.as_internal_mut()
                .unwrap()
                .insert(index, value, notify)
                .unwrap();
        })
    }

    /// return a cursor at the given index
    #[inline]
    pub fn get(&self, mut index: A::Int) -> Option<SafeCursor<'_, T, A>> {
        self.with_node(|mut node| {
            loop {
                match node {
                    Node::Internal(internal_node) => {
                        let result = A::find_pos_internal(internal_node, index);
                        if !result.found {
                            return None;
                        }

                        node = &internal_node.children[result.child_index];
                        index = result.offset;
                    }
                    Node::Leaf(leaf) => {
                        let result = A::find_pos_leaf(leaf, index);
                        if !result.found {
                            return None;
                        }

                        // SAFETY: result is valid
                        return Some(unsafe {
                            std::mem::transmute(SafeCursor::new(
                                leaf.into(),
                                result.child_index,
                                result.offset,
                                result.pos,
                                0,
                            ))
                        });
                    }
                }
            }
        })
    }

    /// return the first valid cursor after the given index
    /// reviewed by @Leeeon233
    #[inline]
    fn get_cursor_ge(&self, mut index: A::Int) -> Option<SafeCursor<'_, T, A>> {
        self.with_node(|mut node| {
            loop {
                match node {
                    Node::Internal(internal_node) => {
                        let result = A::find_pos_internal(internal_node, index);
                        if result.child_index >= internal_node.children.len() {
                            return None;
                        }

                        node = &internal_node.children[result.child_index];
                        index = result.offset;
                    }
                    Node::Leaf(leaf) => {
                        let result = A::find_pos_leaf(leaf, index);
                        if result.child_index >= leaf.children.len() {
                            return None;
                        }

                        // SAFETY: result is valid
                        return Some(unsafe {
                            std::mem::transmute(SafeCursor::new(
                                leaf.into(),
                                result.child_index,
                                result.offset,
                                result.pos,
                                0,
                            ))
                        });
                    }
                }
            }
        })
    }

    #[inline]
    pub fn get_mut(&mut self, index: A::Int) -> Option<SafeCursorMut<'_, T, A>> {
        let cursor = self.get(index);
        cursor.map(|x| SafeCursorMut(x.0))
    }

    #[inline]
    pub fn iter(&self) -> iter::Iter<'_, T, A> {
        // SAFETY: the cursor and iter cannot outlive self
        self.with_node(|node| unsafe {
            iter::Iter::new(std::mem::transmute(node.get_first_leaf()))
        })
    }

    #[inline]
    pub fn iter_mut(&mut self) -> iter::IterMut<'_, T, A> {
        // SAFETY: the cursor and iter cannot outlive self
        self.with_node_mut(|node| unsafe {
            iter::IterMut::new(std::mem::transmute(node.get_first_leaf_mut()))
        })
    }

    #[inline]
    pub fn empty(&self) -> bool {
        self.len() == A::Int::from_usize(0).unwrap()
    }

    pub fn iter_mut_in(
        &mut self,
        start: Option<SafeCursor<'_, T, A>>,
        end: Option<SafeCursor<'_, T, A>>,
    ) -> iter::IterMut<'_, T, A> {
        if self.empty() || (start.is_none() && end.is_none()) {
            self.iter_mut()
        } else {
            // SAFETY: the cursor cannot outlive self, so we are safe here
            self.with_node_mut(|node| unsafe {
                let leaf = node.get_first_leaf().unwrap().into();
                // SAFETY: this is safe because we know there are at least one element in the tree
                let start = start.unwrap_or_else(|| {
                    std::mem::transmute(SafeCursor::new(leaf, 0, 0, Position::Start, 0))
                });
                let start: SafeCursorMut<'_, T, A> = SafeCursorMut(start.0);
                std::mem::transmute::<_, iter::IterMut<'_, T, A>>(iter::IterMut::from_cursor(
                    std::mem::transmute::<_, SafeCursorMut<'_, T, A>>(start),
                    end,
                ))
            })
        }
    }

    pub fn delete_range(&mut self, start: Option<A::Int>, end: Option<A::Int>) {
        self.with_node_mut(|node| {
            node.as_internal_mut()
                .unwrap()
                .delete(start, end, &mut |_, _| {});
        })
    }

    pub fn delete_range_notify<F>(
        &mut self,
        start: Option<A::Int>,
        end: Option<A::Int>,
        notify: &mut F,
    ) where
        F: FnMut(&T, *mut LeafNode<'_, T, A>),
    {
        self.with_node_mut(|node| {
            node.as_internal_mut().unwrap().delete(start, end, notify);
        })
    }

    /// reviewed by @Leeeon233
    pub fn iter_range(&self, start: A::Int, end: Option<A::Int>) -> iter::Iter<'_, T, A> {
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
            iter::Iter::from_cursor(cursor_from, None).unwrap_or_default()
        }
    }

    pub fn update_at_cursors<U, F>(
        &mut self,
        cursors: Vec<UnsafeCursor<T, A>>,
        update_fn: &mut U,
        notify: &mut F,
    ) where
        U: FnMut(&mut T),
        F: FnMut(&T, *mut LeafNode<T, A>),
    {
        let mut updates_map: HashMap<NonNull<_>, Vec<(usize, Vec<T>)>, _> = FxHashMap::default();
        for cursor in cursors {
            // SAFETY: we has the exclusive reference to the tree and the cursor is valid
            let updates = unsafe {
                cursor
                    .leaf
                    .as_ref()
                    .pure_update(cursor.index, cursor.offset, cursor.len, update_fn)
            };

            if let Some(update) = updates {
                updates_map
                    .entry(cursor.leaf)
                    .or_default()
                    .push((cursor.index, update));
            }
        }

        let mut internal_updates_map: HashMap<NonNull<_>, Vec<(usize, Vec<_>)>, _> =
            FxHashMap::default();
        for (mut leaf, updates) in updates_map {
            // SAFETY: we has the exclusive reference to the tree and the cursor is valid
            let leaf = unsafe { leaf.as_mut() };
            if let Err(new) = leaf.apply_updates(updates, notify) {
                internal_updates_map
                    .entry(leaf.parent)
                    .or_default()
                    .push((leaf.get_index_in_parent().unwrap(), new));
            } else {
                // insert empty value to trigger cache update
                internal_updates_map.entry(leaf.parent).or_default();
            }
        }

        while !internal_updates_map.is_empty() {
            let updates_map = std::mem::take(&mut internal_updates_map);
            for (mut node, updates) in updates_map {
                // SAFETY: we has the exclusive reference to the tree and the cursor is valid
                let node = unsafe { node.as_mut() };

                if let Err(new) = node.apply_updates(updates) {
                    internal_updates_map
                        .entry(node.parent.unwrap())
                        .or_default()
                        .push((node.get_index_in_parent().unwrap(), new));
                } else if node.parent.is_some() {
                    // insert empty value to trigger cache update
                    internal_updates_map
                        .entry(node.parent.unwrap())
                        .or_default();
                } else {
                    A::update_cache_internal(node);
                }
            }
        }
    }

    pub fn update_range<U, F>(
        &mut self,
        start: A::Int,
        end: Option<A::Int>,
        update_fn: &mut U,
        notify: &mut F,
    ) where
        U: FnMut(&mut T),
        F: FnMut(&T, *mut LeafNode<'_, T, A>),
    {
        let mut cursors = Vec::new();
        for cursor in self.iter_range(start, end) {
            cursors.push(cursor.0);
        }

        // SAFETY: it's perfectly safe here because we know what we are doing in the update_at_cursors
        self.update_at_cursors(unsafe { std::mem::transmute(cursors) }, update_fn, notify);
    }

    pub fn debug_check(&mut self) {
        self.with_node_mut(|node| {
            node.as_internal_mut().unwrap().check();
        })
    }

    // pub fn iter_cursor_mut(&mut self) -> impl Iterator<Item = SafeCursorMut<'_, T, A>> {}
}

impl<T: Rle, A: RleTreeTrait<T>> RleTree<T, A> {
    #[inline]
    pub fn len(&self) -> A::Int {
        self.with_node(|node| node.len())
    }
}

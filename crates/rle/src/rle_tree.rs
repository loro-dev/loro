use std::{collections::HashMap, ptr::NonNull};

use self::node::{InternalNode, LeafNode, Node};
use crate::Rle;
pub(self) use bumpalo::collections::vec::Vec as BumpVec;
use bumpalo::Bump;
pub use cursor::{SafeCursor, SafeCursorMut, UnsafeCursor};
use fxhash::FxHashMap;
use num::FromPrimitive;
use ouroboros::self_referencing;

use smallvec::SmallVec;
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
    pub(crate) bump: Bump,
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
    fn root(&self) -> &Node<T, A> {
        // SAFETY: self can be shared ref so the root node must be valid and can be shared ref
        self.with_node(|node| unsafe { std::mem::transmute::<_, &Node<T, A>>(&**node) })
    }

    fn root_mut(&mut self) -> &mut Node<T, A> {
        // SAFETY: self can be exclusively ref so the root node must be valid and can be exclusively ref
        self.with_node_mut(|node| unsafe { std::mem::transmute::<_, &mut Node<T, A>>(&mut **node) })
    }

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
        let mut node = self.root();
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

                    return Some(SafeCursor::from_leaf(
                        leaf,
                        result.child_index,
                        result.offset,
                        result.pos,
                        0,
                    ));
                }
            }
        }
    }

    /// return the first valid cursor after the given index
    /// reviewed by @Leeeon233
    #[inline]
    pub(crate) fn get_cursor_ge(&self, mut index: A::Int) -> Option<SafeCursor<'_, T, A>> {
        let mut node = self.root();
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

                    if result.child_index == leaf.children.len() - 1
                        && leaf.next().is_none()
                        && !result.found
                        && (result.pos == Position::After || result.pos == Position::End)
                    {
                        return None;
                    }

                    return Some(SafeCursor::from_leaf(
                        leaf,
                        result.child_index,
                        result.offset,
                        result.pos,
                        0,
                    ));
                }
            }
        }
    }

    #[inline]
    pub fn get_mut(&mut self, index: A::Int) -> Option<SafeCursorMut<'_, T, A>> {
        let cursor = self.get(index);
        // SAFETY: this is safe because we have exclusive ref to the tree
        cursor.map(|x| unsafe { SafeCursorMut::from(x.0) })
    }

    #[inline]
    pub fn iter(&self) -> iter::Iter<'_, T, A> {
        iter::Iter::new(self.root().get_first_leaf())
    }

    #[inline]
    pub fn iter_mut(&mut self) -> iter::IterMut<'_, T, A> {
        // SAFETY: the cursor and iter cannot outlive self
        iter::IterMut::new(self.root_mut().get_first_leaf_mut())
    }

    #[inline]
    pub fn empty(&self) -> bool {
        self.len() == A::Int::from_usize(0).unwrap()
    }

    pub fn iter_mut_in<'a>(
        &'a mut self,
        start: Option<SafeCursor<'a, T, A>>,
        end: Option<SafeCursor<'a, T, A>>,
    ) -> iter::IterMut<'a, T, A> {
        if start.is_none() && end.is_none() {
            self.iter_mut()
        } else {
            let leaf = self.root().get_first_leaf().unwrap();
            // SAFETY: this is safe because we know there are at least one element in the tree
            let start =
                start.unwrap_or_else(|| SafeCursor::from_leaf(leaf, 0, 0, Position::Start, 0));

            // SAFETY: we have exclusive ref to the tree, so it's safe to have an exclusive ref to its elements
            let start: SafeCursorMut<'a, T, A> = unsafe { SafeCursorMut::from(start.0) };
            iter::IterMut::from_cursor(
                start,
                end.map(|x| UnsafeCursor::new(x.0.leaf, x.0.index, x.0.offset, x.0.pos, 0)),
            )
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
                iter::Iter::from_cursor(cursor_from.clone(), cursor_to)
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
        cursors: &mut [UnsafeCursor<T, A>],
        update_fn: &mut U,
        notify: &mut F,
    ) where
        U: FnMut(&mut T),
        F: FnMut(&T, *mut LeafNode<T, A>),
    {
        let mut updates_map: HashMap<NonNull<_>, Vec<(usize, SmallVec<[T; 2]>)>, _> =
            FxHashMap::default();
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

        self.update_with_gathered_map(updates_map, notify);
    }

    // TODO: perf, use smallvec
    pub fn update_at_cursors_with_args<U, F, Arg>(
        &mut self,
        cursor_groups: &[UnsafeCursor<T, A>],
        args: &[Arg],
        update_fn: &mut U,
        notify: &mut F,
    ) where
        U: FnMut(&mut T, &Arg),
        F: FnMut(&T, *mut LeafNode<T, A>),
    {
        let mut cursor_map: HashMap<(NonNull<_>, usize), Vec<(&UnsafeCursor<T, A>, &Arg)>, _> =
            FxHashMap::default();
        for (i, arg) in args.iter().enumerate() {
            let cursor = &cursor_groups[i];
            cursor_map
                .entry((cursor.leaf, cursor.index))
                .or_default()
                .push((cursor, arg));
        }

        let mut updates_map: HashMap<NonNull<_>, Vec<(usize, SmallVec<[T; 2]>)>, _> =
            FxHashMap::default();
        for ((mut leaf, index), args) in cursor_map.iter() {
            // SAFETY: we has the exclusive reference to the tree and the cursor is valid
            let leaf = unsafe { leaf.as_mut() };
            let input_args = args.iter().map(|x| x.1).collect::<Vec<_>>();
            let updates = leaf.pure_updates_at_same_index(
                *index,
                &args.iter().map(|x| x.0.offset).collect::<Vec<_>>(),
                &args.iter().map(|x| x.0.len).collect::<Vec<_>>(),
                &input_args,
                update_fn,
            );

            if let Some(update) = updates {
                updates_map
                    .entry(leaf.into())
                    .or_default()
                    .push((*index, update.into_iter().collect()));
            }
        }

        self.update_with_gathered_map(updates_map, notify);
    }

    // TODO: perf, use smallvec
    fn update_with_gathered_map<F, M>(
        &mut self,
        iter: HashMap<NonNull<LeafNode<T, A>>, Vec<(usize, SmallVec<[T; 2]>)>, M>,
        notify: &mut F,
    ) where
        F: FnMut(&T, *mut LeafNode<T, A>),
    {
        let mut internal_updates_map: HashMap<NonNull<_>, Vec<(usize, Vec<_>)>, _> =
            FxHashMap::default();
        for (mut leaf, updates) in iter {
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

    pub fn debug_check(&mut self) {
        self.with_node_mut(|node| {
            node.as_internal_mut().unwrap().check();
        })
    }

    pub fn debug_inspect(&mut self) {
        println!(
            "RleTree: \n- len={:?}\n- InternalNodes={}\n- LeafNodes={}\n- Elements={}",
            self.len(),
            self.internal_node_num(),
            self.leaf_node_num(),
            self.elem_num()
        );
    }

    fn internal_node_num(&self) -> usize {
        self.with_node(|node| {
            let mut num = 0;
            node.recursive_visit_all(&mut |node| {
                if node.as_internal().is_some() {
                    num += 1;
                }
            });
            num
        })
    }

    fn leaf_node_num(&self) -> usize {
        self.with_node(|node| {
            let mut num = 0;
            node.recursive_visit_all(&mut |node| {
                if node.as_leaf().is_some() {
                    num += 1;
                }
            });
            num
        })
    }

    fn elem_num(&self) -> usize {
        self.with_node(|node| {
            let mut num = 0;
            node.recursive_visit_all(&mut |node| {
                if let Some(leaf) = node.as_leaf() {
                    num += leaf.children.len();
                }
            });
            num
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

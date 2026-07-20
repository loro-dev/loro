#![doc = include_str!("../README.md")]
#![forbid(unsafe_code)]

use core::{fmt::Debug, ops::Range};
use std::collections::{BTreeSet, VecDeque};
use std::ops::AddAssign;
use std::{cmp::Ordering, mem::take, ops::RangeBounds};

pub(crate) use heapless::Vec as HeaplessVec;
use itertools::Itertools;
use rle::{CanRemove, TryInsert};
use rustc_hash::{FxHashMap, FxHashSet};
use thunderdome::Arena;
use thunderdome::Index as RawArenaIndex;

pub use generic_impl::*;

use crate::rle::{HasLength, Mergeable, Sliceable};

mod generic_impl;
pub mod iter;

pub mod rle;

pub type HeapVec<T> = Vec<T>;

const MAX_CHILDREN_NUM: usize = 12;

/// `Elem` should has length. `offset` in search result should always >= `Elem.rle_len()`
pub trait BTreeTrait {
    /// Sometime an [Elem] with length of 0, but it's not empty.
    ///
    /// The empty [Elem]s are the ones that can be safely ignored.
    type Elem: Debug + HasLength + Sliceable + Mergeable + TryInsert + CanRemove;
    type Cache: Debug + Default + Clone + Eq;
    type CacheDiff: Debug + Default + CanRemove;
    // Whether we should use cache diff by default
    const USE_DIFF: bool = true;

    /// If diff.is_some, return value should be some too
    fn calc_cache_internal(cache: &mut Self::Cache, caches: &[Child<Self>]) -> Self::CacheDiff;
    fn apply_cache_diff(cache: &mut Self::Cache, diff: &Self::CacheDiff);
    fn merge_cache_diff(diff1: &mut Self::CacheDiff, diff2: &Self::CacheDiff);
    fn get_elem_cache(elem: &Self::Elem) -> Self::Cache;
    fn new_cache_to_diff(cache: &Self::Cache) -> Self::CacheDiff;
    fn sub_cache(cache_lhs: &Self::Cache, cache_rhs: &Self::Cache) -> Self::CacheDiff;
}

pub trait Query<B: BTreeTrait> {
    type QueryArg: Clone;

    fn init(target: &Self::QueryArg) -> Self;

    fn find_node(&mut self, target: &Self::QueryArg, child_caches: &[Child<B>]) -> FindResult;

    /// Confirm the search result and returns (offset, found)
    ///
    /// If elem is not target, `found=false`
    fn confirm_elem(&mut self, q: &Self::QueryArg, elem: &B::Elem) -> (usize, bool);
}

pub struct BTree<B: BTreeTrait> {
    /// internal nodes
    in_nodes: Arena<Node<B>>,
    /// leaf nodes
    leaf_nodes: Arena<LeafNode<B::Elem>>,
    /// root is always a internal node
    /// TODO: we may use a constant as root index
    root: ArenaIndex,
    root_cache: B::Cache,
}

impl<Elem: Clone, B: BTreeTrait<Elem = Elem>> Clone for BTree<B> {
    fn clone(&self) -> Self {
        Self {
            in_nodes: self.in_nodes.clone(),
            leaf_nodes: self.leaf_nodes.clone(),
            root: self.root,
            root_cache: self.root_cache.clone(),
        }
    }
}

pub struct FindResult {
    pub index: usize,
    pub offset: usize,
    pub found: bool,
}

impl FindResult {
    pub fn new_found(index: usize, offset: usize) -> Self {
        Self {
            index,
            offset,
            found: true,
        }
    }

    pub fn new_missing(index: usize, offset: usize) -> Self {
        Self {
            index,
            offset,
            found: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Idx {
    pub arena: ArenaIndex,
    pub arr: u8,
}

impl Idx {
    pub fn new(arena: ArenaIndex, arr: u8) -> Self {
        Self { arena, arr }
    }
}

type NodePath = HeaplessVec<Idx, 10>;

#[derive(Debug, Clone, PartialEq, Eq, Copy, Hash)]
pub struct Cursor {
    pub leaf: LeafIndex,
    pub offset: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Copy)]
pub struct QueryResult {
    pub cursor: Cursor,
    pub found: bool,
}

/// Exposed arena index
///
/// Only exposed arena index of leaf node.
///
///
#[repr(transparent)]
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, PartialOrd, Ord)]
pub struct LeafIndex(RawArenaIndex);

impl LeafIndex {
    pub fn inner(&self) -> RawArenaIndex {
        self.0
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub enum ArenaIndex {
    Leaf(RawArenaIndex),
    Internal(RawArenaIndex),
}

impl ArenaIndex {
    fn unwrap(self) -> RawArenaIndex {
        match self {
            ArenaIndex::Leaf(x) => x,
            ArenaIndex::Internal(x) => x,
        }
    }

    pub fn unwrap_leaf(self) -> RawArenaIndex {
        match self {
            ArenaIndex::Leaf(x) => x,
            ArenaIndex::Internal(_) => panic!("unwrap_leaf on internal node"),
        }
    }

    pub fn unwrap_internal(self) -> RawArenaIndex {
        match self {
            ArenaIndex::Leaf(_) => panic!("unwrap_internal on leaf node"),
            ArenaIndex::Internal(x) => x,
        }
    }
}

impl From<LeafIndex> for ArenaIndex {
    fn from(value: LeafIndex) -> Self {
        Self::Leaf(value.0)
    }
}

impl From<RawArenaIndex> for LeafIndex {
    fn from(value: RawArenaIndex) -> Self {
        Self(value)
    }
}

/// A slice of element
///
/// - `start` is Some(start_offset) when it's first element of the given range.
/// - `end` is Some(end_offset) when it's last element of the given range.
#[derive(Debug)]
pub struct ElemSlice<'a, Elem> {
    cursor: Cursor,
    pub elem: &'a Elem,
    pub start: Option<usize>,
    pub end: Option<usize>,
}

impl<'a, Elem> ElemSlice<'a, Elem> {
    pub fn cursor(&self) -> &Cursor {
        &self.cursor
    }
}

impl QueryResult {
    pub fn elem<'b, Elem: Debug, B: BTreeTrait<Elem = Elem>>(
        &self,
        tree: &'b BTree<B>,
    ) -> Option<&'b Elem> {
        tree.leaf_nodes.get(self.cursor().leaf.0).map(|x| &x.elem)
    }

    #[inline(always)]
    pub fn cursor(&self) -> Cursor {
        self.cursor
    }

    #[inline(always)]
    pub fn leaf(&self) -> LeafIndex {
        self.cursor().leaf
    }

    #[inline(always)]
    pub fn offset(&self) -> usize {
        self.cursor().offset
    }

    #[inline(always)]
    pub fn found(&self) -> bool {
        self.found
    }

    #[inline(always)]
    pub fn arena(&self) -> RawArenaIndex {
        self.cursor.leaf.0
    }
}

#[derive(Debug, Clone)]
pub struct LeafNode<Elem> {
    elem: Elem,
    parent: RawArenaIndex,
}

impl<T> LeafNode<T> {
    pub fn parent(&self) -> ArenaIndex {
        ArenaIndex::Internal(self.parent)
    }

    pub fn elem(&self) -> &T {
        &self.elem
    }
}

impl<T: Sliceable> LeafNode<T> {
    fn split(&mut self, offset: usize) -> Self {
        let new_elem = self.elem.split(offset);
        Self {
            elem: new_elem,
            parent: self.parent,
        }
    }
}

pub struct Node<B: BTreeTrait> {
    parent: Option<ArenaIndex>,
    parent_slot: u8,
    children: HeaplessVec<Child<B>, MAX_CHILDREN_NUM>,
}

#[repr(transparent)]
#[derive(Debug, Default, Clone)]
pub struct SplittedLeaves {
    pub arr: HeaplessVec<LeafIndex, 2>,
}

impl SplittedLeaves {
    #[inline]
    fn push_option(&mut self, leaf: Option<ArenaIndex>) {
        if let Some(leaf) = leaf {
            self.arr.push(leaf.unwrap().into()).unwrap();
        }
    }

    #[inline]
    fn push(&mut self, leaf: ArenaIndex) {
        self.arr.push(leaf.unwrap().into()).unwrap();
    }
}

impl<Cache: Debug, Elem: Debug, B: BTreeTrait<Elem = Elem, Cache = Cache>> Debug for BTree<B> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        fn fmt_node<Cache: Debug, Elem: Debug, B: BTreeTrait<Elem = Elem>>(
            tree: &BTree<B>,
            node_idx: ArenaIndex,
            f: &mut core::fmt::Formatter<'_>,
            indent_size: usize,
        ) -> core::fmt::Result {
            match node_idx {
                ArenaIndex::Leaf(_) => {}
                ArenaIndex::Internal(_) => {
                    let node = tree.get_internal(node_idx);
                    for child in node.children.iter() {
                        indent(f, indent_size)?;
                        if child.is_internal() {
                            let child_node = tree.get_internal(child.arena);
                            f.write_fmt(format_args!(
                                "{} Arena({:?}) Cache: {:?}\n",
                                child_node.parent_slot, &child.arena, &child.cache
                            ))?;
                            fmt_node::<Cache, Elem, B>(tree, child.arena, f, indent_size + 1)?;
                        } else {
                            let node = tree.get_leaf(child.arena);
                            f.write_fmt(format_args!(
                                "Leaf({:?}) Arena({:?}) Parent({:?}) Cache: {:?}\n",
                                &node.elem, child.arena, node.parent, &child.cache
                            ))?;
                        }
                    }
                }
            }

            Ok(())
        }

        fn indent(f: &mut core::fmt::Formatter<'_>, indent: usize) -> core::fmt::Result {
            for _ in 0..indent {
                f.write_str("    ")?;
            }
            Ok(())
        }

        f.write_str("BTree\n")?;
        indent(f, 1)?;
        f.write_fmt(format_args!(
            "Root Arena({:?}) Cache: {:?}\n",
            &self.root, &self.root_cache
        ))?;
        fmt_node::<Cache, Elem, B>(self, self.root, f, 1)
    }
}

impl<Cache: Debug, Elem: Debug, B: BTreeTrait<Elem = Elem, Cache = Cache>> Debug for Node<B> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Node")
            .field("children", &self.children)
            .finish()
    }
}

impl<Cache: Debug, Elem: Debug, B: BTreeTrait<Elem = Elem, Cache = Cache>> Debug for Child<B> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Child")
            .field("index", &self.arena)
            .field("cache", &self.cache)
            .finish()
    }
}

impl<Elem: Clone, B: BTreeTrait<Elem = Elem>> Clone for Node<B> {
    fn clone(&self) -> Self {
        Self {
            parent: self.parent,
            parent_slot: self.parent_slot,
            children: self.children.clone(),
        }
    }
}

pub struct Child<B: ?Sized + BTreeTrait> {
    pub arena: ArenaIndex,
    pub cache: B::Cache,
}

impl<B: ?Sized + BTreeTrait> Child<B> {
    #[inline]
    fn is_internal(&self) -> bool {
        matches!(self.arena, ArenaIndex::Internal(_))
    }

    #[inline]
    #[allow(unused)]
    fn is_leaf(&self) -> bool {
        matches!(self.arena, ArenaIndex::Leaf(_))
    }
}

impl<B: BTreeTrait> Clone for Child<B> {
    fn clone(&self) -> Self {
        Self {
            arena: self.arena,
            cache: self.cache.clone(),
        }
    }
}

impl<B: BTreeTrait> Child<B> {
    pub fn cache(&self) -> &B::Cache {
        &self.cache
    }

    fn new(arena: ArenaIndex, cache: B::Cache) -> Self {
        Self { arena, cache }
    }
}

impl<B: BTreeTrait> Node<B> {
    #[inline(always)]
    pub fn new() -> Self {
        Self {
            parent: None,
            parent_slot: u8::MAX,
            children: HeaplessVec::new(),
        }
    }

    #[inline(always)]
    pub fn is_full(&self) -> bool {
        self.children.len() >= MAX_CHILDREN_NUM
    }

    #[inline(always)]
    pub fn is_lack(&self) -> bool {
        self.children.len() < MAX_CHILDREN_NUM / 2
    }

    #[inline(always)]
    pub fn len(&self) -> usize {
        self.children.len()
    }

    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    #[inline(always)]
    pub fn is_child_leaf(&self) -> bool {
        if self.children.is_empty() {
            return true;
        }

        self.children[0].is_leaf()
    }

    /// if diff is not provided, the cache will be calculated from scratch
    #[inline(always)]
    fn calc_cache(&self, cache: &mut B::Cache, diff: Option<B::CacheDiff>) -> B::CacheDiff {
        match diff {
            Some(inner) => {
                B::apply_cache_diff(cache, &inner);
                inner
            }
            None => B::calc_cache_internal(cache, &self.children),
        }
    }
}

impl<B: BTreeTrait> Default for Node<B> {
    fn default() -> Self {
        Self::new()
    }
}

type LeafDirtyMap<Diff> = FxHashMap<ArenaIndex, Diff>;

/// Whether the parent node is lack of children
#[repr(transparent)]
struct LackInfo {
    /// if Some, the parent node is lack
    parent_lack: Option<ArenaIndex>,
}

impl<B: BTreeTrait> BTree<B> {
    pub fn new() -> Self {
        let mut arena = Arena::new();
        let root = arena.insert(Node::new());
        Self {
            in_nodes: arena,
            leaf_nodes: Arena::new(),
            root: ArenaIndex::Internal(root),
            root_cache: B::Cache::default(),
        }
    }

    /// Get the number of nodes in the tree.
    /// It includes all the internal nodes and the leaf nodes.
    #[inline(always)]
    pub fn node_len(&self) -> usize {
        self.in_nodes.len() + self.leaf_nodes.len()
    }

    /// Insert new element to the tree.
    ///
    /// Returns (insert_pos, splitted_leaves)
    pub fn insert<Q>(&mut self, q: &Q::QueryArg, data: B::Elem) -> (Cursor, SplittedLeaves)
    where
        Q: Query<B>,
    {
        let Some(result) = self.query::<Q>(q) else {
            return (self.push(data), Default::default());
        };

        self.insert_by_path(result.cursor, data)
    }

    pub fn insert_by_path(&mut self, cursor: Cursor, data: B::Elem) -> (Cursor, SplittedLeaves) {
        let index = cursor.leaf;
        let leaf = self.leaf_nodes.get_mut(index.0).unwrap();
        let mut parent_idx = leaf.parent();
        let cache_diff = if B::USE_DIFF {
            Some(B::new_cache_to_diff(&B::get_elem_cache(&data)))
        } else {
            None
        };

        let mut is_full = false;
        let mut splitted: SplittedLeaves = Default::default();
        let ans = match leaf.elem.try_insert(cursor.offset, data) {
            Ok(_) => cursor,
            Err(data) => {
                // Try to merge
                if cursor.offset == 0 && data.can_merge(&leaf.elem) {
                    leaf.elem.merge_left(&data);
                    Cursor {
                        leaf: index,
                        offset: 0,
                    }
                } else if cursor.offset == leaf.elem.rle_len() && leaf.elem.can_merge(&data) {
                    let offset = leaf.elem.rle_len();
                    leaf.elem.merge_right(&data);
                    Cursor {
                        leaf: index,
                        offset,
                    }
                } else {
                    // Insert new leaf node
                    let SplitInfo {
                        parent_idx: parent_index,
                        insert_slot: insert_index,
                        new_leaf,
                        ..
                    } = self.split_leaf_if_needed(cursor);
                    parent_idx = ArenaIndex::Internal(parent_index);
                    let child = self.alloc_leaf_child(data, parent_index);
                    let ans = child.arena;
                    splitted.push_option(new_leaf);
                    let parent = self.in_nodes.get_mut(parent_index).unwrap();
                    parent.children.insert(insert_index, child).unwrap();
                    is_full = parent.is_full();
                    Cursor {
                        leaf: ans.unwrap().into(),
                        offset: 0,
                    }
                }
            }
        };

        self.recursive_update_cache(cursor.leaf.into(), B::USE_DIFF, cache_diff);
        if is_full {
            self.split(parent_idx);
        }

        (ans, splitted)
    }

    fn alloc_leaf_child(
        &mut self,
        data: <B as BTreeTrait>::Elem,
        parent_index: RawArenaIndex,
    ) -> Child<B> {
        let elem_cache = B::get_elem_cache(&data);
        let new_leaf_index = self.alloc_new_leaf(LeafNode {
            elem: data,
            parent: parent_index,
        });
        Child {
            arena: new_leaf_index,
            cache: elem_cache,
        }
    }

    /// Split a leaf node at offset if it's not the start/end of the leaf node.
    ///
    /// This method should be called when inserting at target pos.
    fn split_leaf_if_needed(&mut self, pos: Cursor) -> SplitInfo {
        let leaf = self.leaf_nodes.get_mut(pos.leaf.0).unwrap();
        let parent_idx = leaf.parent;
        let parent = self.in_nodes.get_mut(leaf.parent).unwrap();
        let mut new_pos = Some(pos);
        let mut rt_new_leaf = None;
        let leaf_slot = parent
            .children
            .iter()
            .position(|x| x.arena.unwrap() == pos.leaf.0)
            .unwrap();
        let left_neighbour = if leaf_slot == 0 {
            None
        } else {
            Some(parent.children[leaf_slot - 1].arena.unwrap().into())
        };
        let insert_pos = if pos.offset == 0 {
            leaf_slot
        } else if pos.offset == leaf.elem.rle_len() {
            if leaf_slot + 1 < parent.children.len() {
                new_pos = Some(Cursor {
                    leaf: parent.children[leaf_slot + 1].arena.unwrap().into(),
                    offset: 0,
                });
            } else {
                new_pos = self.next_elem(pos);
            }
            leaf_slot + 1
        } else {
            assert!(
                pos.offset < leaf.elem.rle_len(),
                "elem.rle_len={} but pos.offset={} Elem:{:?}",
                leaf.elem.rle_len(),
                pos.offset,
                &leaf.elem
            );

            if parent.children.len() + 1 >= MAX_CHILDREN_NUM {
                self.split(ArenaIndex::Internal(parent_idx));
                // parent may be changed because of splitting
                return self.split_leaf_if_needed(pos);
            }

            let new_leaf = leaf.split(pos.offset);
            let left_cache = B::get_elem_cache(&leaf.elem);
            let cache = B::get_elem_cache(&new_leaf.elem);
            // alloc new leaf node
            let leaf_arena_index = {
                let arena_index = self.leaf_nodes.insert(new_leaf);
                ArenaIndex::Leaf(arena_index)
            };
            rt_new_leaf = Some(leaf_arena_index);
            new_pos = Some(Cursor {
                leaf: leaf_arena_index.unwrap().into(),
                offset: 0,
            });
            parent.children[leaf_slot].cache = left_cache;
            parent
                .children
                .insert(
                    leaf_slot + 1,
                    Child {
                        arena: leaf_arena_index,
                        cache,
                    },
                )
                .unwrap();

            leaf_slot + 1
        };

        SplitInfo {
            left_neighbour,
            new_pos,
            parent_idx,
            insert_slot: insert_pos,
            new_leaf: rt_new_leaf,
        }
    }

    fn alloc_new_leaf(&mut self, leaf: LeafNode<B::Elem>) -> ArenaIndex {
        let arena_index = self.leaf_nodes.insert(leaf);
        ArenaIndex::Leaf(arena_index)
    }

    /// Insert many elements into the tree at once
    ///
    /// It will invoke [`BTreeTrait::insert_batch`]
    pub fn insert_many_by_cursor(
        &mut self,
        cursor: Option<Cursor>,
        mut data_iter: impl Iterator<Item = B::Elem>,
    ) {
        let Some(first) = data_iter.next() else {
            return;
        };

        let Some(second) = data_iter.next() else {
            if let Some(c) = cursor {
                self.insert_by_path(c, first);
                return;
            } else {
                self.push(first);
                return;
            }
        };

        let mut data = Vec::with_capacity(data_iter.size_hint().0 + 2);
        data.push(first);
        data.push(second);
        for elem in data_iter {
            data.push(elem);
        }

        merge_adj(&mut data);
        if data.len() == 1 {
            if let Some(c) = cursor {
                self.insert_by_path(c, data.pop().unwrap());
                return;
            } else {
                self.push(data.pop().unwrap());
                return;
            }
        }

        if cursor.is_none() && self.is_empty() {
            assert!(self.is_empty());
            let (new_root, _) = self.create_subtrees_from_elem(data);
            self.in_nodes.remove(self.root.unwrap()).unwrap();
            self.root = new_root;
            return;
        }

        // dbg!(cursor, &data);
        // dbg!(&self);
        let cursor = cursor.expect("Cursor must be provided when tree is not empty");
        let SplitInfo {
            new_pos,
            left_neighbour,
            ..
        } = self.split_leaf_if_needed(cursor);
        let mut inserted = 0;
        if let Some(left) = left_neighbour {
            let left_node = self.leaf_nodes.get_mut(left.0).unwrap();
            let mut i = 0;
            while i < data.len() && left_node.elem.can_merge(&data[i]) {
                left_node.elem.merge_right(&data[i]);
                i += 1;
            }

            self.recursive_update_cache(left.into(), B::USE_DIFF, None);
            inserted = i;
        }

        let mut pos = new_pos.unwrap_or(cursor);
        // TODO: PERF this can be optimized further
        for item in data.drain(inserted..).rev() {
            let (p, _) = self.insert_by_path(pos, item);
            pos = p
        }
    }

    /// The returned height starts from 0. Leaf level is 0.
    ///
    /// Returns (newly created subtree's root, height)
    fn create_subtrees_from_elem(&mut self, data: Vec<B::Elem>) -> (ArenaIndex, usize) {
        let mut height = 0;
        let mut nodes = Vec::with_capacity(data.len() / MAX_CHILDREN_NUM + 1);
        for elem in data.into_iter().chunks(MAX_CHILDREN_NUM).into_iter() {
            let parent_index = self.in_nodes.insert(Node {
                parent: None,
                parent_slot: 0,
                children: Default::default(),
            });

            nodes.push(parent_index);
            let parent = self.in_nodes.get_mut(parent_index).unwrap();
            for (i, elem) in elem.enumerate() {
                let leaf = {
                    // alloc new leaf child
                    let elem_cache = B::get_elem_cache(&elem);
                    let new_leaf_index = {
                        let leaf = LeafNode {
                            elem,
                            parent: parent_index,
                        };
                        let arena_index = self.leaf_nodes.insert(leaf);
                        ArenaIndex::Leaf(arena_index)
                    };
                    Child {
                        arena: new_leaf_index,
                        cache: elem_cache,
                    }
                };
                parent.children[i] = leaf;
            }
        }

        while nodes.len() > 1 {
            let mut new_nodes = Vec::with_capacity(nodes.len() / MAX_CHILDREN_NUM + 1);
            for chunk in nodes.into_iter().chunks(MAX_CHILDREN_NUM).into_iter() {
                let parent_index = self.in_nodes.insert(Node {
                    parent: None,
                    parent_slot: 0,
                    children: Default::default(),
                });

                new_nodes.push(parent_index);
                for (i, child_idx) in chunk.enumerate() {
                    let (parent, child) = self.in_nodes.get2_mut(parent_index, child_idx);
                    let parent = parent.unwrap();
                    let child = child.unwrap();
                    let mut cache = B::Cache::default();
                    B::calc_cache_internal(&mut cache, &child.children);
                    parent.children[i] = Child {
                        arena: ArenaIndex::Internal(child_idx),
                        cache,
                    };
                    child.parent = Some(ArenaIndex::Internal(parent_index));
                    child.parent_slot = i as u8;
                }
            }
            nodes = new_nodes;
            height += 1;
        }

        (ArenaIndex::Internal(nodes[0]), height)
    }

    /// Shift by offset 1.
    ///
    /// It will not stay on empty spans but scan forward
    pub fn shift_path_by_one_offset(&self, mut path: Cursor) -> Option<Cursor>
    where
        B::Elem: rle::HasLength,
    {
        let leaf = self.leaf_nodes.get(path.leaf.0).unwrap();
        if path.offset + 1 < leaf.elem.rle_len() {
            path.offset += 1;
            return Some(path);
        }

        let mut parent_idx = leaf.parent;
        let mut parent = self.in_nodes.get(leaf.parent).unwrap();
        let mut elem_slot_index = Self::get_leaf_slot(path.leaf.0, parent);
        path.offset += 1;
        loop {
            if elem_slot_index == parent.children.len() {
                if let Some(next) = self.next_same_level_in_node(ArenaIndex::Internal(parent_idx)) {
                    elem_slot_index = 0;
                    path.offset = 0;
                    parent_idx = next.unwrap_internal();
                    parent = self.in_nodes.get(parent_idx).unwrap();
                } else {
                    return None;
                }
            }

            let elem = &parent.children[elem_slot_index];
            let leaf = self.leaf_nodes.get(elem.arena.unwrap()).unwrap();
            // skip empty span
            if leaf.elem.rle_len() <= path.offset {
                path.offset -= leaf.elem.rle_len();
                elem_slot_index += 1;
            } else {
                path.leaf = elem.arena.unwrap_leaf().into();
                break;
            }
        }

        Some(path)
    }

    fn get_leaf_slot(leaf_arena_index: RawArenaIndex, parent: &Node<B>) -> usize {
        parent
            .children
            .iter()
            .position(|x| x.arena.unwrap_leaf() == leaf_arena_index)
            .unwrap()
    }

    /// Query the tree by custom query type
    ///
    /// Return None if the tree is empty
    pub fn query<Q>(&self, query: &Q::QueryArg) -> Option<QueryResult>
    where
        Q: Query<B>,
    {
        self.query_with_finder_return::<Q>(query).0
    }

    pub fn query_with_finder_return<Q>(&self, query: &Q::QueryArg) -> (Option<QueryResult>, Q)
    where
        Q: Query<B>,
    {
        let mut finder = Q::init(query);
        if self.is_empty() {
            return (None, finder);
        }

        let mut node = self.in_nodes.get(self.root.unwrap()).unwrap();
        let mut index;
        let mut found = true;
        loop {
            let result = finder.find_node(query, &node.children);
            debug_assert!(!node.children.is_empty());
            let i = result.index;
            found = found && result.found;
            index = node.children[i].arena;
            match index {
                ArenaIndex::Internal(index) => {
                    node = self.in_nodes.get(index).unwrap();
                }
                ArenaIndex::Leaf(_) => {
                    let (offset, leaf_found) = finder.confirm_elem(
                        query,
                        &self.leaf_nodes.get(index.unwrap_leaf()).unwrap().elem,
                    );
                    return (
                        Some(QueryResult {
                            cursor: Cursor {
                                leaf: index.unwrap_leaf().into(),
                                offset,
                            },
                            found: found && leaf_found,
                        }),
                        finder,
                    );
                }
            }
        }
    }

    pub fn get_elem_mut(&mut self, leaf: LeafIndex) -> Option<&mut B::Elem> {
        let node = self.leaf_nodes.get_mut(leaf.0)?;
        Some(&mut node.elem)
    }

    pub fn get_elem(&self, leaf: LeafIndex) -> Option<&<B as BTreeTrait>::Elem> {
        self.leaf_nodes.get(leaf.0).map(|x| &x.elem)
    }

    /// Remove leaf node from the tree
    ///
    /// If it's already removed, this method will return None
    pub fn remove_leaf(&mut self, path: Cursor) -> Option<B::Elem> {
        let leaf = self.leaf_nodes.get_mut(path.leaf.0)?;
        let parent_idx = leaf.parent();
        let parent = self.in_nodes.get_mut(leaf.parent).unwrap();
        let index = Self::get_leaf_slot(path.leaf.0, parent);
        let child = parent.children.remove(index);
        let is_lack = parent.is_lack();
        let is_empty = parent.is_empty();
        debug_assert_eq!(child.arena.unwrap(), path.leaf.0);
        let elem = self.leaf_nodes.remove(child.arena.unwrap()).unwrap().elem;

        self.recursive_update_cache(parent_idx, B::USE_DIFF, None);
        if is_empty {
            self.remove_internal_node(parent_idx.unwrap());
        } else if is_lack {
            self.handle_lack_recursively(parent_idx);
        }

        Some(elem)
    }

    fn remove_internal_node(&mut self, node: RawArenaIndex) {
        if node == self.root.unwrap() {
            return;
        }

        let node = self.in_nodes.remove(node).unwrap();
        if let Some(parent_idx) = node.parent {
            let parent = self.in_nodes.get_mut(parent_idx.unwrap_internal()).unwrap();
            parent.children.remove(node.parent_slot as usize);
            let is_lack = parent.is_lack();
            let is_empty = parent.is_empty();
            self.update_children_parent_slot_from(parent_idx, node.parent_slot as usize);
            if is_empty {
                self.remove_internal_node(parent_idx.unwrap_internal());
            } else if is_lack {
                self.handle_lack_recursively(parent_idx);
            }
        } else {
            // ignore remove root
            unreachable!()
        }
    }

    /// Update the elements in place.
    ///
    /// If the range.start or range.end is in the middle of a leaf node, the leaf node
    /// will be splitted into two leaf nodes. The new leaf nodes will be returned.
    ///
    /// F should returns `Some(cache_diff)` if cache needs to be updated. Otherwise, returns None.
    ///
    /// If the given range has zero length, f will still be called, and the slice will
    /// have same `start` and `end` field
    ///
    /// TODO: need better test coverage
    pub fn update<F>(&mut self, range: Range<Cursor>, f: &mut F) -> SplittedLeaves
    where
        F: FnMut(&mut B::Elem) -> Option<B::CacheDiff>,
    {
        let mut splitted = SplittedLeaves::default();
        let start = range.start;
        let SplitInfo {
            new_pos: end,
            new_leaf,
            ..
        } = self.split_leaf_if_needed(range.end);
        splitted.push_option(new_leaf);
        let SplitInfo {
            new_pos: start,
            new_leaf,
            ..
        } = self.split_leaf_if_needed(start);
        splitted.push_option(new_leaf);
        let Some(start) = start else {
            return splitted;
        };
        let start_leaf = start.leaf;
        let mut path = self.get_path(start_leaf.into());
        let mut dirty_map: LeafDirtyMap<B::CacheDiff> = FxHashMap::default();
        let mut to_remove = Vec::default();

        loop {
            let current_leaf = path.last().unwrap();
            if let Some(end) = end {
                if current_leaf.arena.unwrap_leaf() == end.leaf.0 {
                    break;
                }
            }

            let node = self
                .leaf_nodes
                .get_mut(current_leaf.arena.unwrap_leaf())
                .unwrap();
            let cache_diff = f(&mut node.elem);
            if node.elem.can_remove() {
                to_remove.push(current_leaf.arena);
            }

            if let Some(diff) = cache_diff {
                add_leaf_dirty_map(current_leaf.arena, &mut dirty_map, diff);
            }

            if !self.next_sibling(&mut path) {
                break;
            }
        }

        if !dirty_map.is_empty() {
            self.update_dirty_cache_map(dirty_map);
        } else {
            self.in_nodes
                .get(self.root.unwrap_internal())
                .unwrap()
                .calc_cache(&mut self.root_cache, None);
        }

        for leaf in to_remove {
            self.remove_leaf(Cursor {
                leaf: leaf.unwrap().into(),
                offset: 0,
            });
        }
        splitted
    }

    /// Prefer begin of the next leaf node than end of the current leaf node
    ///
    /// When path.offset == leaf.rle_len(), this method will return
    /// the next leaf node with offset 0
    #[allow(unused)]
    pub fn prefer_right(&self, path: Cursor) -> Option<Cursor> {
        if path.offset == 0 {
            return Some(path);
        }

        let leaf = self.leaf_nodes.get(path.leaf.0).unwrap();
        if path.offset == leaf.elem.rle_len() {
            self.next_elem(path)
        } else {
            Some(path)
        }
    }

    /// Prefer end of the previous leaf node than begin of the current leaf node
    ///
    /// When path.offset == 0, this method will return
    /// the previous leaf node with offset leaf.rle_len()
    #[allow(unused)]
    pub fn prefer_left(&self, path: Cursor) -> Option<Cursor> {
        if path.offset != 0 {
            return Some(path);
        }

        let elem = self.prev_elem(path);
        if let Some(elem) = elem {
            let leaf = self.leaf_nodes.get(elem.leaf.0).unwrap();
            Some(Cursor {
                leaf: elem.leaf,
                offset: leaf.elem.rle_len(),
            })
        } else {
            None
        }
    }

    /// Update leaf node's elements.
    ///
    /// `f` returns Option<(cache_diff, new_insert_1, new_insert2)>
    ///
    /// - If returned value is `None`, the cache will not be updated.
    /// - If leaf_node.can_remove(), it will be removed from the tree.
    ///
    /// Returns (path, splitted_leaves), if is is still valid after this method. (If the leaf node is removed, the path will be None)
    pub fn update_leaf_by_search<Q: Query<B>>(
        &mut self,
        q: &Q::QueryArg,
        f: impl FnOnce(
            &mut B::Elem,
            QueryResult,
        ) -> Option<(B::CacheDiff, Option<B::Elem>, Option<B::Elem>)>,
    ) -> (Option<Cursor>, SplittedLeaves) {
        if self.is_empty() {
            panic!("update_leaf_by_search called on empty tree");
        }

        let mut splitted = SplittedLeaves::default();
        let mut finder = Q::init(q);
        let mut path = NodePath::default();
        let mut node_idx = self.root;
        let mut child_arr_pos = 0;
        while let ArenaIndex::Internal(node_idx_inner) = node_idx {
            path.push(Idx {
                arena: ArenaIndex::Internal(node_idx_inner),
                arr: child_arr_pos,
            })
            .unwrap();
            let node = self.in_nodes.get(node_idx_inner).unwrap();
            let result = finder.find_node(q, &node.children);
            child_arr_pos = result.index as u8;
            node_idx = node.children[result.index].arena;
        }

        let leaf = self.get_leaf_mut(node_idx);
        let (offset, found) = finder.confirm_elem(q, &leaf.elem);
        let ans = QueryResult {
            cursor: Cursor {
                leaf: node_idx.unwrap_leaf().into(),
                offset,
            },
            found,
        };
        let Some((diff, new_insert_1, new_insert_2)) = f(&mut leaf.elem, ans) else {
            return (Some(ans.cursor), splitted);
        };

        if new_insert_2.is_some() {
            unimplemented!()
        }

        // Delete
        if leaf.elem.can_remove() {
            // handle deletion
            // leaf node should be deleted
            assert!(new_insert_1.is_none());
            assert!(new_insert_2.is_none());
            self.leaf_nodes.remove(node_idx.unwrap()).unwrap();
            let mut is_first = true;
            let mut is_child_lack = false;
            let mut child_idx = node_idx;

            // iterate from leaf to root, child to parent
            while let Some(Idx {
                arena: parent_idx,
                arr: parent_arr_pos,
            }) = path.pop()
            {
                let parent = self.get_internal_mut(parent_idx);
                if is_first {
                    parent.children.remove(child_arr_pos as usize);
                    is_first = false;
                } else {
                    B::apply_cache_diff(&mut parent.children[child_arr_pos as usize].cache, &diff);
                }

                let is_lack = parent.is_lack();

                if is_child_lack {
                    self.handle_lack_single_layer(child_idx);
                }

                is_child_lack = is_lack;
                child_idx = parent_idx;
                child_arr_pos = parent_arr_pos;
            }

            B::apply_cache_diff(&mut self.root_cache, &diff);

            if is_child_lack {
                let root = self.get_internal_mut(self.root);
                if root.children.len() == 1 && !root.is_child_leaf() {
                    self.try_reduce_levels();
                }
            }

            return (None, splitted);
        }

        let mut new_cache_and_child = None;
        if let Some(new_insert_1) = new_insert_1 {
            let cache = B::get_elem_cache(&leaf.elem);
            let child = self.alloc_leaf_child(new_insert_1, path.last().unwrap().arena.unwrap());
            splitted.push(child.arena);
            new_cache_and_child = Some((cache, child));
        }

        while let Some(Idx {
            arena: parent_idx,
            arr: parent_arr_pos,
        }) = path.pop()
        {
            let parent = self.get_internal_mut(parent_idx);
            match take(&mut new_cache_and_child) {
                Some((cache, child)) => {
                    parent.children[child_arr_pos as usize].cache = cache;
                    parent
                        .children
                        .insert(child_arr_pos as usize + 1, child)
                        .unwrap();
                    let is_full = parent.is_full();
                    if !parent.is_child_leaf() {
                        self.update_children_parent_slot_from(
                            parent_idx,
                            child_arr_pos as usize + 1,
                        );
                    }
                    if is_full {
                        let (_, _, this_cache, right_child) = self.split_node(parent_idx, None);
                        new_cache_and_child = Some((this_cache, right_child));
                    }
                }
                None => {
                    B::apply_cache_diff(&mut parent.children[child_arr_pos as usize].cache, &diff);
                }
            }

            child_arr_pos = parent_arr_pos;
        }

        if let Some((cache, child)) = new_cache_and_child {
            self.split_root(cache, child);
        } else {
            B::apply_cache_diff(&mut self.root_cache, &diff);
        }

        (Some(ans.cursor), splitted)
    }

    /// Update leaf node's elements, return true if cache need to be updated
    ///
    /// `f` returns (is_cache_updated, cache_diff, new_insert_1, new_insert2)
    ///
    /// - If leaf_node.can_remove(), it will be removed from the tree.
    ///
    /// Returns true if the node_idx is still valid. (If the leaf node is removed, it will return false).
    pub fn update_leaf(
        &mut self,
        node_idx: LeafIndex,
        f: impl FnOnce(&mut B::Elem) -> (bool, Option<B::Elem>, Option<B::Elem>),
    ) -> (bool, SplittedLeaves) {
        let mut splitted = SplittedLeaves::default();
        let node = self.leaf_nodes.get_mut(node_idx.0).unwrap();
        let mut parent_idx = node.parent();
        let (need_update_cache, mut new_insert_1, mut new_insert_2) = f(&mut node.elem);
        {
            // Normalize returned values
            //
            // If the node can be removed, then both new_insert_1 & new_insert_2 should be None
            // The priority is node.elem > new_insert_1 > new_insert_2
            //
            // And new_insert_1 and new_insert_2 should not match `can_remove` condition
            if let Some(ref new_1) = new_insert_1 {
                if new_1.can_remove() {
                    new_insert_1 = new_insert_2.take();
                    if let Some(ref new_1) = new_insert_1 {
                        if new_1.can_remove() {
                            new_insert_1 = None;
                        }
                    }
                }
            }

            if let Some(ref new_2) = new_insert_2 {
                if new_2.can_remove() {
                    new_insert_2 = None;
                } else if new_insert_1.is_none() {
                    std::mem::swap(&mut new_insert_1, &mut new_insert_2);
                }
            }

            if node.elem.can_remove() {
                if let Some(new_1) = new_insert_1 {
                    node.elem = new_1;
                    new_insert_1 = new_insert_2.take();
                }
            }
        }

        let deleted = node.elem.can_remove();

        if need_update_cache {
            self.recursive_update_cache(node_idx.into(), B::USE_DIFF, None);
        }

        if deleted {
            debug_assert!(new_insert_1.is_none());
            debug_assert!(new_insert_2.is_none());
            self.leaf_nodes.remove(node_idx.0).unwrap();
            let parent = self.in_nodes.get_mut(parent_idx.unwrap()).unwrap();
            let slot = Self::get_leaf_slot(node_idx.0, parent);
            parent.children.remove(slot);
            let is_lack = parent.is_lack();
            if is_lack {
                self.handle_lack_recursively(parent_idx);
            }

            (false, splitted)
        } else if new_insert_1.is_none() {
            debug_assert!(new_insert_2.is_none());
            (true, splitted)
        } else {
            if let (Some(new), None) = (&new_insert_1, &new_insert_2) {
                // try merge new insert to next element
                let parent = self.in_nodes.get_mut(parent_idx.unwrap()).unwrap();
                let slot = Self::get_leaf_slot(node_idx.0, parent);
                if slot + 1 < parent.children.len() {
                    let next_idx = parent.children[slot + 1].arena.unwrap().into();
                    let next = self.get_elem_mut(next_idx).unwrap();
                    if new.can_merge(next) {
                        next.merge_left(new);
                        self.recursive_update_cache(next_idx.into(), B::USE_DIFF, None);
                        splitted.push(next_idx.into());
                        return (true, splitted);
                    }
                }
            }

            let count = if new_insert_1.is_some() { 1 } else { 0 }
                + if new_insert_2.is_some() { 1 } else { 0 };
            let parent = self.in_nodes.get_mut(parent_idx.unwrap()).unwrap();
            let parent = if parent.children.len() + count >= MAX_CHILDREN_NUM {
                self.split(parent_idx);
                let node = self.leaf_nodes.get(node_idx.0).unwrap();
                parent_idx = node.parent();
                self.in_nodes.get_mut(parent_idx.unwrap()).unwrap()
            } else {
                parent
            };

            let new: HeaplessVec<_, 2> = new_insert_1
                .into_iter()
                .chain(new_insert_2)
                .map(|elem| {
                    // Allocate new leaf node
                    let parent_index = parent_idx.unwrap();
                    let elem_cache = B::get_elem_cache(&elem);
                    let new_leaf_index = {
                        let leaf = LeafNode {
                            elem,
                            parent: parent_index,
                        };
                        let arena_index = self.leaf_nodes.insert(leaf);
                        ArenaIndex::Leaf(arena_index)
                    };
                    Child {
                        arena: new_leaf_index,
                        cache: elem_cache,
                    }
                })
                .collect();
            let slot = Self::get_leaf_slot(node_idx.0, parent);
            for (i, v) in new.into_iter().enumerate() {
                splitted.push(v.arena);
                parent.children.insert(slot + 1 + i, v).unwrap();
            }

            assert!(!parent.is_full());
            self.recursive_update_cache(parent_idx, B::USE_DIFF, None);
            (true, splitted)
        }
    }

    /// Update the given leaves with the given function in the given range.
    ///
    /// - The range descibes the range inside the leaf node.
    /// - There can be multiple ranges in the same leaf node.
    /// - The cahce will be recalculated for each affected node
    /// - It doesn't guarantee the applying order
    ///
    /// Currently, the time complexity is O(m^2) for each leaf node,
    /// where m is the number of ranges inside the same leaf node.
    /// If we have a really large m, this function need to be optimized.
    pub fn update_leaves_with_arg_in_ranges<A: Debug>(
        &mut self,
        mut args: Vec<(LeafIndex, Range<usize>, A)>,
        mut f: impl FnMut(&mut B::Elem, &A),
    ) -> Vec<LeafIndex> {
        args.sort_by_key(|x| x.0);
        let mut new_leaves = Vec::new();
        let mut dirty_map: LeafDirtyMap<B::CacheDiff> = Default::default();
        let mut new_elems_at_cursor: FxHashMap<Cursor, Vec<B::Elem>> = Default::default();
        for (leaf, group) in &args.into_iter().group_by(|x| x.0) {
            // This loop doesn't change the shape of the tree.  It only changes each leaf element.
            // A leaf element may be splitted into several parts. The first part stay in the tree,
            // while the rest of them are inserted into `new_elem_at_cursor`, which will be inserted
            // into the tree later.
            let leaf_node = self.leaf_nodes.get_mut(leaf.0).unwrap();
            let len = leaf_node.elem().rle_len();
            let mut split_at = BTreeSet::new();
            // PERF we can avoid this alloc and `group_by`
            let group: Vec<_> = group.into_iter().collect();
            for (_, range, _) in group.iter() {
                split_at.insert(range.start);
                split_at.insert(range.end);
            }

            split_at.remove(&0);
            split_at.remove(&len);

            // leaf_node.elem is the first elem
            let old_cache = B::get_elem_cache(&leaf_node.elem);
            if split_at.is_empty() {
                // doesn't need to split
                for (_, range, a) in group.iter() {
                    assert_eq!(range.start, 0);
                    assert_eq!(range.end, len);
                    f(&mut leaf_node.elem, a);
                }
            } else {
                let mut new_elems = Vec::new();

                let first_split = split_at.first().copied().unwrap();

                // handle first element
                let mut elem = leaf_node.elem.split(first_split);
                for (_, r, a) in group.iter() {
                    if r.start == 0 {
                        f(&mut leaf_node.elem, a);
                    }
                }

                // handle elements in the middle
                let mut last_index = first_split;
                for &index in split_at.iter().skip(1) {
                    let next_elem = elem.split(index - last_index);
                    let cur_range = last_index..index;
                    for (_, r, a) in group.iter() {
                        if r.start <= cur_range.start && cur_range.end <= r.end {
                            f(&mut elem, a);
                        }
                    }

                    new_elems.push(elem);
                    elem = next_elem;
                    last_index = index;
                }

                // handle the last element
                for (_, r, a) in group.iter() {
                    if r.end == len {
                        f(&mut elem, a);
                    }
                }

                new_elems.push(elem);
                new_elems_at_cursor.insert(
                    Cursor {
                        leaf,
                        offset: leaf_node.elem().rle_len(),
                    },
                    new_elems,
                );
            }

            let new_cache = B::get_elem_cache(&leaf_node.elem);
            let diff = B::sub_cache(&new_cache, &old_cache);
            dirty_map.insert(leaf.into(), diff);
        }

        // update cache
        self.update_dirty_cache_map(dirty_map);

        // PERF we can use batch insert to optimize this
        // insert the new leaf nodes
        for (mut cursor, elems) in new_elems_at_cursor {
            for elem in elems.into_iter() {
                // PERF can use insert many to optimize when it's supported
                let result = self.insert_by_path(cursor, elem);
                let len = self.get_elem(result.0.leaf).unwrap().rle_len();
                new_leaves.push(result.0.leaf);
                debug_assert_eq!(result.1.arr.len(), 0);
                cursor = Cursor {
                    leaf: result.0.leaf,
                    offset: len,
                };
            }
        }

        new_leaves
    }

    fn update_root_cache(&mut self) {
        self.in_nodes
            .get(self.root.unwrap_internal())
            .unwrap()
            .calc_cache(&mut self.root_cache, None);
    }

    fn update_dirty_cache_map(&mut self, mut diff_map: LeafDirtyMap<B::CacheDiff>) {
        // diff_map only contains leaf nodes when this function is called
        let mut visit_set: FxHashSet<ArenaIndex> = diff_map.keys().copied().collect();
        while !visit_set.is_empty() {
            for child_idx in take(&mut visit_set) {
                let (parent_idx, cache_diff) = match child_idx {
                    ArenaIndex::Leaf(leaf_idx) => {
                        let node = self.leaf_nodes.get(leaf_idx).unwrap();
                        let parent_idx = node.parent;
                        let parent = self.in_nodes.get_mut(parent_idx).unwrap();
                        let cache_diff = diff_map.remove(&child_idx).unwrap();
                        for child in parent.children.iter_mut() {
                            if child.arena == child_idx {
                                B::apply_cache_diff(&mut child.cache, &cache_diff);
                                break;
                            }
                        }

                        (ArenaIndex::Internal(parent_idx), cache_diff)
                    }
                    ArenaIndex::Internal(_) => {
                        let node = self.in_nodes.get(child_idx.unwrap_internal()).unwrap();
                        let Some(parent_idx) = node.parent else {
                            continue;
                        };
                        let (child, parent) = self.get2_mut(child_idx, parent_idx);
                        let cache_diff = child.calc_cache(
                            &mut parent.children[child.parent_slot as usize].cache,
                            diff_map.remove(&child_idx),
                        );

                        (parent_idx, cache_diff)
                    }
                };

                visit_set.insert(parent_idx);
                if let Some(e) = diff_map.get_mut(&parent_idx) {
                    B::merge_cache_diff(e, &cache_diff);
                } else {
                    diff_map.insert(parent_idx, cache_diff);
                }
            }
        }

        self.in_nodes
            .get(self.root.unwrap_internal())
            .unwrap()
            .calc_cache(&mut self.root_cache, None);
    }

    /// Removed deleted children. `deleted` means they are removed from the arena.
    fn filter_deleted_children(&mut self, internal_node: ArenaIndex) {
        let node = self
            .in_nodes
            .get_mut(internal_node.unwrap_internal())
            .unwrap();
        // PERF: I hate this pattern...
        let mut children = take(&mut node.children);
        children.retain(|x| match x.arena {
            ArenaIndex::Leaf(leaf) => self.leaf_nodes.contains(leaf),
            ArenaIndex::Internal(index) => self.in_nodes.contains(index),
        });
        let node = self
            .in_nodes
            .get_mut(internal_node.unwrap_internal())
            .unwrap();
        node.children = children;
    }

    pub fn iter(&self) -> impl Iterator<Item = &B::Elem> + '_ {
        let mut path = self.first_path().unwrap_or_default();
        path.pop();
        let idx = path.last().copied().unwrap_or(Idx::new(self.root, 0));
        debug_assert!(matches!(idx.arena, ArenaIndex::Internal(_)));
        let node = self.get_internal(idx.arena);
        let mut iter = node.children.iter();
        core::iter::from_fn(move || loop {
            if path.is_empty() {
                return None;
            }

            match iter.next() {
                None => {
                    if !self.next_sibling(&mut path) {
                        return None;
                    }

                    let idx = *path.last().unwrap();
                    debug_assert!(matches!(idx.arena, ArenaIndex::Internal(_)));
                    let node = self.get_internal(idx.arena);
                    iter = node.children.iter();
                }
                Some(elem) => {
                    let leaf = self.leaf_nodes.get(elem.arena.unwrap_leaf()).unwrap();
                    return Some(&leaf.elem);
                }
            }
        })
    }

    pub fn drain(&mut self, range: Range<QueryResult>) -> iter::Drain<'_, B> {
        iter::Drain::new(self, Some(range.start), Some(range.end))
    }

    pub fn drain_by_query<Q: Query<B>>(&mut self, range: Range<Q::QueryArg>) -> iter::Drain<'_, B> {
        let start = self.query::<Q>(&range.start);
        let end = self.query::<Q>(&range.end);
        iter::Drain::new(self, start, end)
    }

    fn first_path(&self) -> Option<NodePath> {
        let mut index = self.root;
        let mut node = self.in_nodes.get(index.unwrap_internal()).unwrap();
        if node.is_empty() {
            return None;
        }

        let mut path = NodePath::new();
        loop {
            path.push(Idx::new(index, 0)).unwrap();
            match index {
                ArenaIndex::Leaf(_) => {
                    break;
                }
                ArenaIndex::Internal(_) => {
                    index = node.children[0].arena;
                    if let ArenaIndex::Internal(i) = index {
                        node = self.in_nodes.get(i).unwrap();
                    };
                }
            }
        }

        Some(path)
    }

    fn last_path(&self) -> Option<NodePath> {
        let mut path = NodePath::new();
        let mut index = self.root;
        let mut node = self.in_nodes.get(index.unwrap_internal()).unwrap();
        let mut pos_in_parent = 0;
        if node.is_empty() {
            return None;
        }

        loop {
            path.push(Idx::new(index, pos_in_parent)).unwrap();
            match index {
                ArenaIndex::Leaf(_) => {
                    break;
                }
                ArenaIndex::Internal(_) => {
                    pos_in_parent = node.children.len() as u8 - 1;
                    index = node.children[node.children.len() - 1].arena;
                    if let ArenaIndex::Internal(i) = index {
                        node = self.in_nodes.get(i).unwrap();
                    }
                }
            }
        }

        Some(path)
    }

    pub fn first_leaf(&self) -> Option<LeafIndex> {
        let mut index = self.root;
        let mut node = self.in_nodes.get(index.unwrap_internal()).unwrap();
        loop {
            index = node.children.first()?.arena;
            match index {
                ArenaIndex::Leaf(leaf) => {
                    return Some(leaf.into());
                }
                ArenaIndex::Internal(index) => {
                    node = self.in_nodes.get(index).unwrap();
                }
            }
        }
    }

    pub fn last_leaf(&self) -> Option<LeafIndex> {
        let mut index = self.root;
        let mut node = self.in_nodes.get(index.unwrap_internal()).unwrap();
        loop {
            index = node.children.last()?.arena;
            match index {
                ArenaIndex::Leaf(leaf) => {
                    return Some(leaf.into());
                }
                ArenaIndex::Internal(index) => {
                    node = self.in_nodes.get(index).unwrap();
                }
            }
        }
    }

    pub fn range<Q>(&self, range: Range<Q::QueryArg>) -> Option<Range<QueryResult>>
    where
        Q: Query<B>,
    {
        if self.is_empty() {
            return None;
        }

        Some(self.query::<Q>(&range.start).unwrap()..self.query::<Q>(&range.end).unwrap())
    }

    pub fn iter_range(
        &self,
        range: impl RangeBounds<Cursor>,
    ) -> impl Iterator<Item = ElemSlice<'_, B::Elem>> + '_ {
        let start = match range.start_bound() {
            std::ops::Bound::Included(start) => *start,
            std::ops::Bound::Excluded(_) => unreachable!(),
            std::ops::Bound::Unbounded => self.start_cursor().unwrap(),
        };
        let (inclusive, end) = match range.end_bound() {
            std::ops::Bound::Included(end) => (true, *end),
            std::ops::Bound::Excluded(end) => (false, *end),
            std::ops::Bound::Unbounded => (true, self.end_cursor().unwrap()),
        };
        self._iter_range(start, end, inclusive)
    }

    fn _iter_range(
        &self,
        start: Cursor,
        end: Cursor,
        inclusive_end: bool,
    ) -> impl Iterator<Item = ElemSlice<'_, B::Elem>> + '_ {
        let node_iter = iter::Iter::new(
            self,
            self.get_path(start.leaf.into()),
            self.get_path(end.leaf.into()),
        );
        node_iter.filter_map(move |(path, node)| {
            let leaf = LeafIndex(path.last().unwrap().arena.unwrap_leaf());
            if end.leaf == leaf && end.offset == 0 && !inclusive_end {
                return None;
            }

            Some(ElemSlice {
                cursor: Cursor { leaf, offset: 0 },
                elem: &node.elem,
                start: if start.leaf == leaf {
                    Some(start.offset)
                } else {
                    None
                },
                end: if end.leaf == leaf {
                    Some(end.offset)
                } else {
                    None
                },
            })
        })
    }

    pub fn start_cursor(&self) -> Option<Cursor> {
        Some(Cursor {
            leaf: self.first_leaf()?,
            offset: 0,
        })
    }

    pub fn end_cursor(&self) -> Option<Cursor> {
        let leaf = self.last_leaf()?;
        let node = self.get_leaf(leaf.into());
        Some(Cursor {
            leaf,
            offset: node.elem.rle_len(),
        })
    }

    /// Split the internal node at path into two nodes recursively upwards.
    ///
    // at call site the cache at path can be out-of-date.
    // the cache will be up-to-date after this method
    fn split(&mut self, node_idx: ArenaIndex) {
        self.split_at(node_idx, None)
    }

    fn split_at(&mut self, node_idx: ArenaIndex, at: Option<usize>) {
        let (node_parent, node_parent_slot, this_cache, right_child) =
            self.split_node(node_idx, at);

        self.inner_insert_node(
            node_parent,
            node_parent_slot as usize,
            this_cache,
            right_child,
        );
        // don't need to recursive update cache
    }

    fn split_node(
        &mut self,
        node_idx: ArenaIndex,
        at: Option<usize>,
    ) -> (Option<ArenaIndex>, u8, <B as BTreeTrait>::Cache, Child<B>) {
        let node = self.in_nodes.get_mut(node_idx.unwrap_internal()).unwrap();
        let node_parent = node.parent;
        let node_parent_slot = node.parent_slot;
        let right: Node<B> = Node {
            parent: node.parent,
            parent_slot: u8::MAX,
            children: HeaplessVec::new(),
        };

        // split
        let split = at.unwrap_or(node.children.len() / 2);
        let right_children = HeaplessVec::from_slice(&node.children[split..]).unwrap();
        delete_range(&mut node.children, split..);

        // update cache
        let mut right_cache = B::Cache::default();
        let right_arena_idx = self.in_nodes.insert(right);
        let this_cache = {
            let node = self.get_internal_mut(node_idx);
            let mut cache = Default::default();
            node.calc_cache(&mut cache, None);
            cache
        };

        // update children's parent info
        for (i, child) in right_children.iter().enumerate() {
            if matches!(child.arena, ArenaIndex::Internal(_)) {
                let child = self.get_internal_mut(child.arena);
                child.parent = Some(ArenaIndex::Internal(right_arena_idx));
                child.parent_slot = i as u8;
            } else {
                self.get_leaf_mut(child.arena).parent = right_arena_idx;
            }
        }

        let right = self.in_nodes.get_mut(right_arena_idx).unwrap();
        right.children = right_children;
        // update parent cache
        right.calc_cache(&mut right_cache, None);
        let right_child = Child {
            arena: ArenaIndex::Internal(right_arena_idx),
            cache: right_cache,
        };
        (node_parent, node_parent_slot, this_cache, right_child)
    }

    // call site should ensure the cache is up-to-date after this method
    fn inner_insert_node(
        &mut self,
        parent_idx: Option<ArenaIndex>,
        index: usize,
        new_cache: B::Cache,
        node: Child<B>,
    ) {
        if let Some(parent_idx) = parent_idx {
            let parent = self.get_internal_mut(parent_idx);
            parent.children[index].cache = new_cache;
            parent.children.insert(index + 1, node).unwrap();
            let is_full = parent.is_full();
            self.update_children_parent_slot_from(parent_idx, index + 1);
            if is_full {
                self.split(parent_idx);
            }
        } else {
            self.split_root(new_cache, node);
        }
    }

    /// Update the `parent_slot` fields in `children[index..]`
    fn update_children_parent_slot_from(&mut self, parent_idx: ArenaIndex, index: usize) {
        let parent = self.get_internal_mut(parent_idx);
        if parent.children.len() <= index || parent.is_child_leaf() {
            return;
        }

        // PERF: Is there a way to avoid `take` like this?
        let children = take(&mut parent.children);
        for (i, child) in children[index..].iter().enumerate() {
            let idx = index + i;
            let child = self.get_internal_mut(child.arena);
            child.parent_slot = idx as u8;
        }
        let parent = self.get_internal_mut(parent_idx);
        parent.children = children;
    }

    /// right's cache should be up-to-date
    fn split_root(&mut self, new_cache: B::Cache, right: Child<B>) {
        let root_idx = self.root;
        // set right parent
        let right_node = &mut self.get_internal_mut(right.arena);
        right_node.parent_slot = 1;
        right_node.parent = Some(root_idx);
        let root = self.get_internal_mut(self.root);
        // let left be root
        let mut left_node: Node<B> = core::mem::replace(
            root,
            Node {
                parent: None,
                parent_slot: 0,
                children: Default::default(),
            },
        );
        left_node.parent_slot = 0;
        // set left parent
        left_node.parent = Some(root_idx);

        // push left and right to root.children
        root.children = Default::default();
        let left_children = left_node.children.clone();
        let left_arena = self.in_nodes.insert(left_node);
        let left = Child::new(ArenaIndex::Internal(left_arena), new_cache);
        let mut cache = std::mem::take(&mut self.root_cache);
        let root = self.get_internal_mut(self.root);
        root.children.push(left).unwrap();
        root.children.push(right).unwrap();

        // update new root cache
        root.calc_cache(&mut cache, None);

        for (i, child) in left_children.iter().enumerate() {
            if child.is_internal() {
                let node = self.get_internal_mut(child.arena);
                node.parent = Some(ArenaIndex::Internal(left_arena));
                node.parent_slot = i as u8;
            } else {
                self.get_leaf_mut(child.arena).parent = left_arena;
            }
        }

        self.root_cache = cache;
    }

    #[inline]
    pub fn get_internal_mut(&mut self, index: ArenaIndex) -> &mut Node<B> {
        self.in_nodes.get_mut(index.unwrap_internal()).unwrap()
    }

    #[inline]
    pub fn get_leaf_mut(&mut self, index: ArenaIndex) -> &mut LeafNode<B::Elem> {
        self.leaf_nodes.get_mut(index.unwrap_leaf()).unwrap()
    }

    #[inline]
    fn get2_mut(&mut self, a: ArenaIndex, b: ArenaIndex) -> (&mut Node<B>, &mut Node<B>) {
        let (a, b) = self
            .in_nodes
            .get2_mut(a.unwrap_internal(), b.unwrap_internal());
        (a.unwrap(), b.unwrap())
    }

    /// # Panic
    ///
    /// If the given index is not valid or deleted
    #[inline]
    pub fn get_internal(&self, index: ArenaIndex) -> &Node<B> {
        self.in_nodes.get(index.unwrap_internal()).unwrap()
    }

    #[inline]
    pub fn get_leaf(&self, index: ArenaIndex) -> &LeafNode<B::Elem> {
        self.leaf_nodes.get(index.unwrap_leaf()).unwrap()
    }

    /// The given node is lack of children.
    /// We should merge it into its neighbor or borrow from its neighbor.
    ///
    /// Given a random neighbor is neither full or lack, it's guaranteed
    /// that we can either merge into or borrow from it without breaking
    /// the balance rule.
    ///
    /// - The caches in parent's subtree should be up-to-date when calling this.
    /// - The caches in the parent node will be updated
    fn handle_lack_recursively(&mut self, node_idx: ArenaIndex) {
        let mut lack_info = self.handle_lack_single_layer(node_idx);
        while let Some(parent) = lack_info.parent_lack {
            lack_info = self.handle_lack_single_layer(parent);
        }
    }

    /// The given node is lack of children. This method doesn't handle parent's lack.
    ///
    /// - The caches in parent's subtree should be up-to-date when calling this.
    /// - The caches in the parent node will be updated
    fn handle_lack_single_layer(&mut self, node_idx: ArenaIndex) -> LackInfo {
        if self.root == node_idx {
            self.try_reduce_levels();
            return LackInfo { parent_lack: None };
        }

        let node = self.get_internal(node_idx);
        let parent_idx = node.parent.unwrap();
        let parent = self.get_internal(parent_idx);
        debug_assert_eq!(parent.children[node.parent_slot as usize].arena, node_idx);
        if node.children.is_empty() {
            let slot = node.parent_slot as usize;
            self.get_internal_mut(parent_idx).children.remove(slot);
            self.in_nodes.remove(node_idx.unwrap_internal());
            self.update_children_parent_slot_from(parent_idx, slot);
            return LackInfo {
                parent_lack: Some(parent_idx),
            };
        }
        let ans = match self.pair_neighbor(node_idx) {
            Some((a_idx, b_idx)) => {
                let parent = self.get_internal_mut(parent_idx);
                let mut a_cache = std::mem::take(&mut parent.children[a_idx.arr as usize].cache);
                let mut b_cache = std::mem::take(&mut parent.children[b_idx.arr as usize].cache);
                let mut re_parent = FxHashMap::default();

                let (a, b) = self
                    .in_nodes
                    .get2_mut(a_idx.arena.unwrap_internal(), b_idx.arena.unwrap_internal());
                let a = a.unwrap();
                let b = b.unwrap();
                let ans = if a.len() + b.len() >= MAX_CHILDREN_NUM {
                    // move partially
                    if a.len() < b.len() {
                        // move part of b's children to a
                        let move_len = (b.len() - a.len()) / 2;
                        for child in &b.children[..move_len] {
                            re_parent.insert(child.arena, (a_idx.arena, a.children.len()));
                            a.children.push(child.clone()).unwrap();
                        }
                        delete_range(&mut b.children, ..move_len);
                        for (i, child) in b.children.iter().enumerate() {
                            re_parent.insert(child.arena, (b_idx.arena, i));
                        }
                    } else {
                        // move part of a's children to b
                        let move_len = (a.len() - b.len()) / 2;
                        for (i, child) in b.children.iter().enumerate() {
                            re_parent.insert(child.arena, (b_idx.arena, i + move_len));
                        }
                        let mut b_children =
                            HeaplessVec::from_slice(&a.children[a.children.len() - move_len..])
                                .unwrap();
                        for child in take(&mut b.children) {
                            b_children.push(child).unwrap();
                        }
                        b.children = b_children;
                        for (i, child) in b.children.iter().enumerate() {
                            re_parent.insert(child.arena, (b_idx.arena, i));
                        }
                        let len = a.children.len();
                        delete_range(&mut a.children, len - move_len..);
                    }
                    a.calc_cache(&mut a_cache, None);
                    b.calc_cache(&mut b_cache, None);
                    let parent = self.get_internal_mut(parent_idx);
                    parent.children[a_idx.arr as usize].cache = a_cache;
                    parent.children[b_idx.arr as usize].cache = b_cache;
                    LackInfo {
                        parent_lack: if parent.is_lack() {
                            Some(parent_idx)
                        } else {
                            None
                        },
                    }
                } else {
                    // merge
                    let is_parent_lack = if node_idx == a_idx.arena {
                        // merge b to a, delete b
                        for (i, child) in b.children.iter().enumerate() {
                            re_parent.insert(child.arena, (a_idx.arena, a.children.len() + i));
                        }

                        for child in take(&mut b.children) {
                            a.children.push(child).unwrap();
                        }

                        a.calc_cache(&mut a_cache, None);
                        let parent = self.get_internal_mut(parent_idx);
                        parent.children[a_idx.arr as usize].cache = a_cache;
                        parent.children.remove(b_idx.arr as usize);
                        let is_lack = parent.is_lack();
                        self.purge(b_idx.arena);
                        self.update_children_parent_slot_from(parent_idx, b_idx.arr as usize);
                        is_lack
                    } else {
                        // merge a to b, delete a
                        for (i, child) in a.children.iter().enumerate() {
                            re_parent.insert(child.arena, (b_idx.arena, i));
                        }
                        for (i, child) in b.children.iter().enumerate() {
                            re_parent.insert(child.arena, (b_idx.arena, i + a.children.len()));
                        }

                        for child in take(&mut b.children) {
                            a.children.push(child).unwrap();
                        }

                        b.children = take(&mut a.children);
                        b.calc_cache(&mut b_cache, None);
                        let parent = self.get_internal_mut(parent_idx);
                        parent.children[b_idx.arr as usize].cache = b_cache;
                        parent.children.remove(a_idx.arr as usize);
                        let is_lack = parent.is_lack();
                        self.purge(a_idx.arena);
                        self.update_children_parent_slot_from(parent_idx, a_idx.arr as usize);
                        is_lack
                    };

                    LackInfo {
                        parent_lack: if is_parent_lack {
                            Some(parent_idx)
                        } else {
                            None
                        },
                    }
                };

                // FIXME: make this work
                if cfg!(debug_assertions) {
                    // let (a, b) = self
                    //     .in_nodes
                    //     .get2_mut(a_idx.arena.unwrap_internal(), b_idx.arena.unwrap_internal());
                    // if let Some(a) = a {
                    //     assert!(!a.is_lack() && !a.is_full());
                    // }
                    // if let Some(b) = b {
                    //     assert!(!b.is_lack() && !b.is_full());
                    // }
                }

                for (child, (parent, slot)) in re_parent {
                    match child {
                        ArenaIndex::Leaf(_) => {
                            let child = self.get_leaf_mut(child);
                            child.parent = parent.unwrap_internal();
                        }
                        ArenaIndex::Internal(_) => {
                            let child = self.get_internal_mut(child);
                            child.parent = Some(parent);
                            child.parent_slot = slot as u8;
                        }
                    }
                }
                ans
            }
            None => LackInfo {
                parent_lack: Some(parent_idx),
            },
        };
        ans
    }

    fn try_reduce_levels(&mut self) {
        let mut reduced = false;
        while self.get_internal(self.root).children.len() == 1 {
            let root = self.get_internal(self.root);
            if root.is_child_leaf() {
                break;
            }

            let child_arena = root.children[0].arena;
            let child = self.in_nodes.remove(child_arena.unwrap_internal()).unwrap();
            let root = self.get_internal_mut(self.root);
            let _ = core::mem::replace(root, child);
            reduced = true;
            // root cache should be the same as child cache because there is only one child
        }
        if reduced {
            let root_idx = self.root;
            let root = self.get_internal_mut(self.root);
            root.parent = None;
            root.parent_slot = u8::MAX;
            self.reset_children_parent_pointer(root_idx);
        }
    }

    fn reset_children_parent_pointer(&mut self, parent_idx: ArenaIndex) {
        let parent = self.in_nodes.get(parent_idx.unwrap_internal()).unwrap();
        let children = parent.children.clone();
        for child in children {
            match child.arena {
                ArenaIndex::Leaf(_) => {
                    let child = self.get_leaf_mut(child.arena);
                    child.parent = parent_idx.unwrap_internal();
                }
                ArenaIndex::Internal(_) => {
                    let child = self.get_internal_mut(child.arena);
                    child.parent = Some(parent_idx);
                }
            }
        }
    }

    fn pair_neighbor(&self, this: ArenaIndex) -> Option<(Idx, Idx)> {
        let node = self.get_internal(this);
        let arr = node.parent_slot as usize;
        let parent = self.get_internal(node.parent.unwrap());

        if arr == 0 {
            parent
                .children
                .get(1)
                .map(|x| (Idx::new(this, arr as u8), Idx::new(x.arena, 1)))
        } else {
            parent
                .children
                .get(arr - 1)
                .map(|x| (Idx::new(x.arena, arr as u8 - 1), Idx::new(this, arr as u8)))
        }
    }

    /// Sometimes we cannot use diff because no only the given node is changed, but also its siblings.
    /// For example, after delete a range of nodes, we cannot use the diff from child to infer the diff of parent.
    pub fn recursive_update_cache(
        &mut self,
        mut node_idx: ArenaIndex,
        can_use_diff: bool,
        cache_diff: Option<B::CacheDiff>,
    ) {
        if let ArenaIndex::Leaf(index) = node_idx {
            let leaf = self.leaf_nodes.get(index).unwrap();
            let cache = B::get_elem_cache(&leaf.elem);
            node_idx = leaf.parent();
            let node = self.get_internal_mut(node_idx);
            node.children
                .iter_mut()
                .find(|x| x.arena.unwrap_leaf() == index)
                .unwrap()
                .cache = cache;
        }

        if can_use_diff {
            if let Some(diff) = cache_diff {
                return self.recursive_update_cache_with_diff(node_idx, diff);
            }
        }

        let mut this_idx = node_idx;
        let mut node = self.get_internal_mut(node_idx);
        let mut this_arr = node.parent_slot;
        if can_use_diff {
            if let Some(parent_idx) = node.parent {
                let (parent, this) = self.get2_mut(parent_idx, this_idx);
                let diff =
                    this.calc_cache(&mut parent.children[this_arr as usize].cache, cache_diff);
                return self.recursive_update_cache_with_diff(parent_idx, diff);
            }
        } else {
            while let Some(parent_idx) = node.parent {
                let (parent, this) = self.get2_mut(parent_idx, this_idx);
                this.calc_cache(&mut parent.children[this_arr as usize].cache, None);
                this_idx = parent_idx;
                this_arr = parent.parent_slot;
                node = parent;
            }
        }

        let mut root_cache = std::mem::take(&mut self.root_cache);
        let root = self.root_mut();
        root.calc_cache(
            &mut root_cache,
            if can_use_diff { cache_diff } else { None },
        );
        self.root_cache = root_cache;
    }

    fn recursive_update_cache_with_diff(&mut self, node_idx: ArenaIndex, diff: B::CacheDiff) {
        let mut node = self.get_internal_mut(node_idx);
        let mut this_arr = node.parent_slot;
        while node.parent.is_some() {
            let parent_idx = node.parent.unwrap();
            let parent = self.get_internal_mut(parent_idx);
            B::apply_cache_diff(&mut parent.children[this_arr as usize].cache, &diff);
            this_arr = parent.parent_slot;
            node = parent;
        }

        B::apply_cache_diff(&mut self.root_cache, &diff);
    }

    fn purge(&mut self, index: ArenaIndex) {
        let mut stack = vec![index];
        while let Some(x) = stack.pop() {
            if let ArenaIndex::Leaf(index) = x {
                self.leaf_nodes.remove(index);

                continue;
            }

            let Some(node) = self.in_nodes.remove(x.unwrap()) else {
                continue;
            };

            for x in node.children.iter() {
                stack.push(x.arena);
            }
        }
    }

    /// find the next sibling at the same level
    ///
    /// return false if there is no next sibling
    #[must_use]
    fn next_sibling(&self, path: &mut [Idx]) -> bool {
        if path.len() <= 1 {
            return false;
        }

        let depth = path.len();
        let parent_idx = path[depth - 2];
        let this_idx = path[depth - 1];
        let parent = self.get_internal(parent_idx.arena);
        match parent.children.get(this_idx.arr as usize + 1) {
            Some(next) => {
                path[depth - 1] = Idx::new(next.arena, this_idx.arr + 1);
            }
            None => {
                if !self.next_sibling(&mut path[..depth - 1]) {
                    return false;
                }

                let parent = self.get_internal(path[depth - 2].arena);
                path[depth - 1] = Idx::new(parent.children[0].arena, 0);
            }
        }

        true
    }

    fn next_same_level_in_node(&self, node_idx: ArenaIndex) -> Option<ArenaIndex> {
        match node_idx {
            ArenaIndex::Leaf(_) => {
                let leaf_idx = node_idx.unwrap_leaf();
                let leaf1 = self.leaf_nodes.get(leaf_idx).unwrap();
                let parent1 = self.get_internal(leaf1.parent());
                let (leaf, parent, index) =
                    (leaf1, parent1, Self::get_leaf_slot(leaf_idx, parent1));
                if index + 1 < parent.children.len() {
                    Some(parent.children[index + 1].arena)
                } else if let Some(parent_next) = self.next_same_level_in_node(leaf.parent()) {
                    let parent_next = self.get_internal(parent_next);
                    Some(parent_next.children.first().unwrap().arena)
                } else {
                    None
                }
            }
            ArenaIndex::Internal(_) => {
                let node = self.get_internal(node_idx);
                let parent = self.get_internal(node.parent?);
                if let Some(next) = parent.children.get(node.parent_slot as usize + 1) {
                    Some(next.arena)
                } else if let Some(parent_next) = self.next_same_level_in_node(node.parent?) {
                    let parent_next = self.get_internal(parent_next);
                    parent_next.children.first().map(|x| x.arena)
                } else {
                    None
                }
            }
        }
    }

    fn prev_same_level_in_node(&self, node_idx: ArenaIndex) -> Option<ArenaIndex> {
        match node_idx {
            ArenaIndex::Leaf(leaf_idx) => {
                let leaf = self.leaf_nodes.get(leaf_idx).unwrap();
                let parent = self.get_internal(leaf.parent());
                let index = Self::get_leaf_slot(leaf_idx, parent);
                if index > 0 {
                    Some(parent.children[index - 1].arena)
                } else if let Some(parent_next) = self.prev_same_level_in_node(leaf.parent()) {
                    let parent_next = self.get_internal(parent_next);
                    Some(parent_next.children.last().unwrap().arena)
                } else {
                    None
                }
            }
            ArenaIndex::Internal(_) => {
                let node = self.get_internal(node_idx);
                let parent = self.get_internal(node.parent?);
                if node.parent_slot > 0 {
                    let Some(next) = parent.children.get(node.parent_slot as usize - 1) else {
                        unreachable!()
                    };
                    Some(next.arena)
                } else if let Some(parent_prev) = self.prev_same_level_in_node(node.parent?) {
                    let parent_prev = self.get_internal(parent_prev);
                    parent_prev.children.last().map(|x| x.arena)
                } else {
                    None
                }
            }
        }
    }

    /// find the next element in the tree
    pub fn next_elem(&self, path: Cursor) -> Option<Cursor> {
        self.next_same_level_in_node(path.leaf.into())
            .map(|x| Cursor {
                leaf: x.unwrap_leaf().into(),
                offset: 0,
            })
    }

    pub fn prev_elem(&self, path: Cursor) -> Option<Cursor> {
        self.prev_same_level_in_node(path.leaf.into())
            .map(|x| Cursor {
                leaf: x.unwrap_leaf().into(),
                offset: 0,
            })
    }

    #[inline(always)]
    pub fn root_cache(&self) -> &B::Cache {
        &self.root_cache
    }

    /// This method will release the memory back to OS.
    /// Currently, it's just `*self = Self::new()`
    #[inline(always)]
    pub fn clear(&mut self) {
        *self = Self::new();
    }

    #[inline(always)]
    fn root_mut(&mut self) -> &mut Node<B> {
        self.get_internal_mut(self.root)
    }

    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.get_internal(self.root).is_empty()
    }

    fn get_path(&self, idx: ArenaIndex) -> NodePath {
        let mut path = NodePath::new();
        let mut node_idx = idx;
        while node_idx != self.root {
            match node_idx {
                ArenaIndex::Leaf(inner_node_idx) => {
                    let node = self.leaf_nodes.get(inner_node_idx).unwrap();
                    let parent = self.in_nodes.get(node.parent).unwrap();
                    let index = Self::get_leaf_slot(inner_node_idx, parent);
                    path.push(Idx::new(node_idx, index as u8)).unwrap();
                    node_idx = ArenaIndex::Internal(node.parent);
                }
                ArenaIndex::Internal(_) => {
                    let node = self.get_internal(node_idx);
                    path.push(Idx::new(node_idx, node.parent_slot)).unwrap();
                    node_idx = node.parent.unwrap();
                }
            }
        }
        path.push(Idx::new(self.root, 0)).unwrap();
        path.reverse();
        path
    }

    pub fn push(&mut self, elem: B::Elem) -> Cursor {
        let mut is_full = false;
        let mut parent_idx = self.root;
        let mut update_cache_idx = parent_idx;
        let cache = B::get_elem_cache(&elem);
        let ans = if self.is_empty() {
            let data = self.alloc_leaf_child(elem, parent_idx.unwrap());
            let parent = self.in_nodes.get_mut(parent_idx.unwrap()).unwrap();
            let ans = data.arena;
            parent.children.push(data).unwrap();
            Cursor {
                leaf: ans.unwrap().into(),
                offset: 0,
            }
        } else {
            let leaf_idx = self.last_leaf().unwrap();
            let leaf = self.leaf_nodes.get_mut(leaf_idx.0).unwrap();
            parent_idx = leaf.parent();
            if leaf.elem.can_merge(&elem) {
                update_cache_idx = leaf_idx.into();
                let offset = leaf.elem.rle_len();
                leaf.elem.merge_right(&elem);
                Cursor {
                    leaf: leaf_idx,
                    offset,
                }
            } else {
                let data = self.alloc_leaf_child(elem, parent_idx.unwrap());
                let parent = self.in_nodes.get_mut(parent_idx.unwrap()).unwrap();
                let ans = data.arena;
                update_cache_idx = parent_idx;
                parent.children.push(data).unwrap();
                is_full = parent.is_full();
                Cursor {
                    leaf: ans.unwrap().into(),
                    offset: 0,
                }
            }
        };

        self.recursive_update_cache(
            update_cache_idx,
            B::USE_DIFF,
            if B::USE_DIFF {
                Some(B::new_cache_to_diff(&cache))
            } else {
                None
            },
        );
        if is_full {
            self.split(parent_idx);
        }

        ans
    }

    pub fn prepend(&mut self, elem: B::Elem) -> Cursor {
        let Some(leaf_idx) = self.first_leaf() else {
            let parent_idx = self.root;
            let data = self.alloc_leaf_child(elem, parent_idx.unwrap());
            let parent = self.in_nodes.get_mut(parent_idx.unwrap()).unwrap();
            let ans = data.arena;
            parent.children.push(data).unwrap();
            return Cursor {
                leaf: ans.unwrap().into(),
                offset: 0,
            };
        };
        let leaf = self.leaf_nodes.get_mut(leaf_idx.0).unwrap();
        let parent_idx = leaf.parent();
        let mut is_full = false;
        let ans = if elem.can_merge(&leaf.elem) {
            leaf.elem.merge_left(&elem);
            Cursor {
                leaf: leaf_idx,
                offset: 0,
            }
        } else {
            let parent_idx = leaf.parent;
            let data = self.alloc_leaf_child(elem, parent_idx);
            let parent = self.in_nodes.get_mut(parent_idx).unwrap();
            let ans = data.arena;
            parent.children.insert(0, data).unwrap();
            is_full = parent.is_full();
            Cursor {
                leaf: ans.unwrap().into(),
                offset: 0,
            }
        };

        self.recursive_update_cache(leaf_idx.into(), B::USE_DIFF, None);
        if is_full {
            self.split(parent_idx);
        }

        ans
    }

    /// compare the position of a and b
    pub fn compare_pos(&self, a: Cursor, b: Cursor) -> Ordering {
        if a.leaf == b.leaf {
            return a.offset.cmp(&b.offset);
        }

        let leaf_a = self.leaf_nodes.get(a.leaf.0).unwrap();
        let leaf_b = self.leaf_nodes.get(b.leaf.0).unwrap();
        let mut node_a = self.get_internal(leaf_a.parent());
        if leaf_a.parent == leaf_b.parent {
            for child in node_a.children.iter() {
                if child.arena.unwrap() == a.leaf.0 {
                    return Ordering::Less;
                }
                if child.arena.unwrap() == b.leaf.0 {
                    return Ordering::Greater;
                }
            }
        }

        let mut node_b = self.get_internal(leaf_b.parent());
        while node_a.parent != node_b.parent {
            node_a = self.get_internal(node_a.parent.unwrap());
            node_b = self.get_internal(node_b.parent.unwrap());
        }

        node_a.parent_slot.cmp(&node_b.parent_slot)
    }

    /// Iterate the caches of previous nodes/elements.
    /// This method will visit as less caches as possible.
    /// For example, if all nodes in a subtree need to be visited, we will only visit the root cache.
    ///
    /// f: (node_cache, previous_sibling_elem, (this_elem, offset))
    pub fn visit_previous_caches<F>(&self, cursor: Cursor, mut f: F)
    where
        F: FnMut(PreviousCache<'_, B>),
    {
        // the last index of path points to the leaf element
        let path = self.get_path(cursor.leaf.into());
        let mut path_index = 0;
        let mut child_index = 0;
        let mut node = self.get_internal(path[path_index].arena);
        'outer: loop {
            if path_index + 1 >= path.len() {
                break;
            }

            while child_index == path.get(path_index + 1).map(|x| x.arr).unwrap() {
                path_index += 1;
                if path_index + 1 < path.len() {
                    node = self.get_internal(path[path_index].arena);
                    child_index = 0;
                } else {
                    break 'outer;
                }
            }

            f(PreviousCache::NodeCache(
                &node.children[child_index as usize].cache,
            ));
            child_index += 1;
        }

        let node = self.leaf_nodes.get(cursor.leaf.0).unwrap();
        f(PreviousCache::ThisElemAndOffset {
            elem: &node.elem,
            offset: cursor.offset,
        });
    }

    pub fn diagnose_balance(&self) {
        let mut size_counter: FxHashMap<usize, usize> = Default::default();
        for (_, node) in self.in_nodes.iter() {
            *size_counter.entry(node.children.len()).or_default() += 1;
        }
        dbg!(size_counter);

        let mut size_counter: FxHashMap<usize, usize> = Default::default();
        for (_, node) in self.leaf_nodes.iter() {
            *size_counter.entry(node.elem.rle_len()).or_default() += 1;
        }
        dbg!(size_counter);
    }

    /// Iterate over the leaf elements in the tree if the filter returns true for all its ancestors' caches, including its own cache.
    pub fn iter_with_filter<'a, R: Default + Copy + AddAssign + 'a>(
        &'a self,
        mut f: impl FnMut(&B::Cache) -> (bool, R) + 'a,
    ) -> impl Iterator<Item = (R, &'a B::Elem)> + 'a {
        let mut queue = VecDeque::new();
        queue.push_back((self.root, R::default()));
        std::iter::from_fn(move || {
            while let Some((node_idx, mut r)) = queue.pop_front() {
                match node_idx {
                    ArenaIndex::Leaf(leaf) => {
                        let node = self.leaf_nodes.get(leaf).unwrap();
                        return Some((r, &node.elem));
                    }
                    ArenaIndex::Internal(idx) => {
                        let node = self.in_nodes.get(idx).unwrap();
                        for child in node.children.iter() {
                            let (drill, new_r) = f(&child.cache);
                            if drill {
                                queue.push_back((child.arena, r));
                            }
                            r += new_r;
                        }
                    }
                }
            }

            None
        })
    }

    /// This method allows users to update the caches and the elements with a filter.
    ///
    /// If `f` returns true for a node, it will drill down into the subtree whose root is the node.
    ///
    /// It's the caller's responsibility to ensure the invariance of caches being up to date.
    pub fn update_cache_and_elem_with_filter<'a>(
        &'a mut self,
        mut f: impl FnMut(&mut B::Cache) -> bool + 'a,
        mut g: impl FnMut(&mut B::Elem) + 'a,
    ) {
        let mut stack = vec![self.root];
        while let Some(node_idx) = stack.pop() {
            match node_idx {
                ArenaIndex::Leaf(leaf) => {
                    let node = self.leaf_nodes.get_mut(leaf).unwrap();
                    g(&mut node.elem);
                }
                ArenaIndex::Internal(idx) => {
                    let node = self.in_nodes.get_mut(idx).unwrap();
                    for child in node.children.iter_mut() {
                        if f(&mut child.cache) {
                            stack.push(child.arena);
                        }
                    }
                }
            }
        }
    }

    pub fn depth(&self) -> usize {
        let mut depth = 0;
        let mut index = self.root;
        let mut node = self.in_nodes.get(index.unwrap_internal()).unwrap();
        loop {
            depth += 1;
            index = node.children.first().unwrap().arena;
            match index {
                ArenaIndex::Leaf(_) => return depth,
                ArenaIndex::Internal(index) => {
                    node = self.in_nodes.get(index).unwrap();
                }
            }
        }
    }

    pub fn internal_avg_children_num(&self) -> f64 {
        let mut sum = 0;
        for (_, node) in self.in_nodes.iter() {
            sum += node.children.len();
        }
        sum as f64 / self.in_nodes.len() as f64
    }
}

fn merge_adj<E: Mergeable + Debug>(data: &mut Vec<E>) {
    // Merge adjacent elements
    let mut i = 0;
    let last = data.len() - 1;
    let mut to_delete_start = 0;
    let mut del_len = 0;
    while i < last {
        if data[i].can_merge(&data[i + 1]) {
            let (a, b) = arref::mut_twice(data.as_mut_slice(), i, i + 1).unwrap();
            a.merge_right(b);
            if del_len == 0 {
                to_delete_start = i + 1;
            }

            data.swap(i + 1, to_delete_start + del_len);
            del_len += 1;
            i += 1;
        }
        i += 1;
    }

    if del_len > 0 {
        data.drain(to_delete_start..to_delete_start + del_len);
    }
}

pub enum PreviousCache<'a, B: BTreeTrait> {
    NodeCache(&'a B::Cache),
    PrevSiblingElem(&'a B::Elem),
    ThisElemAndOffset { elem: &'a B::Elem, offset: usize },
}

#[inline(always)]
fn add_leaf_dirty_map<T>(leaf: ArenaIndex, dirty_map: &mut LeafDirtyMap<T>, leaf_diff: T) {
    dirty_map.insert(leaf, leaf_diff);
}

impl<B: BTreeTrait> BTree<B> {
    pub fn check(&self) {
        // check cache
        let mut leaf_level = None;
        for (index, node) in self.in_nodes.iter() {
            if index != self.root.unwrap() {
                assert!(!node.is_empty());
            }

            for (i, child_info) in node.children.iter().enumerate() {
                if matches!(child_info.arena, ArenaIndex::Internal(_)) {
                    assert!(!node.is_child_leaf());
                    let child = self.get_internal(child_info.arena);
                    let mut cache = Default::default();
                    child.calc_cache(&mut cache, None);
                    assert_eq!(child.parent_slot, i as u8);
                    assert_eq!(child.parent, Some(ArenaIndex::Internal(index)));
                    assert_eq!(
                        cache, child_info.cache,
                        "index={:?} child_index={:?}",
                        index, child_info.arena
                    );
                }
            }

            if let Some(parent) = node.parent {
                let parent = self.get_internal(parent);
                assert_eq!(
                    parent.children[node.parent_slot as usize].arena,
                    ArenaIndex::Internal(index)
                );
                self.get_path(ArenaIndex::Internal(index));
            } else {
                assert_eq!(index, self.root.unwrap_internal())
            }

            // if index != self.root.unwrap() {
            //     assert!(!node.is_lack(), "len={}\n", node.len());
            // }
            //
            // assert!(!node.is_full(), "len={}", node.len());
        }

        let root = self.get_internal(self.root);
        let mut root_cache = Default::default();
        root.calc_cache(&mut root_cache, None);
        assert_eq!(&self.root_cache, &root_cache);

        for (leaf_index, leaf_node) in self.leaf_nodes.iter() {
            let mut length = 1;
            let mut node_idx = leaf_node.parent;
            while node_idx != self.root.unwrap() {
                let node = self.get_internal(ArenaIndex::Internal(node_idx));
                length += 1;
                node_idx = node.parent.unwrap().unwrap();
            }
            match leaf_level {
                Some(expected) => {
                    if length != expected {
                        dbg!(leaf_index, leaf_node);
                        assert_eq!(length, expected);
                    }
                }
                None => {
                    leaf_level = Some(length);
                }
            }

            let cache = B::get_elem_cache(&leaf_node.elem);
            let parent = self.get_internal(leaf_node.parent());
            assert_eq!(
                parent
                    .children
                    .iter()
                    .find(|x| x.arena.unwrap_leaf() == leaf_index)
                    .unwrap()
                    .cache,
                cache
            );
            self.get_path(ArenaIndex::Leaf(leaf_index));
        }
    }
}

impl<B: BTreeTrait, T: Into<B::Elem>> FromIterator<T> for BTree<B> {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        let mut tree = Self::new();
        let iter = iter.into_iter();
        let min_size = iter.size_hint().0;
        tree.leaf_nodes.reserve(min_size);
        let max_child_size = MAX_CHILDREN_NUM - 2;

        struct TempInternalNode<B: BTreeTrait> {
            children: HeaplessVec<Child<B>, MAX_CHILDREN_NUM>,
            cache: B::Cache,
            arena_index: RawArenaIndex,
        }

        let parent_num = min_size.div_ceil(max_child_size);
        let mut internal_nodes: Vec<TempInternalNode<B>> = Vec::with_capacity(parent_num);
        let index = tree.in_nodes.insert(Default::default());
        internal_nodes.push(TempInternalNode {
            children: Default::default(),
            cache: Default::default(),
            arena_index: index,
        });

        // create all leaf nodes and their parents
        for elem in iter {
            let parent = match internal_nodes.last_mut() {
                Some(last) if last.children.len() < max_child_size => last,
                Some(last) => {
                    // calculate cache
                    B::calc_cache_internal(&mut last.cache, &last.children);
                    let index = tree.in_nodes.insert(Default::default());
                    internal_nodes.push(TempInternalNode {
                        children: Default::default(),
                        cache: Default::default(),
                        arena_index: index,
                    });
                    internal_nodes.last_mut().unwrap()
                }
                _ => unreachable!(),
            };

            let leaf = LeafNode {
                elem: elem.into(),
                parent: parent.arena_index,
            };

            let cache = B::get_elem_cache(&leaf.elem);
            let leaf_index = tree.leaf_nodes.insert(leaf);
            parent
                .children
                .push(Child {
                    arena: ArenaIndex::Leaf(leaf_index),
                    cache,
                })
                .unwrap();
        }

        // recursively create the internal nodes in higher level, until we reach root
        while internal_nodes.len() > 1 {
            let parent_num = internal_nodes.len().div_ceil(max_child_size);
            let children = std::mem::replace(&mut internal_nodes, Vec::with_capacity(parent_num));
            let index = tree.in_nodes.insert(Default::default());
            internal_nodes.push(TempInternalNode {
                children: Default::default(),
                cache: Default::default(),
                arena_index: index,
            });

            let mut parent_slot = 0;
            // eprintln!(
            //     "children.len={} max_child_size={}",
            //     children.len(),
            //     max_child_size
            // );
            for mut child in children {
                let parent = match internal_nodes.last_mut() {
                    Some(last) if last.children.len() < max_child_size => last,
                    Some(last) => {
                        // calculate cache
                        B::calc_cache_internal(&mut last.cache, &last.children);
                        let index = tree.in_nodes.insert(Default::default());
                        internal_nodes.push(TempInternalNode {
                            children: Default::default(),
                            cache: Default::default(),
                            arena_index: index,
                        });
                        internal_nodes.last_mut().unwrap()
                    }
                    _ => unreachable!(),
                };

                B::calc_cache_internal(&mut child.cache, &child.children);
                let child_node = tree.in_nodes.get_mut(child.arena_index).unwrap();
                child_node.children = child.children;
                child_node.parent = Some(ArenaIndex::Internal(parent.arena_index));
                child_node.parent_slot = parent_slot;
                parent_slot = (parent_slot + 1) % (max_child_size as u8);
                parent
                    .children
                    .push(Child {
                        arena: ArenaIndex::Internal(child.arena_index),
                        cache: child.cache,
                    })
                    .unwrap();
            }

            debug_assert_eq!(parent_num, internal_nodes.len());
        }

        debug_assert_eq!(internal_nodes.len(), 1);
        let node = internal_nodes.remove(0);
        B::calc_cache_internal(&mut tree.root_cache, &node.children);
        tree.in_nodes.remove(tree.root.unwrap());
        tree.root = ArenaIndex::Internal(node.arena_index);
        let root = tree.root.unwrap();
        tree.in_nodes.get_mut(root).unwrap().children = node.children;
        tree
    }
}

struct SplitInfo {
    new_pos: Option<Cursor>,
    left_neighbour: Option<LeafIndex>,
    parent_idx: RawArenaIndex,
    insert_slot: usize,
    new_leaf: Option<ArenaIndex>,
}

impl<B: BTreeTrait> Default for BTree<B> {
    fn default() -> Self {
        Self::new()
    }
}

fn delete_range<T: Clone, const N: usize>(
    arr: &mut heapless::Vec<T, N>,
    range: impl RangeBounds<usize>,
) {
    let start = match range.start_bound() {
        std::ops::Bound::Included(x) => *x,
        std::ops::Bound::Excluded(x) => x + 1,
        std::ops::Bound::Unbounded => 0,
    };
    let end = match range.end_bound() {
        std::ops::Bound::Included(x) => x + 1,
        std::ops::Bound::Excluded(x) => *x,
        std::ops::Bound::Unbounded => arr.len(),
    };

    if start == end {
        return;
    }

    if end - start == 1 {
        arr.remove(start);
        return;
    }

    let mut ans = heapless::Vec::from_slice(&arr[..start]).unwrap();
    ans.extend_from_slice(&arr[end..]).unwrap();
    *arr = ans;
}

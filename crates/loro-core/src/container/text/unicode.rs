use std::{
    iter::Sum,
    ops::{Add, Deref},
};

use rle::{
    rle_tree::{node::Node, tree_trait::FindPosResult, HeapMode, Position},
    HasLength, RleTreeTrait,
};

use super::string_pool::PoolString;

#[derive(Debug, Clone, Copy)]
pub(super) struct UnicodeTreeTrait<const SIZE: usize>;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct TextLength {
    pub utf8: u32,
    pub utf16: Option<u32>,
}

impl Add for TextLength {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        TextLength {
            utf8: self.utf8 + rhs.utf8,
            utf16: if let (Some(a), Some(b)) = (self.utf16, rhs.utf16) {
                Some(a + b)
            } else {
                None
            },
        }
    }
}

impl Sum for TextLength {
    fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
        let mut u8 = 0;
        let mut u16 = Some(0);
        for item in iter {
            u8 += item.utf8;
            if let (Some(a), Some(b)) = (u16, item.utf16) {
                u16 = Some(a + b);
            } else {
                u16 = None;
            }
        }

        Self {
            utf8: u8,
            utf16: u16,
        }
    }
}

impl<const SIZE: usize> RleTreeTrait<PoolString> for UnicodeTreeTrait<SIZE> {
    const MAX_CHILDREN_NUM: usize = SIZE;

    type Int = usize;

    type Cache = TextLength;

    type Arena = HeapMode;

    fn update_cache_leaf(
        node: &mut rle::rle_tree::node::LeafNode<'_, PoolString, Self>,
    ) -> Self::Cache {
        node.cache = node
            .children()
            .iter()
            .fold(TextLength::default(), |acc, cur| acc + cur.text_len());
        node.cache
    }

    fn update_cache_internal(
        node: &mut rle::rle_tree::node::InternalNode<'_, PoolString, Self>,
    ) -> Self::Cache {
        node.cache = node.children().iter().map(|x| x.cache).sum();
        node.cache
    }

    fn find_pos_internal(
        node: &rle::rle_tree::node::InternalNode<'_, PoolString, Self>,
        index: Self::Int,
    ) -> FindPosResult<Self::Int> {
        find_pos_internal(node, index, &|x| x.utf8 as usize)
    }

    fn find_pos_leaf(
        node: &rle::rle_tree::node::LeafNode<'_, PoolString, Self>,
        index: Self::Int,
    ) -> rle::rle_tree::tree_trait::FindPosResult<usize> {
        find_pos_leaf(node, index, &|x| x.atom_len())
    }

    fn get_index(
        node: &rle::rle_tree::node::LeafNode<'_, PoolString, Self>,
        mut child_index: usize,
    ) -> Self::Int {
        debug_assert!(!node.is_deleted());
        let mut index = 0;
        for i in 0..child_index {
            index += node.children()[i].content_len();
        }

        child_index = node.get_index_in_parent().unwrap();
        // SAFETY: parent is valid if node is valid
        let mut node = unsafe { node.parent().as_ref() };
        loop {
            for i in 0..child_index {
                index += node.children()[i].cache.utf8 as usize;
            }

            if let Some(parent) = node.parent() {
                child_index = node.get_index_in_parent().unwrap();
                // SAFETY: parent is valid if node is valid
                node = unsafe { parent.as_ref() };
            } else {
                break;
            }
        }

        index
    }

    fn len_leaf(node: &rle::rle_tree::node::LeafNode<'_, PoolString, Self>) -> Self::Int {
        node.cache.utf8 as usize
    }

    fn len_internal(node: &rle::rle_tree::node::InternalNode<'_, PoolString, Self>) -> Self::Int {
        node.cache.utf8 as usize
    }
}

#[inline(always)]
pub(super) fn find_pos_internal<F, const S: usize>(
    node: &rle::rle_tree::node::InternalNode<'_, PoolString, UnicodeTreeTrait<S>>,
    mut index: usize,
    f: &F,
) -> FindPosResult<usize>
where
    F: Fn(TextLength) -> usize,
{
    if node.children().is_empty() {
        return FindPosResult::new_not_found(0, 0, Position::Before);
    }

    let mut last_cache = 0;
    for (i, child) in node.children().iter().enumerate() {
        last_cache = match child.node.deref() {
            Node::Internal(x) => {
                if index <= f(x.cache) {
                    return FindPosResult::new(i, index, Position::get_pos(index, f(x.cache)));
                }
                f(x.cache)
            }
            Node::Leaf(x) => {
                if index <= f(x.cache) {
                    return FindPosResult::new(i, index, Position::get_pos(index, f(x.cache)));
                }
                f(x.cache)
            }
        };

        index -= last_cache;
    }

    if index > 0 {
        dbg!(&node);
        assert_eq!(index, 0);
    }
    FindPosResult::new(node.children().len() - 1, last_cache, Position::End)
}

#[inline(always)]
pub(super) fn find_pos_leaf<F, const S: usize>(
    node: &rle::rle_tree::node::LeafNode<'_, PoolString, UnicodeTreeTrait<S>>,
    mut index: usize,
    f: &F,
) -> FindPosResult<usize>
where
    F: Fn(&PoolString) -> usize,
{
    if node.children().is_empty() {
        return FindPosResult::new_not_found(0, 0, Position::Before);
    }

    for (i, child) in node.children().iter().enumerate() {
        if index < f(child) {
            return FindPosResult::new(i, index, Position::get_pos(index, f(child)));
        }

        index -= f(child);
    }

    FindPosResult::new(
        node.children().len() - 1,
        f(node.children().last().unwrap()),
        Position::End,
    )
}

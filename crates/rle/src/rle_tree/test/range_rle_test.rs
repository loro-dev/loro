use ctor::ctor;

use crate::rle_tree::tree_trait::Position;

use super::super::*;
use std::ops::Range;

#[derive(Debug)]
struct RangeTreeTrait;
impl RleTreeTrait<Range<usize>> for RangeTreeTrait {
    const MAX_CHILDREN_NUM: usize = 4;
    type Int = usize;
    type InternalCache = usize;
    type LeafCache = usize;

    fn update_cache_leaf(node: &mut node::LeafNode<'_, Range<usize>, Self>) {
        node.cache = node.children.iter().map(HasLength::len).sum();
    }

    fn update_cache_internal(node: &mut InternalNode<'_, Range<usize>, Self>) {
        node.cache = node
            .children
            .iter()
            .map(|x| match x {
                Node::Internal(x) => x.cache,
                Node::Leaf(x) => x.cache,
            })
            .sum();
    }

    fn find_pos_internal(
        node: &mut InternalNode<'_, Range<usize>, Self>,
        mut index: Self::Int,
    ) -> (usize, Self::Int, Position) {
        for (i, child) in node.children().iter().enumerate() {
            match child {
                Node::Internal(x) => {
                    if index <= x.cache {
                        return (i, index, get_pos(index, child));
                    }
                    index -= x.cache;
                }
                Node::Leaf(x) => {
                    if index <= x.cache {
                        return (i, index, get_pos(index, child));
                    }
                    index -= x.cache;
                }
            }
        }

        assert_eq!(index, 0);
        (node.children.len() - 1, index, Position::End)
    }

    fn find_pos_leaf(
        node: &mut node::LeafNode<'_, Range<usize>, Self>,
        mut index: Self::Int,
    ) -> (usize, usize, Position) {
        for (i, child) in node.children().iter().enumerate() {
            if index < HasLength::len(child) {
                return (i, index, get_pos(index, child));
            }

            index -= HasLength::len(child);
        }

        (
            node.children().len() - 1,
            HasLength::len(node.children.last().unwrap()),
            Position::End,
        )
    }

    const MIN_CHILDREN_NUM: usize = Self::MAX_CHILDREN_NUM / 2;

    fn len_leaf(node: &node::LeafNode<'_, Range<usize>, Self>) -> usize {
        node.cache
    }

    fn len_internal(node: &InternalNode<'_, Range<usize>, Self>) -> usize {
        node.cache
    }
}

fn get_pos<T: HasLength>(index: usize, child: &T) -> Position {
    if index == 0 {
        Position::Start
    } else if index == child.len() {
        Position::End
    } else {
        Position::Middle
    }
}

#[test]
fn insert() {
    let mut t: RleTree<Range<usize>, RangeTreeTrait> = RleTree::new();
    let tree = t.get_mut();
    tree.insert(0, 0..1);
    tree.insert(1, 4..8);
    tree.insert(5, 8..10);
    tree.insert(3, 101..108);
    tree.insert(2, 200..208);
    assert_eq!(tree.len(), 22);

    let ans = vec![0..1, 4..5, 200..208, 5..6, 101..108, 6..10];

    for (actual, expected) in tree.iter().zip(ans.iter()) {
        assert_eq!(actual, expected);
    }
}

#[test]
fn delete() {
    let mut t: RleTree<Range<usize>, RangeTreeTrait> = RleTree::new();
    let tree = t.get_mut();
    tree.insert(0, 0..10);
    tree.delete_range(Some(4), Some(5));
    assert_eq!(tree.len(), 9);

    let ans = vec![0..4, 5..10];
    for (actual, expected) in tree.iter().zip(ans.iter()) {
        assert_eq!(actual, expected);
    }
}

#[test]
fn insert_50times() {
    let mut t: RleTree<Range<usize>, RangeTreeTrait> = RleTree::new();
    let tree = t.get_mut();
    for i in (0..100).step_by(2) {
        assert_eq!(tree.len(), i / 2);
        tree.insert(tree.len(), i..i + 1);
        tree.debug_check();
    }
}

#[test]
fn deletion_that_need_merge_to_sibling() {
    let mut t: RleTree<Range<usize>, RangeTreeTrait> = RleTree::new();
    let tree = t.get_mut();
    for i in (0..18).step_by(2) {
        tree.insert(tree.len(), i..i + 1);
    }

    tree.delete_range(Some(1), Some(tree.len() - 1));
    tree.debug_check();
}

#[test]
fn delete_that_need_borrow_from_sibling() {
    let mut t: RleTree<Range<usize>, RangeTreeTrait> = RleTree::new();
    let tree = t.get_mut();
    for i in (0..16).step_by(2) {
        tree.insert(tree.len(), i..i + 1);
    }
    tree.delete_range(Some(2), Some(3));
    // Left [ 0..1, 2..3, 6..7 ]
    // Right [8..9, 10..11, 12..13, 14..15]

    tree.delete_range(Some(1), Some(2));
    {
        // Left [ 0..1, 6..7 ]
        // Right [8..9, 10..11, 12..13, 14..15]
        let left = &tree.node.as_internal().unwrap().children[0];
        assert_eq!(left.as_leaf().unwrap().cache, 2);
        let right = &tree.node.as_internal().unwrap().children[1];
        assert_eq!(right.as_leaf().unwrap().cache, 4);
    }

    tree.delete_range(Some(1), Some(2));
    {
        // Left [ 0..1, 8..9, 10..11 ]
        // Right [12..13, 14..15]
        let left = &tree.node.as_internal().unwrap().children[0];
        assert_eq!(left.as_leaf().unwrap().cache, 3);
        let right = &tree.node.as_internal().unwrap().children[1];
        assert_eq!(right.as_leaf().unwrap().cache, 2);
    }

    let expected = [0..1, 8..9, 10..11, 12..13, 14..15];
    for (actual, expected) in tree.iter().zip(expected.iter()) {
        assert_eq!(actual, expected);
    }

    tree.debug_check();
}

#[test]
fn delete_that_causes_removing_layers() {
    let mut t: RleTree<Range<usize>, RangeTreeTrait> = RleTree::new();
    let tree = t.get_mut();
    for i in (0..128).step_by(2) {
        tree.insert(tree.len(), i..i + 1);
    }
    tree.debug_check();
    tree.delete_range(Some(1), None);
    dbg!(tree);
}

#[ctor]
fn init_color_backtrace() {
    color_backtrace::install();
}

#[test]
fn delete_that_causes_increase_levels() {}

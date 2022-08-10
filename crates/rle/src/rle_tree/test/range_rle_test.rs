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
        node.cache = (node.children.iter().map(HasLength::len).sum());
    }

    fn update_cache_internal(node: &mut InternalNode<'_, Range<usize>, Self>) {
        node.cache = (node
            .children
            .iter()
            .map(|x| match x {
                Node::Internal(x) => x.cache,
                Node::Leaf(x) => x.cache,
            })
            .sum());
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
    tree.delete_range(4, 5);
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
    }
    tree.debug_check();
}

#[test]
fn delete_that_need_merge_to_sibling() {}

#[test]
fn delete_that_need_borrow_from_sibling() {}

#[test]
fn delete_that_causes_removing_a_level() {}

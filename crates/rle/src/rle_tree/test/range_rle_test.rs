use crate::rle_tree::tree_trait::CumulateTreeTrait;

use super::super::*;
use std::ops::Range;

type RangeTreeTrait = CumulateTreeTrait<Range<usize>, 4>;

#[test]
fn insert() {
    let mut t: RleTree<Range<usize>, RangeTreeTrait> = RleTree::default();
    t.with_tree_mut(|tree| {
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
    })
}

#[test]
fn delete() {
    let mut t: RleTree<Range<usize>, RangeTreeTrait> = RleTree::default();
    t.with_tree_mut(|tree| {
        tree.insert(0, 0..10);
        tree.delete_range(Some(4), Some(5));
        assert_eq!(tree.len(), 9);

        let ans = vec![0..4, 5..10];
        for (actual, expected) in tree.iter().zip(ans.iter()) {
            assert_eq!(actual, expected);
        }
    })
}

#[test]
fn insert_50times() {
    let mut t: RleTree<Range<usize>, RangeTreeTrait> = RleTree::default();
    t.with_tree_mut(|tree| {
        for i in (0..100).step_by(2) {
            assert_eq!(tree.len(), i / 2);
            tree.insert(tree.len(), i..i + 1);
            tree.debug_check();
        }
    });
}

#[test]
fn deletion_that_need_merge_to_sibling() {
    let mut t: RleTree<Range<usize>, RangeTreeTrait> = RleTree::default();
    t.with_tree_mut(|tree| {
        for i in (0..18).step_by(2) {
            tree.insert(tree.len(), i..i + 1);
        }

        tree.delete_range(Some(1), Some(tree.len() - 1));
        tree.debug_check();
    });
}

#[test]
fn delete_that_need_borrow_from_sibling() {
    let mut t: RleTree<Range<usize>, RangeTreeTrait> = RleTree::default();
    t.with_tree_mut(|tree| {
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
    });
}

#[test]
fn delete_that_causes_removing_layers() {
    let mut t: RleTree<Range<usize>, RangeTreeTrait> = RleTree::default();
    t.with_tree_mut(|tree| {
        for i in (0..128).step_by(2) {
            tree.insert(tree.len(), i..i + 1);
        }
        tree.debug_check();
        tree.delete_range(Some(1), None);
    });
}

#[test]
fn delete_that_causes_increase_levels() {
    let mut t: RleTree<Range<usize>, RangeTreeTrait> = RleTree::default();
    t.with_tree_mut(|tree| {
        tree.insert(0, 0..100);
        for i in 0..50 {
            tree.delete_range(Some(i), Some(i + 1));
            tree.debug_check();
        }

        dbg!(tree);
    });
}

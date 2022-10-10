use crate::rle_tree::tree_trait::CumulateTreeTrait;

use super::super::*;
use std::ops::Range;

type RangeTreeTrait = CumulateTreeTrait<Range<usize>, 4>;

#[test]
fn insert() {
    let mut tree: RleTree<Range<usize>, RangeTreeTrait> = RleTree::default();
    tree.insert(0, 0..1);
    tree.insert(1, 4..8);
    tree.insert(5, 8..10);
    tree.insert(3, 101..108);
    tree.insert(2, 200..208);
    assert_eq!(tree.len(), 22);

    let ans = vec![0..1, 4..5, 200..208, 5..6, 101..108, 6..10];

    for (actual, expected) in tree.iter().zip(ans.iter()) {
        assert_eq!(actual.as_ref(), expected);
    }
}

#[test]
fn delete() {
    let mut tree: RleTree<Range<usize>, RangeTreeTrait> = RleTree::default();
    tree.insert(0, 0..10);
    tree.delete_range(Some(4), Some(5));
    assert_eq!(tree.len(), 9);

    let ans = vec![0..4, 5..10];
    for (actual, expected) in tree.iter().zip(ans.iter()) {
        assert_eq!(actual.as_ref(), expected);
    }
}

#[test]
fn update_at_cursor() {
    let mut tree: RleTree<Range<usize>, RangeTreeTrait> = RleTree::default();
    tree.insert(0, 0..10);
    tree.insert(10, 11..20);
    tree.insert(19, 21..30);
    tree.insert(28, 31..40);
    for cursor in tree.iter_range(3, Some(7)) {
        // SAFETY: it's a test
        unsafe {
            cursor.0.update_with_split(
                |v| {
                    v.start = 4;
                    v.end = 5;
                },
                &mut |_, _| {},
            )
        }
    }

    // 12..16
    for cursor in tree.iter_range(8, Some(12)) {
        // SAFETY: it's a test
        unsafe {
            cursor.0.update_with_split(
                |v| {
                    v.start = 13;
                    v.end = 15;
                },
                &mut |_, _| {},
            )
        }
    }

    let arr: Vec<_> = tree.iter().map(|x| (*x).clone()).collect();
    assert_eq!(
        arr,
        vec![0..3, 4..5, 7..10, 11..12, 13..15, 16..20, 21..30, 31..40]
    );
}

#[test]
fn insert_50times() {
    let mut tree: RleTree<Range<usize>, RangeTreeTrait> = RleTree::default();
    for i in (0..100).step_by(2) {
        assert_eq!(tree.len(), i / 2);
        tree.insert(tree.len(), i..i + 1);
        tree.debug_check();
    }
}

#[test]
fn deletion_that_need_merge_to_sibling() {
    let mut tree: RleTree<Range<usize>, RangeTreeTrait> = RleTree::default();
    for i in (0..18).step_by(2) {
        tree.insert(tree.len(), i..i + 1);
    }

    tree.delete_range(Some(1), Some(tree.len() - 1));
    tree.debug_check();
}

#[test]
fn delete_that_need_borrow_from_sibling() {
    let mut tree: RleTree<Range<usize>, RangeTreeTrait> = RleTree::default();
    for i in (0..16).step_by(2) {
        tree.insert(tree.len(), i..i + 1);
    }
    tree.delete_range(Some(2), Some(3));
    // Left [ 0..1, 2..3, 6..7 ]
    // Right [8..9, 10..11, 12..13, 14..15]

    tree.delete_range(Some(1), Some(2));
    {
        tree.with_node(|node| {
            // Left [ 0..1, 6..7 ]
            // Right [8..9, 10..11, 12..13, 14..15]
            let left = &node.as_internal().unwrap().children[0];
            assert_eq!(left.as_leaf().unwrap().cache, 2);
            let right = &node.as_internal().unwrap().children[1];
            assert_eq!(right.as_leaf().unwrap().cache, 4);
        })
    }

    tree.delete_range(Some(1), Some(2));
    {
        tree.with_node(|node| {
            // Left [ 0..1, 8..9, 10..11 ]
            // Right [12..13, 14..15]
            let left = &node.as_internal().unwrap().children[0];
            assert_eq!(left.as_leaf().unwrap().cache, 3);
            let right = &node.as_internal().unwrap().children[1];
            assert_eq!(right.as_leaf().unwrap().cache, 2);
        })
    }

    let expected = [0..1, 8..9, 10..11, 12..13, 14..15];
    for (actual, expected) in tree.iter().zip(expected.iter()) {
        assert_eq!(actual.as_ref(), expected);
    }

    tree.debug_check();
}

#[test]
fn delete_that_causes_removing_layers() {
    let mut tree: RleTree<Range<usize>, RangeTreeTrait> = RleTree::default();
    for i in (0..128).step_by(2) {
        tree.insert(tree.len(), i..i + 1);
    }
    tree.debug_check();
    tree.delete_range(Some(1), None);
}

#[test]
fn delete_that_causes_increase_levels() {
    let mut tree: RleTree<Range<usize>, RangeTreeTrait> = RleTree::default();
    tree.insert(0, 0..100);
    for i in 0..50 {
        tree.delete_range(Some(i), Some(i + 1));
        tree.debug_check();
    }

    dbg!(tree);
}

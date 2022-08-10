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

    fn find_insert_pos_internal(
        node: &mut InternalNode<'_, Range<usize>, Self>,
        mut index: Self::Int,
    ) -> (usize, Self::Int) {
        for (i, child) in node.children().iter().enumerate() {
            match child {
                Node::Internal(x) => {
                    if index <= x.cache {
                        return (i, index);
                    }
                    index -= x.cache;
                }
                Node::Leaf(x) => {
                    if index <= x.cache {
                        return (i, index);
                    }
                    index -= x.cache;
                }
            }
        }

        (node.children.len(), index)
    }

    fn find_insert_pos_leaf(
        node: &mut node::LeafNode<'_, Range<usize>, Self>,
        mut index: Self::Int,
    ) -> (usize, usize) {
        for (i, child) in node.children().iter().enumerate() {
            if index < HasLength::len(child) {
                return (i, index);
            }

            index -= HasLength::len(child);
        }

        (node.children().len(), 0)
    }

    const MIN_CHILDREN_NUM: usize = Self::MAX_CHILDREN_NUM / 2;

    fn len_leaf(node: &node::LeafNode<'_, Range<usize>, Self>) -> usize {
        node.cache
    }

    fn len_internal(node: &InternalNode<'_, Range<usize>, Self>) -> usize {
        node.cache
    }
}

#[test]
fn insert() {
    let mut t: RleTree<Range<usize>, RangeTreeTrait> = RleTree::new();
    let tree = t.get_mut();
    tree.insert(0, 0..1);
    tree.insert(1, 4..10);
    tree.insert(3, 101..108);
    tree.insert(2, 200..208);
    assert_eq!(tree.len(), 22);

    let ans = vec![0..1, 4..5, 200..208, 5..6, 101..108, 6..10];

    for (actual, expected) in tree.iter().zip(ans.iter()) {
        assert_eq!(actual, expected);
    }
}

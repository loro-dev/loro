use crate::Rle;

use super::{node::LeafNode, tree_trait::RleTreeTrait};

pub struct Iter<'some, 'bump, T: Rle, A: RleTreeTrait<T>> {
    node: Option<&'some LeafNode<'bump, T, A>>,
    child_index: usize,
    end_node: Option<&'some LeafNode<'bump, T, A>>,
    end_index: Option<usize>,
}

impl<'some, 'bump, T: Rle, A: RleTreeTrait<T>> Iter<'some, 'bump, T, A> {
    pub fn new(node: Option<&'some LeafNode<'bump, T, A>>) -> Self {
        Self {
            node,
            child_index: 0,
            end_node: None,
            end_index: None,
        }
    }

    pub fn new_with_end(
        node: &'some LeafNode<'bump, T, A>,
        index: usize,
        end_node: Option<&'some LeafNode<'bump, T, A>>,
        end_index: Option<usize>,
    ) -> Self {
        Self {
            node: Some(node),
            child_index: index,
            end_node,
            end_index,
        }
    }
}

impl<'a, 'bump, T: Rle, A: RleTreeTrait<T>> Iterator for Iter<'a, 'bump, T, A> {
    type Item = &'a T;

    fn next(&mut self) -> Option<Self::Item> {
        if let (Some(end_node), Some(node), Some(end_index)) =
            (self.end_node, self.node, self.end_index)
        {
            if std::ptr::eq(end_node, node) && self.child_index == end_index {
                return None;
            }
        }

        while let Some(node) = self.node {
            match node.children.get(self.child_index) {
                Some(node) => {
                    self.child_index += 1;
                    return Some(node);
                }
                None => match node.next() {
                    Some(next) => {
                        if let Some(end_node) = self.end_node {
                            // if node == end_node, should not go to next node
                            // in this case end_index == node.children.len()
                            if std::ptr::eq(end_node, node) {
                                return None;
                            }
                        }

                        self.node = Some(next);
                        self.child_index = 0;
                        continue;
                    }
                    None => return None,
                },
            }
        }

        None
    }
}

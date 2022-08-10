use crate::Rle;

use super::{node::LeafNode, tree_trait::RleTreeTrait, RleTreeRaw};

pub struct Iter<'some, 'bump, T: Rle, A: RleTreeTrait<T>> {
    node: Option<&'some LeafNode<'bump, T, A>>,
    child_index: usize,
}

impl<'some, 'bump, T: Rle, A: RleTreeTrait<T>> Iter<'some, 'bump, T, A> {
    pub fn new(node: Option<&'some LeafNode<'bump, T, A>>) -> Self {
        Self {
            node,
            child_index: 0,
        }
    }
}

impl<'a, 'bump, T: Rle, A: RleTreeTrait<T>> Iterator for Iter<'a, 'bump, T, A> {
    type Item = &'a T;

    fn next(&mut self) -> Option<Self::Item> {
        while let Some(node) = self.node {
            match node.children.get(self.child_index) {
                Some(node) => {
                    self.child_index += 1;
                    return Some(node);
                }
                None => match node.next() {
                    Some(next) => {
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

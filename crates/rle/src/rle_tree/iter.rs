use crate::Rle;

use super::{
    node::LeafNode,
    tree_trait::{Position, RleTreeTrait},
    SafeCursor, SafeCursorMut,
};

pub struct Iter<'some, 'bump, T: Rle, A: RleTreeTrait<T>> {
    node: Option<&'some LeafNode<'bump, T, A>>,
    child_index: usize,
    end_node: Option<&'some LeafNode<'bump, T, A>>,
    end_index: Option<usize>,
}

pub struct IterMut<'some, 'bump, T: Rle, A: RleTreeTrait<T>> {
    node: Option<&'some mut LeafNode<'bump, T, A>>,
    child_index: usize,
    end_node: Option<&'some LeafNode<'bump, T, A>>,
    end_index: Option<usize>,
}

impl<'tree, 'bump, T: Rle, A: RleTreeTrait<T>> IterMut<'tree, 'bump, T, A> {
    #[inline]
    pub fn new(node: Option<&'tree mut LeafNode<'bump, T, A>>) -> Self {
        Self {
            node,
            child_index: 0,
            end_node: None,
            end_index: None,
        }
    }

    #[inline]
    pub fn from_cursor(
        mut start: SafeCursorMut<'tree, 'bump, T, A>,
        mut end: Option<SafeCursor<'tree, 'bump, T, A>>,
    ) -> Option<Self> {
        if start.0.pos == Position::After {
            start = start.next()?
        }

        if let Some(end_inner) = end {
            if end_inner.0.pos == Position::Middle
                || end_inner.0.pos == Position::End
                || end_inner.0.pos == Position::After
            {
                end = end_inner.next();
            }
        }

        Some(Self {
            node: Some(start.leaf_mut()),
            child_index: start.0.index,
            end_node: end.map(|end| end.leaf()),
            end_index: end.map(|end| end.index()),
        })
    }
}

impl<'tree, 'bump, T: Rle, A: RleTreeTrait<T>> Iter<'tree, 'bump, T, A> {
    #[inline]
    pub fn new(node: Option<&'tree LeafNode<'bump, T, A>>) -> Self {
        Self {
            node,
            child_index: 0,
            end_node: None,
            end_index: None,
        }
    }

    #[inline]
    pub fn from_cursor(
        mut start: SafeCursor<'tree, 'bump, T, A>,
        mut end: Option<SafeCursor<'tree, 'bump, T, A>>,
    ) -> Option<Self> {
        if start.0.pos == Position::After {
            start = start.next()?
        }

        if let Some(end_inner) = end {
            if end_inner.0.pos == Position::Middle
                || end_inner.0.pos == Position::End
                || end_inner.0.pos == Position::After
            {
                end = end_inner.next();
            }
        }

        Some(Self {
            node: Some(start.leaf()),
            child_index: start.0.index,
            end_node: end.map(|end| end.leaf()),
            end_index: end.map(|end| end.index()),
        })
    }
}

impl<'rf, 'bump, T: Rle, A: RleTreeTrait<T>> Iterator for Iter<'rf, 'bump, T, A> {
    type Item = SafeCursor<'rf, 'bump, T, A>;

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
                Some(_) => {
                    self.child_index += 1;
                    // SAFETY: we just checked that the child exists
                    return Some(unsafe {
                        SafeCursor::new(node.into(), self.child_index - 1, 0, Position::Start)
                    });
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

impl<'rf, 'bump, T: Rle, A: RleTreeTrait<T>> Iterator for IterMut<'rf, 'bump, T, A> {
    type Item = SafeCursorMut<'rf, 'bump, T, A>;

    fn next(&mut self) -> Option<Self::Item> {
        if let (Some(end_node), Some(node), Some(end_index)) = (
            self.end_node,
            self.node.as_mut().map(|x| *x as *const LeafNode<_, _>),
            self.end_index,
        ) {
            if std::ptr::eq(end_node, node as *const _) && self.child_index == end_index {
                return None;
            }
        }

        while let Some(node) = std::mem::take(&mut self.node) {
            let node_ptr = node as *const _;
            match node.children.get(self.child_index) {
                Some(_) => {
                    self.child_index += 1;
                    let leaf = node.into();
                    self.node = Some(node);
                    // SAFETY: we just checked that the child exists
                    return Some(unsafe {
                        SafeCursorMut::new(leaf, self.child_index - 1, 0, Position::Start)
                    });
                }
                None => match node.next_mut() {
                    Some(next) => {
                        if let Some(end_node) = self.end_node {
                            // if node == end_node, should not go to next node
                            // in this case end_index == node.children.len()
                            if std::ptr::eq(end_node, node_ptr) {
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

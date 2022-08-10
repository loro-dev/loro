use std::{
    marker::{PhantomData, PhantomPinned},
    pin::Pin,
    ptr::NonNull,
};

use crate::{HasLength, Rle};

use super::{
    fixed_size_vec::FixedSizedVec, tree_trait::RleTreeTrait, BumpBox, BumpVec, RleTreeRaw,
};
use bumpalo::Bump;
use enum_as_inner::EnumAsInner;
mod internal_impl;
mod leaf_impl;
pub(crate) mod node_trait;

#[derive(Debug, EnumAsInner)]
pub enum Node<'a, T: Rle, A: RleTreeTrait<T>> {
    Internal(BumpBox<'a, InternalNode<'a, T, A>>),
    Leaf(BumpBox<'a, LeafNode<'a, T, A>>),
}

#[derive(Debug)]
pub struct InternalNode<'a, T: Rle, A: RleTreeTrait<T>> {
    bump: &'a Bump,
    parent: Option<NonNull<InternalNode<'a, T, A>>>,
    pub(super) children: FixedSizedVec<'a, Node<'a, T, A>>,
    pub cache: A::InternalCache,
    _pin: PhantomPinned,
    _a: PhantomData<A>,
}

#[derive(Debug)]
pub struct LeafNode<'a, T: Rle, A: RleTreeTrait<T>> {
    bump: &'a Bump,
    parent: NonNull<InternalNode<'a, T, A>>,
    pub(super) children: FixedSizedVec<'a, T>,
    prev: Option<NonNull<LeafNode<'a, T, A>>>,
    next: Option<NonNull<LeafNode<'a, T, A>>>,
    pub cache: A::LeafCache,
    _pin: PhantomPinned,
    _a: PhantomData<A>,
}

#[derive(PartialEq, Eq, Debug)]
pub(crate) enum Either {
    Left,
    Right,
}

impl<'a, T: Rle, A: RleTreeTrait<T>> Node<'a, T, A> {
    #[inline]
    fn new_internal(bump: &'a Bump) -> Self {
        Self::Internal(BumpBox::new_in(InternalNode::new(bump, None), bump))
    }

    #[inline]
    fn new_leaf(bump: &'a Bump, parent: NonNull<InternalNode<'a, T, A>>) -> Self {
        Self::Leaf(BumpBox::new_in(LeafNode::new(bump, parent), bump))
    }

    #[inline]
    pub(crate) fn get_first_leaf(&self) -> Option<&LeafNode<'a, T, A>> {
        match self {
            Self::Internal(node) => node
                .children
                .get(0)
                .and_then(|child| child.get_first_leaf()),
            Self::Leaf(node) => Some(node),
        }
    }

    #[inline]
    fn children_num(&self) -> usize {
        match self {
            Node::Internal(node) => node.children.len(),
            Node::Leaf(node) => node.children.len(),
        }
    }

    #[inline]
    pub(crate) fn parent_mut(&mut self) -> Option<&mut InternalNode<'a, T, A>> {
        match self {
            Node::Internal(node) => unsafe { node.parent.map(|mut x| x.as_mut()) },
            Node::Leaf(node) => Some(unsafe { node.parent.as_mut() }),
        }
    }

    #[inline]
    pub(crate) fn parent(&self) -> Option<&InternalNode<'a, T, A>> {
        match self {
            Node::Internal(node) => node.parent.map(|x| unsafe { x.as_ref() }),
            Node::Leaf(node) => Some(unsafe { node.parent.as_ref() }),
        }
    }

    pub(crate) fn get_self_index(&self) -> Option<usize> {
        self.parent().map(|parent| {
            parent
                .children
                .iter()
                .position(|child| match (child, self) {
                    (Node::Internal(a), Node::Internal(b)) => std::ptr::eq(&**a, &**b),
                    (Node::Leaf(a), Node::Leaf(b)) => std::ptr::eq(&**a, &**b),
                    _ => false,
                })
                .unwrap()
        })
    }

    pub(crate) fn get_a_sibling(&self) -> Option<(&Self, Either)> {
        let index = self.get_self_index()?;
        let parent = self.parent()?;
        if index > 0 {
            Some((&parent.children[index - 1], Either::Left))
        } else if index + 1 < parent.children.len() {
            Some((&parent.children[index + 1], Either::Right))
        } else {
            None
        }
    }

    pub(crate) fn set_parent(&mut self, parent: NonNull<InternalNode<'a, T, A>>) {
        match self {
            Node::Internal(node) => node.parent = Some(parent),
            Node::Leaf(node) => node.parent = parent,
        }
    }

    // FIXME: change parent
    pub(crate) fn merge_to_sibling(&mut self, sibling: &mut Node<'a, T, A>, either: Either) {
        if either == Either::Left {
            match sibling {
                Node::Internal(sibling) => {
                    let self_node = self.as_internal_mut().unwrap();
                    let ptr = NonNull::new(&mut **sibling).unwrap();
                    for mut child in self_node.children.drain(..) {
                        child.set_parent(ptr);
                        sibling.children.push(child);
                    }
                }
                Node::Leaf(sibling) => {
                    let self_node = self.as_leaf_mut().unwrap();
                    for child in self_node.children.drain(..) {
                        sibling.children.push(child);
                    }
                }
            }
        } else {
            match sibling {
                Node::Internal(sibling) => {
                    let self_node = self.as_internal_mut().unwrap();
                    let ptr = NonNull::new(&mut **sibling).unwrap();
                    sibling.children.inner().splice(
                        0..0,
                        self_node.children.drain(0..).rev().map(|mut x| {
                            x.set_parent(ptr);
                            x
                        }),
                    );
                }
                Node::Leaf(sibling) => {
                    let self_node = self.as_leaf_mut().unwrap();
                    sibling
                        .children
                        .inner()
                        .splice(0..0, self_node.children.drain(0..).rev());
                }
            }
        }
    }

    // FIXME: change parent
    pub(crate) fn borrow_from_sibling(&mut self, sibling: &mut Node<'a, T, A>, either: Either) {
        if either == Either::Left {
            match sibling {
                Node::Internal(sibling) => {
                    let self_node = self.as_internal_mut().unwrap();
                    let self_ptr = NonNull::new(&mut **self_node).unwrap();
                    let sibling_drain =
                        sibling.children.drain(A::MIN_CHILDREN_NUM..).map(|mut x| {
                            x.set_parent(self_ptr);
                            x
                        });
                    self_node.children.inner().splice(0..0, sibling_drain);
                }
                Node::Leaf(sibling) => {
                    let self_node = self.as_leaf_mut().unwrap();
                    let sibling_drain = sibling.children.drain(A::MIN_CHILDREN_NUM..);
                    self_node.children.inner().splice(0..0, sibling_drain);
                }
            }
        } else {
            match sibling {
                Node::Internal(sibling) => {
                    let self_node = self.as_internal_mut().unwrap();
                    let self_ptr = NonNull::new(&mut **self_node).unwrap();
                    let end = self_node.children.len();
                    let sibling_len = sibling.children.len();
                    self_node.children.inner().splice(
                        end..end,
                        sibling
                            .children
                            .drain(0..sibling_len - A::MIN_CHILDREN_NUM)
                            .map(|mut x| {
                                x.set_parent(self_ptr);
                                x
                            }),
                    );
                }
                Node::Leaf(sibling) => {
                    let self_node = self.as_leaf_mut().unwrap();
                    let end = self_node.children.len();
                    let sibling_len = sibling.children.len();
                    self_node.children.inner().splice(
                        end..end,
                        sibling.children.drain(0..sibling_len - A::MIN_CHILDREN_NUM),
                    );
                }
            }
        }
    }

    pub(crate) fn remove(&mut self) {
        let index = self.get_self_index().unwrap();
        let parent = self.parent_mut().unwrap();
        for _ in parent.children.drain(index..index + 1) {}
    }
}

impl<'a, T: Rle, A: RleTreeTrait<T>> HasLength for Node<'a, T, A> {
    #[inline]
    fn len(&self) -> usize {
        match self {
            Node::Internal(node) => node.len(),
            Node::Leaf(node) => node.len(),
        }
    }
}

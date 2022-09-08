use std::{
    fmt::Debug,
    marker::{PhantomData, PhantomPinned},
    ptr::NonNull,
};

use crate::Rle;

use super::{cursor::SafeCursor, tree_trait::RleTreeTrait, BumpVec};
use bumpalo::Bump;
use enum_as_inner::EnumAsInner;
mod internal_impl;
mod leaf_impl;
pub(crate) mod node_trait;

#[derive(EnumAsInner)]
pub enum Node<'a, T: Rle, A: RleTreeTrait<T>> {
    Internal(InternalNode<'a, T, A>),
    Leaf(LeafNode<'a, T, A>),
}

pub struct InternalNode<'a, T: Rle, A: RleTreeTrait<T>> {
    bump: &'a Bump,
    pub(super) parent: Option<NonNull<InternalNode<'a, T, A>>>,
    pub(super) children: BumpVec<'a, &'a mut Node<'a, T, A>>,
    pub cache: A::InternalCache,
    _pin: PhantomPinned,
    _a: PhantomData<A>,
}

// TODO: remove bump field
// TODO: remove parent field?
// TODO: only one child?
pub struct LeafNode<'a, T: Rle, A: RleTreeTrait<T>> {
    bump: &'a Bump,
    pub(super) parent: NonNull<InternalNode<'a, T, A>>,
    pub(super) children: BumpVec<'a, &'a mut T>,
    pub(crate) prev: Option<NonNull<LeafNode<'a, T, A>>>,
    pub(crate) next: Option<NonNull<LeafNode<'a, T, A>>>,
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
    fn _new_internal(bump: &'a Bump, parent: Option<NonNull<InternalNode<'a, T, A>>>) -> Self {
        Self::Internal(InternalNode::new(bump, parent))
    }

    #[inline]
    fn new_leaf(bump: &'a Bump, parent: NonNull<InternalNode<'a, T, A>>) -> &'a mut Self {
        bump.alloc(Self::Leaf(LeafNode::new(bump, parent)))
    }

    #[inline]
    pub(crate) fn get_first_leaf(&self) -> Option<&LeafNode<'a, T, A>> {
        match self {
            Self::Internal(node) => node
                .children
                .first()
                .and_then(|child| child.get_first_leaf()),
            Self::Leaf(node) => Some(node),
        }
    }

    #[inline]
    pub(crate) fn get_last_leaf(&self) -> Option<&LeafNode<'a, T, A>> {
        match self {
            Self::Internal(node) => node.children.last().and_then(|child| child.get_last_leaf()),
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
            let ans = parent
                .children
                .iter()
                .position(|child| match (child, self) {
                    (Node::Internal(a), Node::Internal(b)) => std::ptr::eq(a, b),
                    (Node::Leaf(a), Node::Leaf(b)) => std::ptr::eq(a, b),
                    _ => false,
                });

            #[cfg(debug_assertions)]
            if ans.is_none() {
                dbg!(parent);
                dbg!(self);
                unreachable!();
            }

            ans.unwrap()
        })
    }

    pub(crate) fn get_a_sibling(&self) -> Option<(&Self, Either)> {
        let index = self.get_self_index()?;
        let parent = self.parent()?;
        if index > 0 {
            Some((parent.children[index - 1], Either::Left))
        } else if index + 1 < parent.children.len() {
            Some((parent.children[index + 1], Either::Right))
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

    pub(crate) fn merge_to_sibling<F>(
        &mut self,
        sibling: &mut Node<'a, T, A>,
        either: Either,
        notify: &mut F,
    ) where
        F: FnMut(&T, *mut LeafNode<'a, T, A>),
    {
        if either == Either::Left {
            match sibling {
                Node::Internal(sibling) => {
                    let self_node = self.as_internal_mut().unwrap();
                    let ptr = NonNull::new(&mut *sibling).unwrap();
                    for child in self_node.children.drain(..) {
                        child.set_parent(ptr);
                        sibling.children.push(child);
                    }
                }
                Node::Leaf(sibling) => {
                    let self_node = self.as_leaf_mut().unwrap();
                    for child in self_node.children.drain(..) {
                        notify(child, sibling);
                        sibling.children.push(child);
                    }
                }
            }
        } else {
            match sibling {
                Node::Internal(sibling) => {
                    let self_node = self.as_internal_mut().unwrap();
                    let ptr = NonNull::new(&mut *sibling).unwrap();
                    sibling.children.splice(
                        0..0,
                        self_node.children.drain(0..).map(|x| {
                            x.set_parent(ptr);
                            x
                        }),
                    );
                }
                Node::Leaf(sibling) => {
                    let self_node = self.as_leaf_mut().unwrap();
                    let sibling_ptr = sibling as *mut _;
                    sibling.children.splice(
                        0..0,
                        self_node.children.drain(0..).map(|x| {
                            notify(x, sibling_ptr);
                            x
                        }),
                    );
                }
            }
        }

        self.update_cache();
        sibling.update_cache();
    }

    pub(crate) fn borrow_from_sibling<F>(
        &mut self,
        sibling: &mut Node<'a, T, A>,
        either: Either,
        notify: &mut F,
    ) where
        F: FnMut(&T, *mut LeafNode<'a, T, A>),
    {
        if either == Either::Left {
            match sibling {
                Node::Internal(sibling) => {
                    let self_node = self.as_internal_mut().unwrap();
                    let self_ptr = NonNull::new(&mut *self_node).unwrap();
                    let sibling_drain = sibling.children.drain(A::MIN_CHILDREN_NUM..).map(|x| {
                        x.set_parent(self_ptr);
                        x
                    });
                    self_node.children.splice(0..0, sibling_drain);
                }
                Node::Leaf(sibling) => {
                    let self_node = self.as_leaf_mut().unwrap();
                    let self_ptr = self_node as *mut _;
                    let sibling_drain = sibling.children.drain(A::MIN_CHILDREN_NUM..).map(|x| {
                        notify(x, self_ptr);
                        x
                    });
                    self_node.children.splice(0..0, sibling_drain);
                }
            }
        } else {
            match sibling {
                Node::Internal(sibling) => {
                    let self_node = self.as_internal_mut().unwrap();
                    let self_ptr = NonNull::new(&mut *self_node).unwrap();
                    let end = self_node.children.len();
                    let sibling_len = sibling.children.len();
                    self_node.children.splice(
                        end..end,
                        sibling
                            .children
                            .drain(0..sibling_len - A::MIN_CHILDREN_NUM)
                            .map(|x| {
                                x.set_parent(self_ptr);
                                x
                            }),
                    );
                }
                Node::Leaf(sibling) => {
                    let self_node = self.as_leaf_mut().unwrap();
                    let end = self_node.children.len();
                    let sibling_len = sibling.children.len();
                    let self_ptr = self_node as *mut _;
                    self_node.children.splice(
                        end..end,
                        sibling
                            .children
                            .drain(0..sibling_len - A::MIN_CHILDREN_NUM)
                            .map(|x| {
                                notify(x, self_ptr);
                                x
                            }),
                    );
                }
            }
        }

        self.update_cache();
        sibling.update_cache();
    }

    pub(crate) fn remove(&mut self) {
        let index = self.get_self_index().unwrap();
        let parent = self.parent_mut().unwrap();
        for _ in parent.children.drain(index..index + 1) {}
    }

    pub(crate) fn update_cache(&mut self) {
        match self {
            Node::Internal(node) => A::update_cache_internal(node),
            Node::Leaf(node) => A::update_cache_leaf(node),
        }
    }
}

impl<'a, T: Rle, A: RleTreeTrait<T>> Node<'a, T, A> {
    #[inline]
    pub fn len(&self) -> A::Int {
        match self {
            Node::Internal(node) => A::len_internal(node),
            Node::Leaf(node) => A::len_leaf(node),
        }
    }
}

impl<'a, T: Rle, A: RleTreeTrait<T>> Debug for Node<'a, T, A> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Internal(arg0) => arg0.fmt(f),
            Self::Leaf(arg0) => arg0.fmt(f),
        }
    }
}

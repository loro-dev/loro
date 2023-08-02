use std::{
    fmt::Debug,
    marker::{PhantomData, PhantomPinned},
    ops::Deref,
    ptr::NonNull,
};

use crate::Rle;

use super::{
    arena::{Arena, VecTrait},
    cursor::SafeCursor,
    tree_trait::{ArenaBoxedNode, ArenaVec, RleTreeTrait},
};
use enum_as_inner::EnumAsInner;
mod internal_impl;
mod leaf_impl;
pub(crate) mod node_trait;
mod utils;

#[derive(EnumAsInner)]
pub enum Node<'a, T: Rle, A: RleTreeTrait<T>> {
    Internal(InternalNode<'a, T, A>),
    Leaf(LeafNode<'a, T, A>),
}

#[derive(Debug)]
pub struct Child<'a, T: Rle, A: RleTreeTrait<T>> {
    pub node: ArenaBoxedNode<'a, T, A>,
    pub parent_cache: A::CacheInParent,
}

impl<'a, T: Rle, A: RleTreeTrait<T>> Child<'a, T, A> {
    pub fn from(node: ArenaBoxedNode<'a, T, A>) -> Self {
        Self {
            parent_cache: node.cache().into(),
            node,
        }
    }
}

pub struct InternalNode<'a, T: Rle + 'a, A: RleTreeTrait<T> + 'a> {
    bump: &'a A::Arena,
    pub(crate) parent: Option<NonNull<InternalNode<'a, T, A>>>,
    pub(super) children: ArenaVec<'a, T, A, Child<'a, T, A>>,
    pub cache: A::Cache,
    _pin: PhantomPinned,
    _a: PhantomData<A>,
}

// TODO: remove bump field
pub struct LeafNode<'a, T: Rle + 'a, A: RleTreeTrait<T>> {
    bump: &'a A::Arena,
    pub(crate) parent: NonNull<InternalNode<'a, T, A>>,
    pub(crate) children: <A::Arena as Arena>::Vec<'a, T>,
    pub(crate) prev: Option<NonNull<LeafNode<'a, T, A>>>,
    pub(crate) next: Option<NonNull<LeafNode<'a, T, A>>>,
    pub cache: A::Cache,
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
    fn _new_internal(bump: &'a A::Arena, parent: Option<NonNull<InternalNode<'a, T, A>>>) -> Self {
        Self::Internal(InternalNode::new(bump, parent))
    }

    #[inline]
    fn new_leaf(
        bump: &'a A::Arena,
        parent: NonNull<InternalNode<'a, T, A>>,
    ) -> <A::Arena as Arena>::Boxed<'a, Self> {
        bump.allocate(Self::Leaf(LeafNode::new(bump, parent)))
    }

    #[inline]
    pub(crate) fn get_first_leaf(&self) -> Option<&LeafNode<'a, T, A>> {
        match self {
            Self::Internal(node) => node
                .children
                .first()
                .and_then(|child| child.node.get_first_leaf()),
            Self::Leaf(node) => Some(node),
        }
    }

    #[inline]
    pub(crate) fn get_first_leaf_mut(&mut self) -> Option<&mut LeafNode<'a, T, A>> {
        match self {
            Self::Internal(node) => node
                .children
                .first_mut()
                .and_then(|child| child.node.get_first_leaf_mut()),
            Self::Leaf(node) => Some(node),
        }
    }

    #[inline]
    pub(crate) fn get_last_leaf(&self) -> Option<&LeafNode<'a, T, A>> {
        match self {
            Self::Internal(node) => node
                .children
                .last()
                .and_then(|child| child.node.get_last_leaf()),
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
        // SAFETY: all tree data is pinned and can only be operating on single thread
        unsafe {
            match self {
                Node::Internal(node) => node.parent.map(|mut x| x.as_mut()),
                Node::Leaf(node) => Some(node.parent.as_mut()),
            }
        }
    }

    #[inline]
    pub(crate) fn parent(&self) -> Option<&InternalNode<'a, T, A>> {
        // SAFETY: all tree data is pinned and can only be operating on single thread
        unsafe {
            match self {
                Node::Internal(node) => node.parent.map(|x| x.as_ref()),
                Node::Leaf(node) => Some(node.parent.as_ref()),
            }
        }
    }

    pub(crate) fn get_self_index(&self) -> Option<usize> {
        self.parent().map(|parent| {
            let ans = parent
                .children
                .iter()
                .position(|child| match (child.node.deref(), self) {
                    (Node::Internal(a), Node::Internal(b)) => std::ptr::eq(a, b),
                    (Node::Leaf(a), Node::Leaf(b)) => std::ptr::eq(a, b),
                    _ => false,
                });

            #[cfg(debug_assertions)]
            if ans.is_none() {
                unreachable!();
            }

            ans.unwrap()
        })
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
                    for mut child in self_node.children.drain(..) {
                        child.node.set_parent(ptr);
                        sibling.children.push(child);
                    }
                }
                Node::Leaf(sibling) => {
                    let self_node = self.as_leaf_mut().unwrap();
                    for child in self_node.children.drain(..) {
                        notify(&child, sibling);
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
                        self_node.children.drain(0..).map(|mut x| {
                            x.node.set_parent(ptr);
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
                            notify(&x, sibling_ptr);
                            x
                        }),
                    );
                }
            }
        }

        // TODO: Perf
        self.update_cache(None);
        sibling.update_cache(None);
    }

    #[inline(always)]
    pub(crate) fn child_num(&self) -> usize {
        match self {
            Node::Internal(x) => x.children.len(),
            Node::Leaf(x) => x.children.len(),
        }
    }

    #[inline(always)]
    pub(crate) fn is_empty(&self) -> bool {
        self.child_num() == 0
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
                    let sibling_drain =
                        sibling.children.drain(A::MIN_CHILDREN_NUM..).map(|mut x| {
                            x.node.set_parent(self_ptr);
                            x
                        });
                    self_node.children.splice(0..0, sibling_drain);
                }
                Node::Leaf(sibling) => {
                    let self_node = self.as_leaf_mut().unwrap();
                    let self_ptr = self_node as *mut _;
                    let sibling_drain = sibling.children.drain(A::MIN_CHILDREN_NUM..).map(|x| {
                        notify(&x, self_ptr);
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
                            .map(|mut x| {
                                x.node.set_parent(self_ptr);
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
                                notify(&x, self_ptr);
                                x
                            }),
                    );
                }
            }
        }

        // TODO: perf
        self.update_cache(None);
        sibling.update_cache(None);
    }

    pub(crate) fn update_cache(&mut self, update: Option<A::CacheInParent>) -> A::CacheInParent {
        match self {
            Node::Internal(node) => A::update_cache_internal(node, update),
            Node::Leaf(node) => A::update_cache_leaf(node),
        }
    }

    pub(crate) fn recursive_visit_all(&self, f: &mut impl FnMut(&Node<T, A>)) {
        f(self);
        match self {
            Node::Internal(node) => {
                for child in node.children.deref() {
                    child.node.recursive_visit_all(f);
                }
            }
            Node::Leaf(_) => {}
        }
    }

    #[inline(always)]
    pub fn cache(&self) -> A::Cache {
        match self {
            Node::Internal(x) => x.cache,
            Node::Leaf(x) => x.cache,
        }
    }

    pub(crate) fn is_deleted(&self) -> bool {
        match self {
            Node::Internal(node) => {
                let mut node = node;
                while let Some(parent) = node.parent() {
                    if self.get_self_index().is_none() {
                        return true;
                    }

                    // SAFETY: parent is a valid pointer
                    node = unsafe { parent.as_ref() };
                }

                false
            }
            Node::Leaf(x) => x.is_deleted(),
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

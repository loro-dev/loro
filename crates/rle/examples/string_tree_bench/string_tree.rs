use std::{
    borrow::{Borrow, BorrowMut},
    cell::{Ref, RefCell, RefMut},
    ops::{Deref, DerefMut, Range, RangeBounds},
    rc::Rc,
};

use rle::{
    rle_tree::{
        node::{InternalNode, LeafNode, Node},
        tree_trait::{Position, RleTreeTrait},
    },
    HasLength, Mergable, Sliceable,
};
use smartstring::SmartString;

type SString = SmartString<smartstring::LazyCompact>;

#[derive(Debug)]
pub struct CustomString {
    str: Rc<RefCell<SString>>,
    slice: Range<usize>,
}

impl CustomString {
    fn str(&self) -> Ref<'_, SString> {
        RefCell::borrow(&self.str)
    }

    fn str_mut(&self) -> RefMut<'_, SString> {
        RefCell::borrow_mut(&self.str)
    }
}

impl HasLength for CustomString {
    fn len(&self) -> usize {
        rle::HasLength::len(&self.slice)
    }
}

impl Mergable for CustomString {
    fn is_mergable(&self, other: &Self, _conf: &()) -> bool
    where
        Self: Sized,
    {
        self.slice.start == 0
            && self.slice.end == self.str().len()
            && self.str().capacity() > other.len() + self.str().len()
            && Rc::strong_count(&self.str) == 1
    }

    fn merge(&mut self, other: &Self, _conf: &())
    where
        Self: Sized,
    {
        self.str_mut().push_str(other.str().as_str());
        let length = self.str().len();
        self.borrow_mut().slice.end = length;
    }
}

impl Sliceable for CustomString {
    fn slice(&self, from: usize, to: usize) -> Self {
        CustomString {
            str: self.str.clone(),
            slice: self.slice.start + from..self.slice.start + to,
        }
    }
}

#[derive(Debug)]
pub struct StringTreeTrait;
impl RleTreeTrait<CustomString> for StringTreeTrait {
    const MAX_CHILDREN_NUM: usize = 4;

    const MIN_CHILDREN_NUM: usize = Self::MAX_CHILDREN_NUM / 2;

    type Int = usize;

    type InternalCache = usize;

    type LeafCache = usize;

    fn update_cache_leaf(node: &mut rle::rle_tree::node::LeafNode<'_, CustomString, Self>) {
        node.cache = node.children().iter().map(|x| HasLength::len(&**x)).sum();
    }

    fn update_cache_internal(node: &mut rle::rle_tree::node::InternalNode<'_, CustomString, Self>) {
        node.cache = node.children().iter().map(|x| Node::len(x)).sum();
    }

    fn find_pos_internal(
        node: &mut InternalNode<'_, CustomString, Self>,
        mut index: Self::Int,
    ) -> (usize, Self::Int, Position) {
        let mut last_cache = 0;
        for (i, child) in node.children().iter().enumerate() {
            last_cache = match child {
                Node::Internal(x) => {
                    if index <= x.cache {
                        return (i, index, get_pos(index, *child));
                    }
                    x.cache
                }
                Node::Leaf(x) => {
                    if index <= x.cache {
                        return (i, index, get_pos(index, *child));
                    }
                    x.cache
                }
            };

            index -= last_cache;
        }

        if index > 0 {
            dbg!(&node);
            assert_eq!(index, 0);
        }
        (node.children().len() - 1, last_cache, Position::End)
    }

    fn find_pos_leaf(
        node: &mut LeafNode<'_, CustomString, Self>,
        mut index: Self::Int,
    ) -> (usize, usize, Position) {
        for (i, child) in node.children().iter().enumerate() {
            if index < HasLength::len(&**child) {
                return (i, index, get_pos(index, &**child));
            }

            index -= HasLength::len(&**child);
        }

        (
            node.children().len() - 1,
            HasLength::len(&**node.children().last().unwrap()),
            Position::End,
        )
    }

    fn len_leaf(node: &LeafNode<'_, CustomString, Self>) -> usize {
        node.cache
    }

    fn len_internal(node: &InternalNode<'_, CustomString, Self>) -> usize {
        node.cache
    }

    fn check_cache_internal(node: &InternalNode<'_, CustomString, Self>) {
        assert_eq!(node.cache, node.children().iter().map(|x| x.len()).sum());
    }

    fn check_cache_leaf(node: &LeafNode<'_, CustomString, Self>) {
        assert_eq!(node.cache, node.children().iter().map(|x| x.len()).sum());
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

impl From<&str> for CustomString {
    fn from(origin: &str) -> Self {
        CustomString {
            str: Rc::new(RefCell::new(SString::from(origin))),
            slice: 0..origin.len(),
        }
    }
}

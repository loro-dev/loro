use enum_as_inner::EnumAsInner;
use rle::{HasLength, Mergable, Sliceable};
use serde::{Deserialize, Serialize};

use crate::container::{
    list::list_op::{InnerListOp, ListOp},
    map::MapSet,
    tree::tree_op::TreeOp,
};

#[derive(EnumAsInner, Debug, Clone)]
pub enum InnerContent {
    List(InnerListOp),
    Map(MapSet),
    Tree(TreeOp),
}

// Note: It will be encoded into binary format, so the order of its fields should not be changed.
#[derive(EnumAsInner, Debug, PartialEq, Serialize, Deserialize)]
pub enum RawOpContent<'a> {
    Map(MapSet),
    List(ListOp<'a>),
    Tree(TreeOp),
}

impl<'a> Clone for RawOpContent<'a> {
    fn clone(&self) -> Self {
        match self {
            Self::Map(arg0) => Self::Map(arg0.clone()),
            Self::List(arg0) => Self::List(arg0.clone()),
            Self::Tree(arg0) => Self::Tree(*arg0),
        }
    }
}

impl<'a> RawOpContent<'a> {
    pub fn to_static(&self) -> RawOpContent<'static> {
        match self {
            Self::Map(arg0) => RawOpContent::Map(arg0.clone()),
            Self::List(arg0) => match arg0 {
                ListOp::Insert { slice, pos } => RawOpContent::List(ListOp::Insert {
                    slice: slice.to_static(),
                    pos: *pos,
                }),
                ListOp::Delete(x) => RawOpContent::List(ListOp::Delete(*x)),
                ListOp::StyleStart {
                    start,
                    end,
                    key,
                    value,
                    info,
                } => RawOpContent::List(ListOp::StyleStart {
                    start: *start,
                    end: *end,
                    key: key.clone(),
                    value: value.clone(),
                    info: *info,
                }),
                ListOp::StyleEnd => RawOpContent::List(ListOp::StyleEnd),
                ListOp::Move {
                    from,
                    to,
                    elem_id: from_id,
                } => RawOpContent::List(ListOp::Move {
                    from: *from,
                    to: *to,
                    elem_id: *from_id,
                }),
                ListOp::Set { elem_id, value } => RawOpContent::List(ListOp::Set {
                    elem_id: *elem_id,
                    value: value.clone(),
                }),
            },
            Self::Tree(arg0) => RawOpContent::Tree(*arg0),
        }
    }
}

impl<'a> HasLength for RawOpContent<'a> {
    fn content_len(&self) -> usize {
        match self {
            RawOpContent::Map(x) => x.content_len(),
            RawOpContent::List(x) => x.content_len(),
            RawOpContent::Tree(x) => x.content_len(),
        }
    }
}

impl<'a> Mergable for RawOpContent<'a> {
    fn is_mergable(&self, other: &Self, _conf: &()) -> bool
    where
        Self: Sized,
    {
        match (self, other) {
            (RawOpContent::Map(x), RawOpContent::Map(y)) => x.is_mergable(y, &()),
            (RawOpContent::List(x), RawOpContent::List(y)) => x.is_mergable(y, &()),
            _ => false,
        }
    }

    fn merge(&mut self, _other: &Self, _conf: &())
    where
        Self: Sized,
    {
        match self {
            RawOpContent::Map(x) => match _other {
                RawOpContent::Map(y) => x.merge(y, &()),
                _ => unreachable!(),
            },
            RawOpContent::List(x) => match _other {
                RawOpContent::List(y) => x.merge(y, &()),
                _ => unreachable!(),
            },
            RawOpContent::Tree(x) => match _other {
                RawOpContent::Tree(y) => x.merge(y, &()),
                _ => unreachable!(),
            },
        }
    }
}

impl HasLength for InnerContent {
    fn content_len(&self) -> usize {
        match self {
            InnerContent::List(list) => list.atom_len(),
            InnerContent::Map(_) => 1,
            InnerContent::Tree(_) => 1,
        }
    }
}

impl Sliceable for InnerContent {
    fn slice(&self, from: usize, to: usize) -> Self {
        match self {
            a @ InnerContent::Map(_) => a.clone(),
            InnerContent::List(x) => InnerContent::List(x.slice(from, to)),
            a @ InnerContent::Tree(_) => a.clone(),
        }
    }
}

impl Mergable for InnerContent {
    fn is_mergable(&self, other: &Self, _conf: &()) -> bool
    where
        Self: Sized,
    {
        match (self, other) {
            (InnerContent::List(x), InnerContent::List(y)) => x.is_mergable(y, &()),
            _ => false,
        }
    }

    fn merge(&mut self, _other: &Self, _conf: &())
    where
        Self: Sized,
    {
        match self {
            InnerContent::List(x) => match _other {
                InnerContent::List(y) => x.merge(y, &()),
                _ => unreachable!(),
            },
            InnerContent::Map(_) => unreachable!(),
            InnerContent::Tree(_) => unreachable!(),
        }
    }
}

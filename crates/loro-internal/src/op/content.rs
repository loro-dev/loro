use enum_as_inner::EnumAsInner;
use rle::{HasLength, Mergable, Sliceable};
use serde::{Deserialize, Serialize};

use crate::{
    container::{
        list::list_op::{InnerListOp, ListOp},
        map::MapSet,
        tree::tree_op::TreeOp,
    },
    encoding::OwnedValue,
};

#[derive(EnumAsInner, Debug, Clone)]
pub enum InnerContent {
    List(InnerListOp),
    Map(MapSet),
    Tree(TreeOp),
    // The future content should not use any encoded arena context.
    Future(FutureInnerContent),
}

#[derive(EnumAsInner, Debug, Clone)]
pub enum FutureInnerContent {
    Unknown { prop: i32, value: OwnedValue },
}

// Note: It will be encoded into binary format, so the order of its fields should not be changed.
#[derive(EnumAsInner, Debug, PartialEq, Serialize, Deserialize)]
pub enum RawOpContent<'a> {
    Map(MapSet),
    List(ListOp<'a>),
    Tree(TreeOp),
    #[serde(untagged)]
    Future(FutureRawOpContent),
}

#[derive(EnumAsInner, Debug, PartialEq, Serialize, Deserialize)]
pub enum FutureRawOpContent {
    Unknown { prop: i32, value: OwnedValue },
}

impl<'a> Clone for RawOpContent<'a> {
    fn clone(&self) -> Self {
        match self {
            Self::Map(arg0) => Self::Map(arg0.clone()),
            Self::List(arg0) => Self::List(arg0.clone()),
            Self::Tree(arg0) => Self::Tree(*arg0),
            Self::Future(f) => Self::Future(match f {
                FutureRawOpContent::Unknown { prop, value } => FutureRawOpContent::Unknown {
                    prop: *prop,
                    value: value.clone(),
                },
            }),
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
                ListOp::Set { elem_id, value } => {
                    RawOpContent::List(ListOp::Set { elem_id: *elem_id, value: value.clone() })
                }
            },
            Self::Tree(arg0) => RawOpContent::Tree(*arg0),
            Self::Future(f) => RawOpContent::Future(match f {
                FutureRawOpContent::Unknown { prop, value } => FutureRawOpContent::Unknown {
                    prop: *prop,
                    value: value.clone(),
                },
            }),
        }
    }
}

impl<'a> HasLength for RawOpContent<'a> {
    fn content_len(&self) -> usize {
        match self {
            RawOpContent::Map(x) => x.content_len(),
            RawOpContent::List(x) => x.content_len(),
            RawOpContent::Tree(x) => x.content_len(),
            RawOpContent::Future(f) => match f {
                FutureRawOpContent::Unknown { .. } => 1,
            },
        }
    }
}

impl<'a> Sliceable for RawOpContent<'a> {
    fn slice(&self, from: usize, to: usize) -> Self {
        match self {
            RawOpContent::Map(x) => RawOpContent::Map(x.slice(from, to)),
            RawOpContent::List(x) => RawOpContent::List(x.slice(from, to)),
            RawOpContent::Tree(x) => RawOpContent::Tree(x.slice(from, to)),
            RawOpContent::Future(f) => match f {
                FutureRawOpContent::Unknown { .. } => unreachable!(),
            },
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
            RawOpContent::Future(f) => match f {
                FutureRawOpContent::Unknown { .. } => unreachable!(),
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
            InnerContent::Future(f) => match f {
                FutureInnerContent::Unknown { .. } => 1,
            },
        }
    }
}

impl Sliceable for InnerContent {
    fn slice(&self, from: usize, to: usize) -> Self {
        match self {
            a @ InnerContent::Map(_) => a.clone(),
            a @ InnerContent::Tree(_) => a.clone(),
            InnerContent::List(x) => InnerContent::List(x.slice(from, to)),
            InnerContent::Future(f) => match f {
                FutureInnerContent::Unknown { .. } => unreachable!(),
            },
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
            _ => unreachable!(),
        }
    }
}

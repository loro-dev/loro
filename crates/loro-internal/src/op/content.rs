use enum_as_inner::EnumAsInner;
use rle::{HasLength, Mergable, Sliceable};
#[cfg(feature = "wasm")]
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
    #[cfg(feature = "counter")]
    Counter(i64),
    Unknown {
        prop: i32,
        value: OwnedValue,
    },
}

// Note: It will be encoded into binary format, so the order of its fields should not be changed.
#[derive(EnumAsInner, Debug, PartialEq)]
#[cfg_attr(feature = "wasm", derive(Serialize, Deserialize,))]
pub enum RawOpContent<'a> {
    Map(MapSet),
    List(ListOp<'a>),
    Tree(TreeOp),
    #[serde(untagged)]
    Future(FutureRawOpContent),
}

#[derive(EnumAsInner, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum FutureRawOpContent {
    #[cfg(feature = "counter")]
    Counter(i64),
    Unknown {
        prop: i32,
        value: OwnedValue,
    },
}

impl<'a> Clone for RawOpContent<'a> {
    fn clone(&self) -> Self {
        match self {
            Self::Map(arg0) => Self::Map(arg0.clone()),
            Self::List(arg0) => Self::List(arg0.clone()),
            Self::Tree(arg0) => Self::Tree(arg0.clone()),
            Self::Future(f) => Self::Future(match f {
                #[cfg(feature = "counter")]
                FutureRawOpContent::Counter(x) => FutureRawOpContent::Counter(*x),
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
                ListOp::Set { elem_id, value } => RawOpContent::List(ListOp::Set {
                    elem_id: *elem_id,
                    value: value.clone(),
                }),
            },
            Self::Tree(arg0) => RawOpContent::Tree(arg0.clone()),
            Self::Future(f) => RawOpContent::Future(match f {
                #[cfg(feature = "counter")]
                FutureRawOpContent::Counter(x) => FutureRawOpContent::Counter(*x),
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
                #[cfg(feature = "counter")]
                FutureRawOpContent::Counter(_) => 1,
                FutureRawOpContent::Unknown { .. } => 1,
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
            (RawOpContent::List(x), RawOpContent::List(y)) => x.is_mergable(y, &()),
            (RawOpContent::Tree(x), RawOpContent::Tree(y)) => x.is_mergable(y, &()),
            _ => false,
        }
    }

    fn merge(&mut self, _other: &Self, _conf: &())
    where
        Self: Sized,
    {
        match self {
            RawOpContent::List(x) => match _other {
                RawOpContent::List(y) => x.merge(y, &()),
                _ => unreachable!(),
            },
            _ => unreachable!(),
        }
    }
}

impl HasLength for InnerContent {
    fn content_len(&self) -> usize {
        match self {
            InnerContent::List(list) => list.atom_len(),
            InnerContent::Map(_) => 1,
            InnerContent::Tree(_) => 1,
            InnerContent::Future(_) => 1,
        }
    }
}

impl Sliceable for InnerContent {
    fn slice(&self, from: usize, to: usize) -> Self {
        match self {
            a @ InnerContent::Map(_) => a.clone(),
            a @ InnerContent::Tree(_) => a.clone(),
            InnerContent::List(x) => InnerContent::List(x.slice(from, to)),
            InnerContent::Future(f) => InnerContent::Future(f.clone()),
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

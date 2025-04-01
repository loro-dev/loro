use std::sync::Arc;

use enum_as_inner::EnumAsInner;
use loro_common::{ContainerID, ContainerType, LoroValue};
use rle::{HasLength, Mergable, Sliceable};
#[cfg(feature = "wasm")]
use serde::{Deserialize, Serialize};

use crate::{
    arena::SharedArena,
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
    Tree(Arc<TreeOp>),
    // The future content should not use any encoded arena context.
    Future(FutureInnerContent),
}

impl InnerContent {
    pub fn visit_created_children(&self, arena: &SharedArena, f: &mut dyn FnMut(&ContainerID)) {
        match self {
            Self::List(l) => match l {
                InnerListOp::Insert { slice, .. } => {
                    for v in arena.iter_value_slice(slice.to_range()) {
                        if let LoroValue::Container(c) = v {
                            f(&c);
                        }
                    }
                }
                InnerListOp::Set { value, .. } => {
                    if let LoroValue::Container(c) = value {
                        f(c);
                    }
                }

                InnerListOp::Move { .. } => {}
                InnerListOp::InsertText { .. } => {}
                InnerListOp::Delete(_) => {}
                InnerListOp::StyleStart { .. } => {}
                InnerListOp::StyleEnd => {}
            },
            Self::Map(m) => {
                if let Some(LoroValue::Container(c)) = &m.value {
                    f(c);
                }
            }
            Self::Tree(t) => {
                let id = t.target().associated_meta_container();
                f(&id);
            }
            Self::Future(f) => match &f {
                #[cfg(feature = "counter")]
                crate::op::FutureInnerContent::Counter(_) => {}
                crate::op::FutureInnerContent::Unknown { .. } => {}
            },
        }
    }
}

impl InnerContent {
    pub fn estimate_storage_size(&self, kind: ContainerType) -> usize {
        match self {
            Self::List(l) => l.estimate_storage_size(kind),
            Self::Map(_) => 3,
            Self::Tree(_) => 8,
            Self::Future(f) => f.estimate_storage_size(),
        }
    }
}

#[derive(EnumAsInner, Debug, Clone)]
pub enum FutureInnerContent {
    #[cfg(feature = "counter")]
    Counter(f64),
    Unknown {
        prop: i32,
        value: Box<OwnedValue>,
    },
}
impl FutureInnerContent {
    fn estimate_storage_size(&self) -> usize {
        match self {
            #[cfg(feature = "counter")]
            Self::Counter(_) => 4,
            Self::Unknown { .. } => 6,
        }
    }
}

// Note: It will be encoded into binary format, so the order of its fields should not be changed.
#[derive(EnumAsInner, Debug, PartialEq)]
#[cfg_attr(feature = "wasm", derive(Serialize, Deserialize,))]
pub enum RawOpContent<'a> {
    Map(MapSet),
    List(ListOp<'a>),
    Tree(Arc<TreeOp>),
    #[cfg(feature = "counter")]
    Counter(f64),
    Unknown {
        prop: i32,
        value: OwnedValue,
    },
}

impl Clone for RawOpContent<'_> {
    fn clone(&self) -> Self {
        match self {
            Self::Map(arg0) => Self::Map(arg0.clone()),
            Self::List(arg0) => Self::List(arg0.clone()),
            Self::Tree(arg0) => Self::Tree(arg0.clone()),
            #[cfg(feature = "counter")]
            Self::Counter(x) => Self::Counter(*x),
            Self::Unknown { prop, value } => Self::Unknown {
                prop: *prop,
                value: value.clone(),
            },
        }
    }
}

impl RawOpContent<'_> {
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
            #[cfg(feature = "counter")]
            Self::Counter(x) => RawOpContent::Counter(*x),
            Self::Unknown { prop, value } => RawOpContent::Unknown {
                prop: *prop,
                value: value.clone(),
            },
        }
    }
}

impl HasLength for RawOpContent<'_> {
    fn content_len(&self) -> usize {
        match self {
            RawOpContent::Map(x) => x.content_len(),
            RawOpContent::List(x) => x.content_len(),
            RawOpContent::Tree(x) => x.content_len(),
            #[cfg(feature = "counter")]
            RawOpContent::Counter(_) => 1,
            RawOpContent::Unknown { .. } => 1,
        }
    }
}

impl Mergable for RawOpContent<'_> {
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
            Self::List(list) => list.atom_len(),
            Self::Map(_) => 1,
            Self::Tree(_) => 1,
            Self::Future(_) => 1,
        }
    }
}

impl Sliceable for InnerContent {
    fn slice(&self, from: usize, to: usize) -> Self {
        match self {
            a @ Self::Map(_) => {
                assert!(from == 0 && to == 1);
                a.clone()
            }
            a @ Self::Tree(_) => {
                assert!(from == 0 && to == 1);
                a.clone()
            }
            Self::List(x) => Self::List(x.slice(from, to)),
            Self::Future(f) => {
                assert!(from == 0 && to == 1);
                Self::Future(f.clone())
            }
        }
    }
}

impl Mergable for InnerContent {
    fn is_mergable(&self, other: &Self, _conf: &()) -> bool
    where
        Self: Sized,
    {
        match (self, other) {
            (Self::List(x), Self::List(y)) => x.is_mergable(y, &()),
            _ => false,
        }
    }

    fn merge(&mut self, _other: &Self, _conf: &())
    where
        Self: Sized,
    {
        match self {
            Self::List(x) => match _other {
                Self::List(y) => x.merge(y, &()),
                _ => unreachable!(),
            },
            _ => unreachable!(),
        }
    }
}

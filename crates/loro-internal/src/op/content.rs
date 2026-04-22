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
            InnerContent::List(l) => match l {
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
            crate::op::InnerContent::Map(m) => {
                if let Some(LoroValue::Container(c)) = &m.value {
                    f(c);
                }
            }
            crate::op::InnerContent::Tree(t) => {
                let id = t.target().associated_meta_container();
                f(&id);
            }
            crate::op::InnerContent::Future(f) => match &f {
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
            InnerContent::List(l) => l.estimate_storage_size(kind),
            InnerContent::Map(_) => 3,
            InnerContent::Tree(_) => 8,
            InnerContent::Future(f) => f.estimate_storage_size(),
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
            FutureInnerContent::Counter(_) => 4,
            FutureInnerContent::Unknown { .. } => 6,
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
            a @ InnerContent::Map(_) => {
                assert!(from == 0 && to == 1);
                a.clone()
            }
            a @ InnerContent::Tree(_) => {
                assert!(from == 0 && to == 1);
                a.clone()
            }
            InnerContent::List(x) => InnerContent::List(x.slice(from, to)),
            InnerContent::Future(f) => {
                assert!(from == 0 && to == 1);
                InnerContent::Future(f.clone())
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

#[cfg(test)]
mod tests {
    use std::{borrow::Cow, sync::Arc};

    use fractional_index::FractionalIndex;
    use loro_common::{ContainerID, ContainerType, TreeID};
    use rle::{HasLength, Mergable, Sliceable};

    use super::*;
    use crate::{
        container::{
            list::list_op::{DeleteSpanWithId, ListOp},
            map::MapSet,
            tree::tree_op::TreeOp,
        },
        op::{ListSlice, SliceRange},
        InternalString, LoroValue, ID,
    };

    #[test]
    fn raw_op_content_reports_lengths_and_static_content_without_borrowing() {
        let raw_insert = RawOpContent::List(ListOp::Insert {
            slice: ListSlice::RawData(Cow::Owned(vec![1_i64.into(), 2_i64.into()])),
            pos: 3,
        });
        assert_eq!(raw_insert.content_len(), 2);
        match raw_insert.to_static() {
            RawOpContent::List(ListOp::Insert { slice, pos }) => {
                assert_eq!(pos, 3);
                assert_eq!(
                    slice,
                    ListSlice::RawData(Cow::Owned(vec![1_i64.into(), 2_i64.into()]))
                );
            }
            other => panic!("expected list insert, got {other:?}"),
        }

        let raw_map = RawOpContent::Map(MapSet {
            key: InternalString::from("title"),
            value: Some("draft".into()),
        });
        assert_eq!(raw_map.content_len(), 1);
        assert_eq!(raw_map.to_static(), raw_map);

        let raw_tree = RawOpContent::Tree(Arc::new(TreeOp::Delete {
            target: TreeID::new(2, 9),
        }));
        assert_eq!(raw_tree.content_len(), 1);
        assert_eq!(raw_tree.to_static(), raw_tree);

        let raw_unknown = RawOpContent::Unknown {
            prop: 7,
            value: OwnedValue::I64(11),
        };
        assert_eq!(raw_unknown.content_len(), 1);
        assert_eq!(raw_unknown.to_static(), raw_unknown);

        #[cfg(feature = "counter")]
        {
            let raw_counter = RawOpContent::Counter(1.25);
            assert_eq!(raw_counter.content_len(), 1);
            assert_eq!(raw_counter.to_static(), raw_counter);
        }
    }

    #[test]
    fn raw_op_content_merges_only_mergeable_list_operations() {
        let mut left =
            RawOpContent::List(ListOp::Delete(DeleteSpanWithId::new(ID::new(1, 0), 0, 1)));
        let right = RawOpContent::List(ListOp::Delete(DeleteSpanWithId::new(ID::new(1, 1), 0, 1)));
        assert!(left.is_mergable(&right, &()));
        left.merge(&right, &());
        assert_eq!(left.content_len(), 2);

        let map = RawOpContent::Map(MapSet {
            key: InternalString::from("k"),
            value: Some(1_i64.into()),
        });
        assert!(!left.is_mergable(&map, &()));
    }

    #[test]
    fn inner_content_lengths_storage_estimates_slices_and_merges_follow_inner_ops() {
        let mut left = InnerContent::List(InnerListOp::Insert {
            slice: SliceRange(0..2),
            pos: 0,
        });
        let right = InnerContent::List(InnerListOp::Insert {
            slice: SliceRange(2..4),
            pos: 2,
        });
        assert_eq!(left.content_len(), 2);
        assert_eq!(left.estimate_storage_size(ContainerType::List), 8);
        assert!(left.is_mergable(&right, &()));
        left.merge(&right, &());
        assert_eq!(left.content_len(), 4);

        let sliced = left.slice(1, 3);
        match sliced {
            InnerContent::List(InnerListOp::Insert { slice, pos }) => {
                assert_eq!(slice, SliceRange(1..3));
                assert_eq!(pos, 1);
            }
            other => panic!("expected sliced list insert, got {other:?}"),
        }

        let map = InnerContent::Map(MapSet {
            key: InternalString::from("flag"),
            value: Some(true.into()),
        });
        assert_eq!(map.content_len(), 1);
        assert_eq!(map.estimate_storage_size(ContainerType::Map), 3);
        assert!(matches!(map.slice(0, 1), InnerContent::Map(_)));

        let tree = InnerContent::Tree(Arc::new(TreeOp::Delete {
            target: TreeID::new(4, 5),
        }));
        assert_eq!(tree.content_len(), 1);
        assert_eq!(tree.estimate_storage_size(ContainerType::Tree), 8);
        assert!(matches!(tree.slice(0, 1), InnerContent::Tree(_)));

        let future = InnerContent::Future(FutureInnerContent::Unknown {
            prop: 99,
            value: Box::new(OwnedValue::Null),
        });
        assert_eq!(future.content_len(), 1);
        assert_eq!(future.estimate_storage_size(ContainerType::Unknown(3)), 6);
        assert!(matches!(future.slice(0, 1), InnerContent::Future(_)));

        #[cfg(feature = "counter")]
        {
            let counter = InnerContent::Future(FutureInnerContent::Counter(2.0));
            assert_eq!(counter.estimate_storage_size(ContainerType::Counter), 4);
        }
    }

    #[test]
    fn visit_created_children_reports_containers_created_by_list_map_and_tree_content() {
        let arena = SharedArena::new();
        let list_child = ContainerID::new_normal(ID::new(1, 1), ContainerType::List);
        let text_child = ContainerID::new_normal(ID::new(1, 2), ContainerType::Text);
        let values = arena.alloc_values(
            vec![
                LoroValue::Container(list_child.clone()),
                "plain".into(),
                LoroValue::Container(text_child.clone()),
            ]
            .into_iter(),
        );
        let list = InnerContent::List(InnerListOp::Insert {
            slice: SliceRange(values.start as u32..values.end as u32),
            pos: 0,
        });
        let mut children = Vec::new();
        list.visit_created_children(&arena, &mut |id| children.push(id.clone()));
        assert_eq!(children, vec![list_child.clone(), text_child.clone()]);

        let map_child = ContainerID::new_normal(ID::new(2, 1), ContainerType::Map);
        let map = InnerContent::Map(MapSet {
            key: InternalString::from("child"),
            value: Some(LoroValue::Container(map_child.clone())),
        });
        let mut children = Vec::new();
        map.visit_created_children(&arena, &mut |id| children.push(id.clone()));
        assert_eq!(children, vec![map_child]);

        let tree_target = TreeID::new(3, 7);
        let tree = InnerContent::Tree(Arc::new(TreeOp::Create {
            target: tree_target,
            parent: None,
            position: FractionalIndex::default(),
        }));
        let mut children = Vec::new();
        tree.visit_created_children(&arena, &mut |id| children.push(id.clone()));
        assert_eq!(children, vec![tree_target.associated_meta_container()]);

        let future = InnerContent::Future(FutureInnerContent::Unknown {
            prop: 1,
            value: Box::new(OwnedValue::False),
        });
        let mut children = Vec::new();
        future.visit_created_children(&arena, &mut |id| children.push(id.clone()));
        assert!(children.is_empty());
    }
}

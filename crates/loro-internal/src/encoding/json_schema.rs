// mod op;

use std::{borrow::Cow, sync::Arc};

use self::op::{JsonSchema, OpContent};

use loro_common::{
    ContainerID, ContainerType, IdLp, LoroError, LoroResult, LoroValue, PeerID, TreeID, ID,
};
use rle::{HasLength, Sliceable};

use crate::{
    arena::SharedArena,
    change::Change,
    container::{
        list::list_op::{DeleteSpan, DeleteSpanWithId, InnerListOp},
        map::MapSet,
        richtext::TextStyleInfoFlag,
        tree::tree_op::TreeOp,
    },
    op::{InnerContent, Op, SliceRange},
    version::Frontiers,
    OpLog, VersionVector,
};

use super::encode_reordered::{import_changes_to_oplog, ValueRegister};

pub(crate) fn export_json<'a, 'c: 'a>(oplog: &'c OpLog, vv: &VersionVector) -> String {
    let actual_start_vv: VersionVector = vv
        .iter()
        .filter_map(|(&peer, &end_counter)| {
            if end_counter == 0 {
                return None;
            }

            let this_end = oplog.vv().get(&peer).cloned().unwrap_or(0);
            if this_end <= end_counter {
                return Some((peer, this_end));
            }

            Some((peer, end_counter))
        })
        .collect();

    let mut peer_register = ValueRegister::<PeerID>::new();
    let diff_changes = init_encode(oplog, &actual_start_vv);
    let changes = encode_changes(&diff_changes, &oplog.arena, &mut peer_register);
    serde_json::to_string_pretty(&JsonSchema {
        changes,
        schema_version: 1,
        peers: peer_register.unwrap_vec(),
        // TODO:
        start_vv: actual_start_vv,
        end_vv: oplog.vv().clone(),
    })
    .unwrap()
}

pub(crate) fn import_json(oplog: &mut OpLog, json: &str) -> LoroResult<()> {
    let json: JsonSchema = serde_json::from_str(json)
        .map_err(|e| LoroError::DecodeError(format!("cannot decode json {}", e).into()))?;
    let changes = decode_changes(json, &oplog.arena);
    let (latest_ids, pending_changes) = import_changes_to_oplog(changes, oplog)?;
    if oplog.try_apply_pending(latest_ids).should_update && !oplog.batch_importing {
        oplog.dag.refresh_frontiers();
    }
    oplog.import_unknown_lamport_pending_changes(pending_changes)?;
    Ok(())
}

fn init_encode<'a>(oplog: &'a OpLog, vv: &'_ VersionVector) -> Vec<Cow<'a, Change>> {
    let self_vv = oplog.vv();
    let start_vv = vv.trim(oplog.vv());
    let mut diff_changes: Vec<Cow<'a, Change>> = Vec::new();
    for change in oplog.iter_changes_peer_by_peer(&start_vv, self_vv) {
        let start_cnt = start_vv.get(&change.id.peer).copied().unwrap_or(0);
        if change.id.counter < start_cnt {
            let offset = start_cnt - change.id.counter;
            diff_changes.push(Cow::Owned(change.slice(offset as usize, change.atom_len())));
        } else {
            diff_changes.push(Cow::Borrowed(change));
        }
    }
    diff_changes.sort_by_key(|x| x.lamport);
    diff_changes
}

fn register_id(id: &ID, peer_register: &mut ValueRegister<PeerID>) -> ID {
    let peer = peer_register.register(&id.peer);
    ID::new(peer as PeerID, id.counter)
}

fn register_idlp(idlp: &IdLp, peer_register: &mut ValueRegister<PeerID>) -> IdLp {
    IdLp {
        peer: peer_register.register(&idlp.peer) as PeerID,
        lamport: idlp.lamport,
    }
}

fn register_tree_id(tree: &TreeID, peer_register: &mut ValueRegister<PeerID>) -> TreeID {
    TreeID {
        peer: peer_register.register(&tree.peer) as PeerID,
        counter: tree.counter,
    }
}

fn register_container_id(
    container: ContainerID,
    peer_register: &mut ValueRegister<PeerID>,
) -> ContainerID {
    match container {
        ContainerID::Normal {
            peer,
            counter,
            container_type,
        } => ContainerID::Normal {
            peer: peer_register.register(&peer) as PeerID,
            counter,
            container_type,
        },
        r => r,
    }
}

fn convert_container_id(container: ContainerID, peers: &[PeerID]) -> ContainerID {
    match container {
        ContainerID::Normal {
            peer,
            counter,
            container_type,
        } => ContainerID::Normal {
            peer: peers[peer as usize],
            counter,
            container_type,
        },
        r => r,
    }
}

fn convert_id(id: &ID, peers: &[PeerID]) -> ID {
    ID {
        peer: peers[id.peer as usize],
        counter: id.counter,
    }
}

fn convert_idlp(idlp: &IdLp, peers: &[PeerID]) -> IdLp {
    IdLp {
        lamport: idlp.lamport,
        peer: peers[idlp.peer as usize],
    }
}

fn convert_tree_id(tree: &TreeID, peers: &[PeerID]) -> TreeID {
    TreeID {
        peer: peers[tree.peer as usize],
        counter: tree.counter,
    }
}

fn encode_changes(
    diff_changes: &[Cow<'_, Change>],
    arena: &SharedArena,
    peer_register: &mut ValueRegister<PeerID>,
) -> Vec<op::Change> {
    let mut changes = Vec::with_capacity(diff_changes.len());
    for change in diff_changes.iter() {
        let mut ops = Vec::with_capacity(change.ops().len());
        for Op {
            counter,
            container,
            content,
        } in change.ops().iter()
        {
            let mut container = arena.get_container_id(*container).unwrap();
            if container.is_normal() {
                container = register_container_id(container, peer_register);
            }
            let op = match container.container_type() {
                ContainerType::List => match content {
                    InnerContent::List(list) => OpContent::List(match list {
                        InnerListOp::Insert { slice, pos } => {
                            let mut value =
                                arena.get_values(slice.0.start as usize..slice.0.end as usize);
                            value.iter_mut().for_each(|x| {
                                if let LoroValue::Container(id) = x {
                                    if id.is_normal() {
                                        *id = register_container_id(id.clone(), peer_register);
                                    }
                                }
                            });
                            op::ListOp::Insert {
                                pos: *pos,
                                value: value.into(),
                            }
                        }
                        InnerListOp::Delete(DeleteSpanWithId {
                            id_start,
                            span: DeleteSpan { pos, signed_len },
                        }) => op::ListOp::Delete {
                            pos: *pos,
                            len: *signed_len,
                            delete_start_id: register_id(id_start, peer_register),
                        },
                        _ => unreachable!(),
                    }),
                    _ => unreachable!(),
                },
                ContainerType::MovableList => match content {
                    InnerContent::List(list) => OpContent::MovableList(match list {
                        InnerListOp::Insert { slice, pos } => {
                            let mut value =
                                arena.get_values(slice.0.start as usize..slice.0.end as usize);
                            value.iter_mut().for_each(|x| {
                                if let LoroValue::Container(id) = x {
                                    if id.is_normal() {
                                        *id = register_container_id(id.clone(), peer_register);
                                    }
                                }
                            });
                            op::MovableListOp::Insert {
                                pos: *pos,
                                value: value.into(),
                            }
                        }
                        InnerListOp::Delete(DeleteSpanWithId {
                            id_start,
                            span: DeleteSpan { pos, signed_len },
                        }) => op::MovableListOp::Delete {
                            pos: *pos,
                            len: *signed_len,
                            delete_start_id: register_id(id_start, peer_register),
                        },
                        InnerListOp::Move { from, from_id, to } => op::MovableListOp::Move {
                            from: *from,
                            to: *to,
                            from_id: register_idlp(from_id, peer_register),
                        },
                        InnerListOp::Set { elem_id, value } => {
                            let value = if let LoroValue::Container(id) = value {
                                if id.is_normal() {
                                    LoroValue::Container(register_container_id(
                                        id.clone(),
                                        peer_register,
                                    ))
                                } else {
                                    value.clone()
                                }
                            } else {
                                value.clone()
                            };
                            op::MovableListOp::Set {
                                elem_id: register_idlp(elem_id, peer_register),
                                value,
                            }
                        }
                        _ => unreachable!(),
                    }),
                    _ => unreachable!(),
                },
                ContainerType::Text => match content {
                    InnerContent::List(list) => OpContent::Text(match list {
                        InnerListOp::InsertText {
                            slice,
                            unicode_start: _,
                            unicode_len: _,
                            pos,
                        } => {
                            let text = String::from_utf8(slice.as_bytes().to_vec()).unwrap();
                            op::TextOp::Insert { pos: *pos, text }
                        }
                        InnerListOp::Delete(DeleteSpanWithId {
                            id_start,
                            span: DeleteSpan { pos, signed_len },
                        }) => op::TextOp::Delete {
                            pos: *pos,
                            len: *signed_len,
                            id_start: register_id(id_start, peer_register),
                        },
                        InnerListOp::StyleStart {
                            start,
                            end,
                            key,
                            value,
                            info,
                        } => op::TextOp::Mark {
                            start: *start,
                            end: *end,
                            style: (key.to_string(), value.clone()),
                            info: info.to_byte(),
                        },
                        InnerListOp::StyleEnd => op::TextOp::MarkEnd,
                        _ => unreachable!(),
                    }),
                    _ => unreachable!(),
                },
                ContainerType::Map => match content {
                    InnerContent::Map(MapSet { key, value }) => {
                        OpContent::Map(if let Some(v) = value {
                            let value = if let LoroValue::Container(id) = v {
                                if id.is_normal() {
                                    LoroValue::Container(register_container_id(
                                        id.clone(),
                                        peer_register,
                                    ))
                                } else {
                                    v.clone()
                                }
                            } else {
                                v.clone()
                            };
                            op::MapOp::Insert {
                                key: key.to_string(),
                                value,
                            }
                        } else {
                            op::MapOp::Delete {
                                key: key.to_string(),
                            }
                        })
                    }

                    _ => unreachable!(),
                },

                ContainerType::Tree => match content {
                    // TODO: how to determine the type of the tree op?
                    InnerContent::Tree(TreeOp { target, parent }) => OpContent::Tree({
                        if let Some(p) = parent {
                            if TreeID::is_deleted_root(p) {
                                op::TreeOp::Delete {
                                    target: register_tree_id(target, peer_register),
                                }
                            } else {
                                op::TreeOp::Move {
                                    target: register_tree_id(target, peer_register),
                                    parent: Some(register_tree_id(p, peer_register)),
                                }
                            }
                        } else {
                            op::TreeOp::Move {
                                target: register_tree_id(target, peer_register),
                                parent: None,
                            }
                        }
                    }),
                    _ => unreachable!(),
                },
            };
            ops.push(op::Op {
                counter: *counter,
                container,
                content: op,
            });
        }
        let c = op::Change {
            id: register_id(&change.id, peer_register),
            ops,
            deps: change
                .deps
                .iter()
                .map(|id| register_id(id, peer_register))
                .collect(),
            lamport: change.lamport,
            timestamp: change.timestamp,
            msg: None,
        };
        changes.push(c);
    }
    changes
}

fn decode_changes(json: JsonSchema, arena: &SharedArena) -> Vec<Change> {
    let JsonSchema { peers, changes, .. } = json;
    let mut ans = Vec::with_capacity(changes.len());
    for op::Change {
        id,
        timestamp,
        deps,
        lamport,
        msg: _,
        ops,
    } in changes
    {
        let id = convert_id(&id, &peers);
        let ops = ops
            .into_iter()
            .map(|op| decode_op(op, arena, &peers))
            .collect();
        let change = Change {
            id,
            timestamp,
            deps: Frontiers::from_iter(deps.into_iter().map(|id| convert_id(&id, &peers))),
            lamport,
            ops,
            has_dependents: false,
        };
        ans.push(change);
    }
    ans
}

fn decode_op(op: op::Op, arena: &SharedArena, peers: &[PeerID]) -> Op {
    let op::Op {
        counter,
        container,
        content,
    } = op;
    let container = convert_container_id(container, peers);
    let idx = arena.register_container(&container);
    let content = match container.container_type() {
        ContainerType::Text => match content {
            OpContent::Text(text) => match text {
                op::TextOp::Insert { pos, text } => {
                    let (slice, result) = arena.alloc_str_with_slice(&text);
                    InnerContent::List(InnerListOp::InsertText {
                        slice,
                        unicode_start: result.start as u32,
                        unicode_len: (result.end - result.start) as u32,
                        pos,
                    })
                }
                op::TextOp::Delete { pos, len, id_start } => {
                    let id_start = convert_id(&id_start, peers);
                    InnerContent::List(InnerListOp::Delete(DeleteSpanWithId {
                        id_start,
                        span: DeleteSpan {
                            pos,
                            signed_len: len,
                        },
                    }))
                }
                op::TextOp::Mark {
                    start,
                    end,
                    style: (key, value),
                    info,
                } => InnerContent::List(InnerListOp::StyleStart {
                    start,
                    end,
                    key: key.into(),
                    value,
                    info: TextStyleInfoFlag::from_byte(info),
                }),
                op::TextOp::MarkEnd => InnerContent::List(InnerListOp::StyleEnd),
            },
            _ => unreachable!(),
        },
        ContainerType::List => match content {
            OpContent::List(list) => match list {
                op::ListOp::Insert { pos, value } => {
                    let mut values = value.into_list().unwrap();
                    Arc::make_mut(&mut values).iter_mut().for_each(|v| {
                        if let LoroValue::Container(id) = v {
                            if id.is_normal() {
                                *id = convert_container_id(id.clone(), peers);
                            }
                        }
                    });
                    let range = arena.alloc_values(values.iter().cloned());
                    InnerContent::List(InnerListOp::Insert {
                        slice: SliceRange::new(range.start as u32..range.end as u32),
                        pos,
                    })
                }
                op::ListOp::Delete {
                    pos,
                    len,
                    delete_start_id,
                } => InnerContent::List(InnerListOp::Delete(DeleteSpanWithId {
                    id_start: convert_id(&delete_start_id, peers),
                    span: DeleteSpan {
                        pos,
                        signed_len: len,
                    },
                })),
            },
            _ => unreachable!(),
        },
        ContainerType::MovableList => match content {
            OpContent::MovableList(list) => match list {
                op::MovableListOp::Insert { pos, value } => {
                    let mut values = value.into_list().unwrap();
                    Arc::make_mut(&mut values).iter_mut().for_each(|v| {
                        if let LoroValue::Container(id) = v {
                            if id.is_normal() {
                                *id = convert_container_id(id.clone(), peers);
                            }
                        }
                    });
                    let range = arena.alloc_values(values.iter().cloned());
                    InnerContent::List(InnerListOp::Insert {
                        slice: SliceRange::new(range.start as u32..range.end as u32),
                        pos,
                    })
                }
                op::MovableListOp::Delete {
                    pos,
                    len,
                    delete_start_id,
                } => InnerContent::List(InnerListOp::Delete(DeleteSpanWithId {
                    id_start: convert_id(&delete_start_id, peers),
                    span: DeleteSpan {
                        pos,
                        signed_len: len,
                    },
                })),
                op::MovableListOp::Move { from, from_id, to } => {
                    let from_id = convert_idlp(&from_id, peers);
                    InnerContent::List(InnerListOp::Move { from, from_id, to })
                }
                op::MovableListOp::Set { elem_id, mut value } => {
                    let elem_id = convert_idlp(&elem_id, peers);
                    if let LoroValue::Container(id) = &mut value {
                        *id = convert_container_id(id.clone(), peers);
                    }
                    InnerContent::List(InnerListOp::Set { elem_id, value })
                }
            },
            _ => unreachable!(),
        },
        ContainerType::Map => match content {
            OpContent::Map(map) => match map {
                op::MapOp::Insert { key, mut value } => {
                    if let LoroValue::Container(id) = &mut value {
                        *id = convert_container_id(id.clone(), peers);
                    }
                    InnerContent::Map(MapSet {
                        key: key.into(),
                        value: Some(value),
                    })
                }
                op::MapOp::Delete { key } => InnerContent::Map(MapSet {
                    key: key.into(),
                    value: None,
                }),
            },
            _ => unreachable!(),
        },
        ContainerType::Tree => match content {
            OpContent::Tree(tree) => match tree {
                op::TreeOp::Move { target, parent } => InnerContent::Tree(TreeOp {
                    target: convert_tree_id(&target, peers),
                    parent: parent.map(|p| convert_tree_id(&p, peers)),
                }),
                op::TreeOp::Delete { target } => InnerContent::Tree(TreeOp {
                    target: convert_tree_id(&target, peers),
                    parent: Some(TreeID::delete_root()),
                }),
            },
            _ => unreachable!(),
        },
    };
    Op {
        counter,
        container: idx,
        content,
    }
}

pub mod op {
    use loro_common::{ContainerID, IdLp, Lamport, LoroValue, PeerID, TreeID, ID};
    use serde::{Deserialize, Serialize};
    use smallvec::SmallVec;

    use crate::VersionVector;

    #[derive(Debug, Serialize, Deserialize)]
    pub struct JsonSchema {
        pub schema_version: u8,
        pub start_vv: VersionVector,
        pub end_vv: VersionVector,
        pub peers: Vec<PeerID>,
        pub changes: Vec<Change>,
    }
    #[derive(Debug, Serialize, Deserialize)]
    pub struct Change {
        #[serde(with = "self::serde_impl::id")]
        pub id: ID,
        pub timestamp: i64,
        pub deps: SmallVec<[ID; 2]>,
        pub lamport: Lamport,
        pub msg: Option<String>,
        pub ops: Vec<Op>,
    }

    #[derive(Debug)]
    pub struct Op {
        pub content: OpContent,
        pub container: ContainerID,
        pub counter: i32,
    }

    #[derive(Debug, Serialize, Deserialize)]
    #[serde(untagged)]
    pub enum OpContent {
        List(ListOp),
        MovableList(MovableListOp),
        Map(MapOp),
        Text(TextOp),
        Tree(TreeOp),
    }

    #[derive(Debug, Serialize, Deserialize)]
    #[serde(tag = "type", rename_all = "snake_case")]
    pub enum ListOp {
        Insert {
            pos: usize,
            value: LoroValue,
        },
        Delete {
            pos: isize,
            len: isize,
            #[serde(with = "self::serde_impl::id")]
            delete_start_id: ID,
        },
    }

    #[derive(Debug, Serialize, Deserialize)]
    #[serde(tag = "type", rename_all = "snake_case")]
    pub enum MovableListOp {
        Insert {
            pos: usize,
            value: LoroValue,
        },
        Delete {
            pos: isize,
            len: isize,
            #[serde(with = "self::serde_impl::id")]
            delete_start_id: ID,
        },
        Move {
            from: u32,
            to: u32,
            #[serde(with = "self::serde_impl::idlp")]
            from_id: IdLp,
        },
        Set {
            #[serde(with = "self::serde_impl::idlp")]
            elem_id: IdLp,
            value: LoroValue,
        },
    }

    #[derive(Debug, Serialize, Deserialize)]
    #[serde(tag = "type", rename_all = "snake_case")]
    pub enum MapOp {
        Insert { key: String, value: LoroValue },
        Delete { key: String },
    }

    #[derive(Debug, Serialize, Deserialize)]
    #[serde(tag = "type", rename_all = "snake_case")]
    pub enum TextOp {
        Insert {
            pos: u32,
            text: String,
        },
        Delete {
            pos: isize,
            len: isize,
            #[serde(with = "self::serde_impl::id")]
            id_start: ID,
        },
        Mark {
            start: u32,
            end: u32,
            style: (String, LoroValue),
            info: u8,
        },
        MarkEnd,
    }

    #[derive(Debug, Serialize, Deserialize)]
    #[serde(tag = "type", rename_all = "snake_case")]
    pub enum TreeOp {
        // Create {
        //     target: TreeID,
        //     parent: Option<TreeID>,
        //     fractional_index: String,
        // },
        Move {
            target: TreeID,
            parent: Option<TreeID>,
        },
        Delete {
            target: TreeID,
        },
    }

    mod serde_impl {

        use loro_common::{ContainerID, ContainerType};
        use serde::{
            de::{MapAccess, Visitor},
            ser::SerializeStruct,
            Deserialize, Deserializer, Serialize, Serializer,
        };

        impl Serialize for super::Op {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: Serializer,
            {
                let mut s = serializer.serialize_struct("Op", 3)?;
                s.serialize_field("container", &self.container.to_string())?;
                s.serialize_field("content", &self.content)?;
                s.serialize_field("counter", &self.counter)?;
                s.end()
            }
        }

        impl<'de> Deserialize<'de> for super::Op {
            fn deserialize<D>(deserializer: D) -> Result<super::Op, D::Error>
            where
                D: Deserializer<'de>,
            {
                struct __Visitor<'a> {
                    marker: std::marker::PhantomData<&'a ()>,
                }

                impl<'a, 'de> Visitor<'de> for __Visitor<'a> {
                    type Value = super::Op;
                    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                        formatter.write_str("struct Op")
                    }

                    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
                    where
                        A: MapAccess<'de>,
                    {
                        let (_key, container) = map.next_entry::<&str, &str>()?.unwrap();
                        let container = ContainerID::try_from(container)
                            .map_err(|_| serde::de::Error::custom("invalid container id"))?;
                        let op = match container.container_type() {
                            ContainerType::List => {
                                let (_key, op) = map.next_entry::<&str, super::ListOp>()?.unwrap();
                                super::OpContent::List(op)
                            }
                            ContainerType::MovableList => {
                                let (_key, op) =
                                    map.next_entry::<&str, super::MovableListOp>()?.unwrap();
                                super::OpContent::MovableList(op)
                            }
                            ContainerType::Map => {
                                let (_key, op) = map.next_entry::<&str, super::MapOp>()?.unwrap();
                                super::OpContent::Map(op)
                            }
                            ContainerType::Text => {
                                let (_key, op) = map.next_entry::<&str, super::TextOp>()?.unwrap();
                                super::OpContent::Text(op)
                            }
                            ContainerType::Tree => {
                                let (_key, op) = map.next_entry::<&str, super::TreeOp>()?.unwrap();
                                super::OpContent::Tree(op)
                            }
                        };
                        let (_, counter) = map.next_entry::<&str, i32>()?.unwrap();
                        Ok(super::Op {
                            container,
                            content: op,
                            counter,
                        })
                    }
                }
                const FIELDS: &[&str] = &["content", "container", "counter"];
                deserializer.deserialize_struct(
                    "Op",
                    FIELDS,
                    __Visitor {
                        marker: Default::default(),
                    },
                )
            }
        }

        pub mod id {
            use loro_common::ID;
            use serde::{Deserialize, Deserializer, Serializer};

            pub fn serialize<S>(id: &ID, s: S) -> Result<S::Ok, S::Error>
            where
                S: Serializer,
            {
                s.serialize_str(&id.to_string())
            }

            pub fn deserialize<'de, 'a, D>(d: D) -> Result<ID, D::Error>
            where
                D: Deserializer<'de>,
            {
                let str: &str = Deserialize::deserialize(d)?;
                let id: ID = ID::try_from(str).unwrap();
                Ok(id)
            }
        }

        pub mod idlp {
            use loro_common::IdLp;
            use serde::{Deserialize, Deserializer, Serializer};

            pub fn serialize<S>(idlp: &IdLp, s: S) -> Result<S::Ok, S::Error>
            where
                S: Serializer,
            {
                s.serialize_str(&idlp.to_string())
            }

            pub fn deserialize<'de, 'a, D>(d: D) -> Result<IdLp, D::Error>
            where
                D: Deserializer<'de>,
            {
                let str: &str = Deserialize::deserialize(d)?;
                let id: IdLp = IdLp::try_from(str).unwrap();
                Ok(id)
            }
        }
    }
}

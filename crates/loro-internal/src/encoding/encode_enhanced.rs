// allow impl in zerovec macro
#![allow(clippy::incorrect_partial_ord_impl_on_ord_type)]
use fxhash::{FxHashMap, FxHashSet};
use loro_common::{HasCounterSpan, HasIdSpan, HasLamportSpan, TreeID};
use rle::{HasLength, RleVec, Sliceable};
use serde_columnar::{columnar, iter_from_bytes, to_vec};
use std::{borrow::Cow, ops::Deref, sync::Arc};
use zerovec::{vecs::Index32, VarZeroVec};

use crate::{
    change::{Change, Timestamp},
    container::{
        idx::ContainerIdx,
        list::list_op::{DeleteSpan, ListOp},
        map::MapSet,
        richtext::TextStyleInfoFlag,
        tree::tree_op::TreeOp,
        ContainerID, ContainerType,
    },
    id::{Counter, PeerID, ID},
    op::{ListSlice, RawOpContent, RemoteOp},
    oplog::OpLog,
    span::HasId,
    version::Frontiers,
    InternalString, LoroError, LoroValue, VersionVector,
};

type PeerIdx = u32;

#[zerovec::make_varule(RootContainerULE)]
#[zerovec::derive(Serialize, Deserialize)]
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
struct RootContainer<'a> {
    #[serde(borrow)]
    name: Cow<'a, str>,
    type_: ContainerType,
}

#[columnar(vec, ser, de, iterable)]
#[derive(Debug, Clone)]
struct NormalContainer {
    #[columnar(strategy = "DeltaRle")]
    peer_idx: PeerIdx,
    #[columnar(strategy = "DeltaRle")]
    counter: Counter,
    #[columnar(strategy = "Rle")]
    type_: u8,
}

#[columnar(vec, ser, de, iterable)]
#[derive(Debug, Clone)]
struct ChangeEncoding {
    #[columnar(strategy = "Rle")]
    pub(super) peer_idx: PeerIdx,
    #[columnar(strategy = "DeltaRle")]
    pub(super) timestamp: Timestamp,
    #[columnar(strategy = "DeltaRle")]
    pub(super) op_len: u32,
    /// The length of deps that exclude the dep on the same client
    #[columnar(strategy = "Rle")]
    pub(super) deps_len: u32,
    /// Whether the change has a dep on the same client.
    /// It can save lots of space by using this field instead of [`DepsEncoding`]
    #[columnar(strategy = "BoolRle")]
    pub(super) dep_on_self: bool,
}

#[columnar(vec, ser, de, iterable)]
#[derive(Debug, Clone)]
struct OpEncoding {
    #[columnar(strategy = "DeltaRle")]
    container: usize,
    /// Key index or insert/delete pos or target tree id index
    #[columnar(strategy = "DeltaRle")]
    prop: usize,
    /// 0: insert or the parent tree id is not none
    /// 1: delete or the parent tree id is none
    /// 2: text-anchor-start
    /// 3: text-anchor-end
    #[columnar(strategy = "Rle")]
    kind: u8,
    /// the length of the deletion or insertion or target tree id index
    #[columnar(strategy = "Rle")]
    insert_del_len: isize,
}

#[derive(PartialEq, Eq)]
enum Kind {
    Insert,
    Delete,
    TextAnchorStart,
    TextAnchorEnd,
}

impl Kind {
    fn from_byte(byte: u8) -> Self {
        match byte {
            0 => Self::Insert,
            1 => Self::Delete,
            2 => Self::TextAnchorStart,
            3 => Self::TextAnchorEnd,
            _ => panic!("invalid kind byte"),
        }
    }

    fn to_byte(&self) -> u8 {
        match self {
            Self::Insert => 0,
            Self::Delete => 1,
            Self::TextAnchorStart => 2,
            Self::TextAnchorEnd => 3,
        }
    }
}

#[columnar(vec, ser, de, iterable)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub(super) struct DepsEncoding {
    #[columnar(strategy = "DeltaRle")]
    pub(super) client_idx: PeerIdx,
    #[columnar(strategy = "DeltaRle")]
    pub(super) counter: Counter,
}

type TreeIDEncoding = DepsEncoding;

impl DepsEncoding {
    pub(super) fn new(client_idx: PeerIdx, counter: Counter) -> Self {
        Self {
            client_idx,
            counter,
        }
    }
}

#[columnar(ser, de)]
struct DocEncoding<'a> {
    #[columnar(class = "vec", iter = "ChangeEncoding")]
    changes: Vec<ChangeEncoding>,
    #[columnar(class = "vec", iter = "OpEncoding")]
    ops: Vec<OpEncoding>,
    #[columnar(class = "vec", iter = "DepsEncoding")]
    deps: Vec<DepsEncoding>,
    #[columnar(class = "vec")]
    normal_containers: Vec<NormalContainer>,
    #[columnar(borrow)]
    str: Cow<'a, str>,
    #[columnar(borrow)]
    style_info: Cow<'a, [u8]>,
    style_key: Vec<usize>,
    style_values: Vec<LoroValue>,
    #[columnar(borrow)]
    root_containers: VarZeroVec<'a, RootContainerULE, Index32>,
    start_counter: Vec<Counter>,
    values: Vec<Option<LoroValue>>,
    clients: Vec<PeerID>,
    keys: Vec<InternalString>,
    // the index 0 is DELETE_ROOT
    tree_ids: Vec<TreeIDEncoding>,
}

pub fn encode_oplog_v2(oplog: &OpLog, vv: &VersionVector) -> Vec<u8> {
    let mut peer_id_to_idx: FxHashMap<PeerID, PeerIdx> = FxHashMap::default();
    let mut peers = Vec::with_capacity(oplog.changes().len());
    let mut diff_changes = Vec::new();
    let self_vv = oplog.vv();
    let start_vv = vv.trim(&oplog.vv());
    let diff = self_vv.diff(&start_vv);

    let mut start_counter = Vec::new();

    for span in diff.left.iter() {
        let id = span.id_start();
        let changes = oplog.get_change_at(id).unwrap();
        let peer_id = *span.0;
        let idx = peers.len() as PeerIdx;
        peers.push(peer_id);
        peer_id_to_idx.insert(peer_id, idx);
        start_counter.push(
            changes
                .id
                .counter
                .max(start_vv.get(&peer_id).copied().unwrap_or(0)),
        );
    }

    for (change, _) in oplog.iter_causally(start_vv.clone(), self_vv.clone()) {
        let start_cnt = start_vv.get(&change.id.peer).copied().unwrap_or(0);
        if change.id.counter < start_cnt {
            let offset = start_cnt - change.id.counter;
            diff_changes.push(Cow::Owned(change.slice(offset as usize, change.atom_len())));
        } else {
            diff_changes.push(Cow::Borrowed(change));
        }
    }

    let (root_containers, container_idx2index, normal_containers) =
        extract_containers(&diff_changes, oplog, &mut peer_id_to_idx, &mut peers);

    for change in &diff_changes {
        for deps in change.deps.iter() {
            peer_id_to_idx.entry(deps.peer).or_insert_with(|| {
                let idx = peers.len() as PeerIdx;
                peers.push(deps.peer);
                idx
            });
        }
    }

    let change_num = diff_changes.len();
    let mut changes = Vec::with_capacity(change_num);
    let mut ops = Vec::with_capacity(change_num);
    let mut keys = Vec::new();
    let mut key_to_idx = FxHashMap::default();
    let mut deps = Vec::with_capacity(change_num);
    let mut values = Vec::new();
    // the index 0 is DELETE_ROOT
    let mut tree_ids = Vec::new();
    let mut tree_id_to_idx = FxHashMap::default();
    let mut string: String = String::new();
    let mut style_key_idx = Vec::new();
    let mut style_values = Vec::new();
    let mut style_info = Vec::new();

    for change in &diff_changes {
        let client_idx = peer_id_to_idx[&change.id.peer];
        let mut dep_on_self = false;
        let mut deps_len = 0;
        for dep in change.deps.iter() {
            if change.id.peer != dep.peer {
                deps.push(DepsEncoding::new(
                    *peer_id_to_idx.get(&dep.peer).unwrap(),
                    dep.counter,
                ));
                deps_len += 1;
            } else {
                dep_on_self = true;
            }
        }

        let mut op_len = 0;
        for op in change.ops.iter() {
            let container = op.container;
            let container_index = *container_idx2index.get(&container).unwrap();
            let remote_ops = oplog.local_op_to_remote(op);
            for op in remote_ops {
                let content = op.content;
                let (prop, kind, insert_del_len) = match content {
                    crate::op::RawOpContent::Tree(TreeOp { target, parent }) => {
                        // TODO: refactor extract register idx
                        let target_peer_idx =
                            *peer_id_to_idx.entry(target.peer).or_insert_with(|| {
                                let idx = peers.len() as PeerIdx;
                                peers.push(target.peer);
                                idx
                            });
                        let target_encoding = TreeIDEncoding {
                            client_idx: target_peer_idx,
                            counter: target.counter,
                        };
                        let target_idx =
                            *tree_id_to_idx.entry(target_encoding).or_insert_with(|| {
                                tree_ids.push(target_encoding);
                                // the index 0 is DELETE_ROOT
                                tree_ids.len()
                            });
                        let (is_none, parent_idx) = if let Some(parent) = parent {
                            if TreeID::is_deleted_root(Some(parent)) {
                                (Kind::Insert, 0)
                            } else {
                                let parent_peer_idx =
                                    *peer_id_to_idx.entry(parent.peer).or_insert_with(|| {
                                        let idx = peers.len() as PeerIdx;
                                        peers.push(parent.peer);
                                        idx
                                    });
                                let parent_encoding = TreeIDEncoding {
                                    client_idx: parent_peer_idx,
                                    counter: parent.counter,
                                };
                                let parent_idx =
                                    *tree_id_to_idx.entry(parent_encoding).or_insert_with(|| {
                                        tree_ids.push(parent_encoding);
                                        tree_ids.len()
                                    });
                                (Kind::Insert, parent_idx)
                            }
                        } else {
                            (Kind::Delete, 0)
                        };
                        (target_idx, is_none, parent_idx as isize)
                    }
                    crate::op::RawOpContent::Map(MapSet { key, value }) => {
                        if value.is_some() {
                            values.push(value.clone());
                        }
                        (
                            *key_to_idx.entry(key.clone()).or_insert_with(|| {
                                keys.push(key.clone());
                                keys.len() - 1
                            }),
                            if value.is_some() {
                                Kind::Insert
                            } else {
                                Kind::Delete
                            },
                            0,
                        )
                    }
                    crate::op::RawOpContent::List(list) => match list {
                        ListOp::Insert { slice, pos } => {
                            let len;
                            match &slice {
                                ListSlice::RawData(v) => {
                                    len = 0;
                                    values.push(Some(LoroValue::List(Arc::new(v.to_vec()))));
                                }
                                ListSlice::RawStr {
                                    str,
                                    unicode_len: _,
                                } => {
                                    len = str.len();
                                    assert!(len > 0, "{:?}", &slice);
                                    string.push_str(str.deref());
                                }
                            };
                            (pos, Kind::Insert, len as isize)
                        }
                        ListOp::Delete(span) => {
                            // span.len maybe negative
                            (span.pos as usize, Kind::Delete, span.signed_len)
                        }
                        ListOp::StyleStart {
                            start,
                            end,
                            key,
                            info,
                            value,
                        } => {
                            let key_idx = *key_to_idx.entry(key.clone()).or_insert_with(|| {
                                keys.push(key.clone());
                                keys.len() - 1
                            });
                            style_key_idx.push(key_idx);
                            style_info.push(info.to_byte());
                            style_values.push(value);
                            (
                                start as usize,
                                Kind::TextAnchorStart,
                                end as isize - start as isize,
                            )
                        }
                        ListOp::StyleEnd => (0, Kind::TextAnchorEnd, 0),
                    },
                };
                op_len += 1;
                ops.push(OpEncoding {
                    prop,
                    kind: kind.to_byte(),
                    insert_del_len,
                    container: container_index,
                })
            }
        }
        changes.push(ChangeEncoding {
            peer_idx: client_idx as PeerIdx,
            timestamp: change.timestamp,
            deps_len,
            op_len,
            dep_on_self,
        });
    }

    let encoded = DocEncoding {
        changes,
        ops,
        deps,
        str: Cow::Owned(string),
        clients: peers,
        keys,
        start_counter,
        root_containers: VarZeroVec::from(&root_containers),
        normal_containers,
        values,
        style_key: style_key_idx,
        style_values,
        style_info: Cow::Owned(style_info),
        tree_ids,
    };

    to_vec(&encoded).unwrap()
}

/// Extract containers from oplog changes.
///
/// Containers are sorted by their peer_id and counter so that
/// they can be compressed by using delta encoding.
fn extract_containers(
    diff_changes: &Vec<Cow<Change>>,
    oplog: &OpLog,
    peer_id_to_idx: &mut FxHashMap<PeerID, PeerIdx>,
    peers: &mut Vec<PeerID>,
) -> (
    Vec<RootContainer<'static>>,
    FxHashMap<ContainerIdx, usize>,
    Vec<NormalContainer>,
) {
    let mut root_containers = Vec::new();
    let mut container_idx2index = FxHashMap::default();
    let normal_containers = {
        // register containers in sorted order
        let mut visited = FxHashSet::default();
        let mut normal_container_idx_pairs = Vec::new();
        for change in diff_changes {
            for op in change.ops.iter() {
                let container = op.container;
                if visited.contains(&container) {
                    continue;
                }

                visited.insert(container);
                let id = oplog.arena.get_container_id(container).unwrap();
                match id {
                    ContainerID::Root {
                        name,
                        container_type,
                    } => {
                        container_idx2index.insert(container, root_containers.len());
                        root_containers.push(RootContainer {
                            name: Cow::Owned(name.to_string()),
                            type_: container_type,
                        });
                    }
                    ContainerID::Normal {
                        peer,
                        counter,
                        container_type,
                    } => normal_container_idx_pairs.push((
                        NormalContainer {
                            peer_idx: *peer_id_to_idx.entry(peer).or_insert_with(|| {
                                peers.push(peer);
                                (peers.len() - 1) as PeerIdx
                            }),
                            counter,
                            type_: container_type.to_u8(),
                        },
                        container,
                    )),
                }
            }
        }

        normal_container_idx_pairs.sort_by(|a, b| {
            if a.0.peer_idx != b.0.peer_idx {
                a.0.peer_idx.cmp(&b.0.peer_idx)
            } else {
                a.0.counter.cmp(&b.0.counter)
            }
        });

        let mut index = root_containers.len();
        normal_container_idx_pairs
            .into_iter()
            .map(|(container, idx)| {
                container_idx2index.insert(idx, index);
                index += 1;
                container
            })
            .collect::<Vec<_>>()
    };

    (root_containers, container_idx2index, normal_containers)
}

pub fn decode_oplog_v2(oplog: &mut OpLog, input: &[u8]) -> Result<(), LoroError> {
    let encoded = iter_from_bytes::<DocEncoding>(input)
        .map_err(|e| LoroError::DecodeError(e.to_string().into()))?;

    let DocEncodingIter {
        changes: change_encodings,
        ops,
        deps,
        normal_containers,
        mut start_counter,
        str,
        clients: peers,
        keys,
        root_containers,
        values,
        style_key,
        style_values,
        style_info,
        tree_ids,
    } = encoded;

    debug_log::debug_dbg!(&start_counter);
    let mut op_iter = ops;
    let mut deps_iter = deps;
    let mut style_key_iter = style_key.into_iter();
    let mut style_value_iter = style_values.into_iter();
    let mut style_info_iter = style_info.iter();
    let get_container = |idx: usize| {
        if idx < root_containers.len() {
            let Some(container) = root_containers.get(idx) else {
                return None;
            };
            Some(ContainerID::Root {
                name: container.name.into(),
                container_type: ContainerType::from_u8(container.type_),
            })
        } else {
            let Some(container) = normal_containers.get(idx - root_containers.len()) else {
                return None;
            };
            Some(ContainerID::Normal {
                peer: peers[container.peer_idx as usize],
                counter: container.counter,
                container_type: ContainerType::from_u8(container.type_),
            })
        }
    };

    let mut value_iter = values.into_iter();
    let mut str_index = 0;
    let changes = change_encodings
        .map(|change_encoding| {
            let counter = start_counter
                .get_mut(change_encoding.peer_idx as usize)
                .unwrap();
            let ChangeEncoding {
                peer_idx,
                timestamp,
                op_len,
                deps_len,
                dep_on_self,
            } = change_encoding;

            let peer_id = peers[peer_idx as usize];
            let mut ops = RleVec::<[RemoteOp; 1]>::new();
            let mut delta = 0;
            for op in op_iter.by_ref().take(op_len as usize) {
                let OpEncoding {
                    container: container_idx,
                    prop,
                    insert_del_len,
                    kind,
                } = op;

                let Some(container_id) = get_container(container_idx) else {
                    return Err(LoroError::DecodeError("".into()));
                };
                let container_type = container_id.container_type();
                let content = match container_type {
                    ContainerType::Tree => {
                        let target_encoding = tree_ids[prop - 1];
                        let target = TreeID {
                            peer: peers[target_encoding.client_idx as usize],
                            counter: target_encoding.counter,
                        };
                        let parent = if kind == 1 {
                            None
                        } else if insert_del_len == 0 {
                            TreeID::delete_root()
                        } else {
                            let parent_encoding = tree_ids[insert_del_len as usize - 1];
                            let parent = TreeID {
                                peer: peers[parent_encoding.client_idx as usize],
                                counter: parent_encoding.counter,
                            };
                            Some(parent)
                        };
                        RawOpContent::Tree(TreeOp { target, parent })
                    }
                    ContainerType::Map => {
                        let key = keys[prop].clone();
                        if Kind::from_byte(kind) == Kind::Delete {
                            RawOpContent::Map(MapSet { key, value: None })
                        } else {
                            RawOpContent::Map(MapSet {
                                key,
                                value: value_iter.next().unwrap(),
                            })
                        }
                    }
                    ContainerType::List | ContainerType::Text => {
                        let pos = prop;
                        match Kind::from_byte(kind) {
                            Kind::Insert => match container_type {
                                ContainerType::Text => {
                                    let insert_len = insert_del_len as usize;
                                    let s = &str[str_index..str_index + insert_len];
                                    str_index += insert_len;
                                    RawOpContent::List(ListOp::Insert {
                                        slice: ListSlice::from_borrowed_str(s),
                                        pos,
                                    })
                                }
                                ContainerType::List => {
                                    let value = value_iter.next().flatten().unwrap();
                                    RawOpContent::List(ListOp::Insert {
                                        slice: ListSlice::RawData(Cow::Owned(
                                            match Arc::try_unwrap(value.into_list().unwrap()) {
                                                Ok(v) => v,
                                                Err(v) => v.deref().clone(),
                                            },
                                        )),
                                        pos,
                                    })
                                }
                                _ => unreachable!(),
                            },
                            Kind::Delete => RawOpContent::List(ListOp::Delete(DeleteSpan {
                                pos: pos as isize,
                                signed_len: insert_del_len,
                            })),
                            Kind::TextAnchorStart => RawOpContent::List(ListOp::StyleStart {
                                start: pos as u32,
                                end: insert_del_len as u32 + pos as u32,
                                key: keys[style_key_iter.next().unwrap()].clone(),
                                value: style_value_iter.next().unwrap(),
                                info: TextStyleInfoFlag::from_byte(
                                    *style_info_iter.next().unwrap(),
                                ),
                            }),
                            Kind::TextAnchorEnd => RawOpContent::List(ListOp::StyleEnd),
                        }
                    }
                };
                let remote_op = RemoteOp {
                    container: container_id,
                    counter: *counter + delta,
                    content,
                };
                delta += remote_op.content_len() as i32;
                ops.push(remote_op);
            }

            let mut deps: Frontiers = (0..deps_len)
                .map(|_| {
                    let raw = deps_iter.next().unwrap();
                    ID::new(peers[raw.client_idx as usize], raw.counter)
                })
                .collect();
            if dep_on_self && *counter > 0 {
                deps.push(ID::new(peer_id, *counter - 1));
            }

            let change = Change {
                id: ID {
                    peer: peer_id,
                    counter: *counter,
                },
                // calc lamport after parsing all changes
                lamport: 0,
                has_dependents: false,
                timestamp,
                ops,
                deps,
            };

            *counter += delta;
            Ok(change)
        })
        .collect::<Result<Vec<_>, LoroError>>();
    let changes = match changes {
        Ok(changes) => changes,
        Err(err) => return Err(err),
    };
    let mut pending_remote_changes = Vec::new();
    debug_log::debug_dbg!(&changes);
    let mut latest_ids = Vec::new();
    oplog.arena.clone().with_op_converter(|converter| {
        'outer: for mut change in changes {
            if change.ctr_end() <= oplog.vv().get(&change.id.peer).copied().unwrap_or(0) {
                // skip included changes
                continue;
            }

            latest_ids.push(change.id_last());
            // calc lamport or pending if its deps are not satisfied
            for dep in change.deps.iter() {
                match oplog.dag.get_lamport(dep) {
                    Some(lamport) => {
                        change.lamport = change.lamport.max(lamport + 1);
                    }
                    None => {
                        pending_remote_changes.push(change);
                        continue 'outer;
                    }
                }
            }

            // convert change into inner format
            let mut ops = RleVec::new();
            for op in change.ops {
                let lamport = change.lamport;
                let content = op.content;
                let op = converter.convert_single_op(
                    &op.container,
                    change.id.peer,
                    op.counter,
                    lamport,
                    content,
                );
                ops.push(op);
            }

            let change = Change {
                ops,
                id: change.id,
                deps: change.deps,
                lamport: change.lamport,
                timestamp: change.timestamp,
                has_dependents: false,
            };

            let Some(change) = oplog.trim_the_known_part_of_change(change) else {
                continue;
            };
            // update dag and push the change
            let mark = oplog.insert_dag_node_on_new_change(&change);
            oplog.next_lamport = oplog.next_lamport.max(change.lamport_end());
            oplog.latest_timestamp = oplog.latest_timestamp.max(change.timestamp);
            oplog.dag.vv.extend_to_include_end_id(ID {
                peer: change.id.peer,
                counter: change.id.counter + change.atom_len() as Counter,
            });
            oplog.insert_new_change(change, mark);
        }
    });

    let mut vv = oplog.dag.vv.clone();
    oplog.try_apply_pending(latest_ids, &mut vv);
    if !oplog.batch_importing {
        oplog.dag.refresh_frontiers();
    }

    oplog.import_unknown_lamport_remote_changes(pending_remote_changes)?;
    assert_eq!(str_index, str.len());
    Ok(())
}

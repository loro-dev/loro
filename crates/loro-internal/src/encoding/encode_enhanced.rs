use fxhash::{FxHashMap, FxHashSet};
use loro_common::{HasCounterSpan, HasLamportSpan, TreeID};
use rle::{HasLength, RleVec};
use serde_columnar::{columnar, iter_from_bytes, to_vec};
use std::{borrow::Cow, cmp::Ordering, ops::Deref, sync::Arc};
use zerovec::{vecs::Index32, VarZeroVec};

use crate::{
    change::{Change, Lamport, Timestamp},
    container::text::text_content::ListSlice,
    container::{
        idx::ContainerIdx,
        list::list_op::{DeleteSpan, ListOp},
        map::MapSet,
        tree::tree_op::TreeOp,
        ContainerID, ContainerType,
    },
    id::{Counter, PeerID, ID},
    op::{RawOpContent, RemoteOp},
    oplog::{AppDagNode, OpLog},
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
    /// key index or insert/delete pos or target tree id index
    #[columnar(strategy = "DeltaRle")]
    prop: usize,
    /// is deleted or is none of parent tree id
    #[columnar(strategy = "BoolRle")]
    is_del: bool,
    // the length of the deletion or insertion or target tree id index
    #[columnar(strategy = "Rle")]
    insert_del_len: isize,
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
    let mut peers = Vec::with_capacity(oplog.changes.len());
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
        start_counter.push(changes.id.counter);
    }

    for (change, _) in oplog.iter_causally(start_vv, self_vv.clone()) {
        diff_changes.push(change);
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
            let op = oplog.local_op_to_remote(op);
            for content in op.contents.into_iter() {
                let (prop, is_del, insert_del_len) = match content {
                    crate::op::RawOpContent::Tree(TreeOp { target, parent }) => {
                        let target_peer_idx = *peer_id_to_idx.get(&target.peer).unwrap();
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
                                (false, 0)
                            } else {
                                let parent_peer_idx = *peer_id_to_idx.get(&parent.peer).unwrap();
                                let parent_encoding = TreeIDEncoding {
                                    client_idx: parent_peer_idx,
                                    counter: parent.counter,
                                };
                                let parent_idx =
                                    *tree_id_to_idx.entry(parent_encoding).or_insert_with(|| {
                                        tree_ids.push(parent_encoding);
                                        tree_ids.len()
                                    });
                                (false, parent_idx)
                            }
                        } else {
                            (true, 0)
                        };
                        (target_idx, is_none, parent_idx as isize)
                    }
                    crate::op::RawOpContent::Map(MapSet { key, value }) => {
                        values.push(value.clone());
                        (
                            *key_to_idx.entry(key.clone()).or_insert_with(|| {
                                keys.push(key.clone());
                                keys.len() - 1
                            }),
                            false,
                            0,
                        )
                    }
                    crate::op::RawOpContent::List(list) => match list {
                        ListOp::Insert { slice, pos } => {
                            let mut len = 0;
                            match slice {
                                ListSlice::RawData(v) => {
                                    values.push(Some(LoroValue::List(Arc::new(v.to_vec()))));
                                }
                                ListSlice::RawStr {
                                    str,
                                    unicode_len: _,
                                } => {
                                    len = str.len();
                                    string.push_str(str.deref());
                                }
                            };
                            assert!(len > 0);
                            (pos, false, len as isize)
                        }
                        ListOp::Delete(span) => {
                            // span.len maybe negative
                            (span.pos as usize, true, span.len)
                        }
                    },
                };
                op_len += 1;
                ops.push(OpEncoding {
                    prop,
                    is_del,
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
        tree_ids,
    };

    to_vec(&encoded).unwrap()
}

/// Extract containers from oplog changes.
///
/// Containers are sorted by their peer_id and counter so that
/// they can be compressed by using delta encoding.
fn extract_containers(
    diff_changes: &Vec<&Change>,
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
        tree_ids,
    } = encoded;

    let start_vv: VersionVector = peers
        .iter()
        .copied()
        .zip(start_counter.iter().map(|x| *x as Counter))
        .collect::<FxHashMap<_, _>>()
        .into();
    let ord = start_vv.partial_cmp(oplog.vv());
    if ord.is_none() || ord.unwrap() == Ordering::Greater {
        return Err(LoroError::DecodeError(
            format!(
                "Warning: current Loro version is `{:?}`, but remote changes start at version `{:?}`.
                These updates can not be applied",
                oplog.vv(),
                start_vv
            )
            .into(),
        ));
    }

    let mut op_iter = ops;
    let mut deps_iter = deps;
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
                    is_del,
                    insert_del_len,
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
                        let parent = if is_del {
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
                        RawOpContent::Map(MapSet {
                            key,
                            value: value_iter.next().unwrap(),
                        })
                    }
                    ContainerType::List | ContainerType::Text => {
                        let pos = prop;
                        if is_del {
                            RawOpContent::List(ListOp::Delete(DeleteSpan {
                                pos: pos as isize,
                                len: insert_del_len,
                            }))
                        } else {
                            match container_type {
                                ContainerType::Tree => unreachable!(),
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
                                ContainerType::Map => unreachable!(),
                            }
                        }
                    }
                };
                let remote_op = RemoteOp {
                    container: container_id,
                    counter: *counter + delta,
                    contents: vec![content].into(),
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
            if dep_on_self {
                deps.push(ID::new(peer_id, *counter - 1));
            }

            let change = Change {
                id: ID {
                    peer: peer_id,
                    counter: *counter,
                },
                // calc lamport after parsing all changes
                lamport: 0,
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
    oplog.arena.clone().with_op_converter(|converter| {
        'outer: for mut change in changes {
            if change.id.counter < oplog.vv().get(&change.id.peer).copied().unwrap_or(0) {
                // skip included changes
                continue;
            }

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
                for content in op.contents.into_iter() {
                    let op = converter.convert_single_op(&op.container, op.counter, content);
                    ops.push(op);
                }
            }

            let change = Change {
                ops,
                id: change.id,
                deps: change.deps,
                lamport: change.lamport,
                timestamp: change.timestamp,
            };

            // update dag and push the change
            let len = change.content_len();
            if change.deps.len() == 1 && change.deps[0].peer == change.id.peer {
                // don't need to push new element to dag because it only depends on itself
                let nodes = oplog.dag.map.get_mut(&change.id.peer).unwrap();
                let last = nodes.vec_mut().last_mut().unwrap();
                assert_eq!(last.peer, change.id.peer);
                assert_eq!(last.cnt + last.len as Counter, change.id.counter);
                assert_eq!(last.lamport + last.len as Lamport, change.lamport);
                last.len = change.id.counter as usize + len - last.cnt as usize;
                last.has_succ = false;
            } else {
                let vv = oplog.dag.frontiers_to_im_vv(&change.deps);
                oplog
                    .dag
                    .map
                    .entry(change.id.peer)
                    .or_default()
                    .push(AppDagNode {
                        vv,
                        peer: change.id.peer,
                        cnt: change.id.counter,
                        lamport: change.lamport,
                        deps: change.deps.clone(),
                        has_succ: false,
                        len,
                    });
                for dep in change.deps.iter() {
                    let target = oplog.dag.get_mut(*dep).unwrap();
                    if target.ctr_last() == dep.counter {
                        target.has_succ = true;
                    }
                }
            }
            oplog.next_lamport = oplog.next_lamport.max(change.lamport_end());
            oplog.latest_timestamp = oplog.latest_timestamp.max(change.timestamp);
            oplog.dag.vv.extend_to_include_end_id(ID {
                peer: change.id.peer,
                counter: change.id.counter + change.atom_len() as Counter,
            });
            oplog
                .changes
                .entry(change.id.peer)
                .or_default()
                .push(change);
        }
    });
    if !oplog.batch_importing {
        oplog.dag.refresh_frontiers();
    }

    oplog.import_unknown_lamport_remote_changes(pending_remote_changes)?;
    assert_eq!(str_index, str.len());
    Ok(())
}

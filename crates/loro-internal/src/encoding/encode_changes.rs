use std::{collections::VecDeque, ops::Range, sync::Arc};

use fxhash::FxHashMap;
use itertools::Itertools;
use rle::{HasLength, RleVec};
use serde_columnar::{columnar, iter_from_bytes, iterable::*, to_vec};

use crate::{
    change::{Change, Lamport, Timestamp},
    container::text::text_content::ListSlice,
    container::{
        list::list_op::{DeleteSpan, ListOp},
        map::MapSet,
        ContainerID, ContainerType,
    },
    encoding::RemoteClientChanges,
    id::{Counter, PeerID, ID},
    op::{RawOpContent, RemoteOp},
    oplog::OpLog,
    span::HasId,
    version::Frontiers,
    InternalString, LoroError, LoroValue, VersionVector,
};

type ClientIdx = u32;
type Clients = Vec<PeerID>;
type Containers = Vec<ContainerID>;

#[columnar(vec, ser, de, iterable)]
#[derive(Debug, Clone)]
pub struct ChangeEncoding {
    #[columnar(strategy = "Rle")]
    pub(super) client_idx: ClientIdx,
    #[columnar(strategy = "DeltaRle")]
    pub(super) timestamp: Timestamp,
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
    #[columnar(strategy = "Rle")]
    container: usize,
    /// key index or insert/delete pos
    #[columnar(strategy = "DeltaRle")]
    prop: usize,
    // TODO: can be compressed
    gc: usize,
    value: LoroValue,
}

#[columnar(vec, ser, de, iterable)]
#[derive(Debug, Copy, Clone)]
pub(super) struct DepsEncoding {
    #[columnar(strategy = "Rle")]
    pub(super) client_idx: ClientIdx,
    #[columnar(strategy = "DeltaRle")]
    pub(super) counter: Counter,
}

impl DepsEncoding {
    pub(super) fn new(client_idx: ClientIdx, counter: Counter) -> Self {
        Self {
            client_idx,
            counter,
        }
    }
}

#[columnar(ser, de)]
#[derive(Debug)]
struct DocEncoding {
    #[columnar(class = "vec", iter = "ChangeEncoding")]
    changes: Vec<ChangeEncoding>,
    #[columnar(class = "vec", iter = "OpEncoding")]
    ops: Vec<OpEncoding>,
    #[columnar(class = "vec", iter = "DepsEncoding")]
    deps: Vec<DepsEncoding>,
    clients: Clients,
    containers: Containers,
    keys: Vec<InternalString>,
    start_counter: Vec<Counter>,
}

pub(super) fn encode_oplog_changes(oplog: &OpLog, vv: &VersionVector) -> Vec<u8> {
    let mut client_id_to_idx: FxHashMap<PeerID, ClientIdx> = FxHashMap::default();
    let mut clients = Vec::with_capacity(oplog.changes.len());
    let mut container_indexes = Vec::new();
    let mut container_idx2index = FxHashMap::default();
    let mut container_ids = Vec::new();
    let mut change_num = 0;

    let mut diff_changes = Vec::new();
    let self_vv = oplog.vv();
    let diff = self_vv.diff(vv);

    let mut start_counter = Vec::new();

    for span in diff.left.iter() {
        let changes = oplog.get_change_since(span.id_start());
        change_num += changes.len();
        let client_id = *span.0;
        client_id_to_idx.entry(client_id).or_insert_with(|| {
            let idx = clients.len() as ClientIdx;
            clients.push(client_id);
            idx
        });
        start_counter.push(changes.first().unwrap().id.counter);
        diff_changes.extend(changes);
    }

    for change in &diff_changes {
        for deps in change.deps.iter() {
            client_id_to_idx.entry(deps.peer).or_insert_with(|| {
                let idx = clients.len() as ClientIdx;
                clients.push(deps.peer);
                idx
            });
        }
    }

    let mut changes = Vec::with_capacity(change_num);
    let mut ops = Vec::with_capacity(change_num);
    let mut keys = Vec::new();
    let mut key_to_idx = FxHashMap::default();
    let mut deps = Vec::with_capacity(change_num);

    for change in diff_changes {
        let client_idx = client_id_to_idx[&change.id.peer];
        let mut dep_on_self = false;
        let mut deps_len = 0;
        for dep in change.deps.iter() {
            if change.id.peer != dep.peer {
                deps.push(DepsEncoding::new(
                    *client_id_to_idx.get(&dep.peer).unwrap(),
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
            let container_idx = *container_idx2index.entry(container).or_insert_with(|| {
                container_indexes.push(container);
                container_ids.push(oplog.arena.get_container_id(container).unwrap());
                container_indexes.len() - 1
            });

            let op = oplog.local_op_to_remote(op);
            for content in op.contents.into_iter() {
                let (prop, gc, value) = match content {
                    crate::op::RawOpContent::Map(MapSet { key, value }) => (
                        *key_to_idx.entry(key.clone()).or_insert_with(|| {
                            keys.push(key);
                            keys.len() - 1
                        }),
                        0,
                        value,
                    ),
                    crate::op::RawOpContent::List(list) => match list {
                        ListOp::Insert { slice, pos } => (
                            pos,
                            match &slice {
                                ListSlice::Unknown(v) => *v,
                                _ => 0,
                            },
                            // TODO: perf may be optimized by using borrow type instead
                            match slice {
                                ListSlice::RawData(v) => LoroValue::List(Arc::new(v.to_vec())),
                                ListSlice::RawStr {
                                    str,
                                    unicode_len: _,
                                } => LoroValue::String(Arc::new(str.to_string())),
                                ListSlice::Unknown(_) => LoroValue::Null,
                            },
                        ),
                        ListOp::Delete(span) => {
                            (span.pos as usize, 0, LoroValue::I32(span.len as i32))
                        }
                    },
                };
                op_len += 1;
                ops.push(OpEncoding {
                    container: container_idx,
                    prop,
                    value,
                    gc,
                })
            }
        }

        changes.push(ChangeEncoding {
            client_idx: client_idx as ClientIdx,
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
        clients,
        containers: container_ids,
        keys,
        start_counter,
    };

    to_vec(&encoded).unwrap()
}

pub(crate) fn decode_oplog_changes(oplog: &mut OpLog, input: &[u8]) -> Result<(), LoroError> {
    let changes = decode_changes_to_inner_format_oplog(input, oplog)?;
    oplog.import_remote_changes(changes)?;
    Ok(())
}

pub(super) fn decode_changes_to_inner_format_oplog(
    input: &[u8],
    store: &OpLog,
) -> Result<RemoteClientChanges<'static>, LoroError> {
    let encoded = iter_from_bytes::<DocEncoding>(input)
        .map_err(|e| LoroError::DecodeError(e.to_string().into()))?;

    let DocEncodingIter {
        changes: change_encodings,
        ops,
        deps,
        clients,
        containers,
        keys,
        start_counter,
    } = encoded;

    let mut op_iter = ops;
    let mut changes = FxHashMap::default();
    let mut deps_iter = deps;

    for (client_idx, this_change_encodings) in &change_encodings.group_by(|c| c.client_idx) {
        let mut counter = start_counter[client_idx as usize];
        for change_encoding in this_change_encodings {
            let ChangeEncoding {
                client_idx,
                timestamp,
                op_len,
                deps_len,
                dep_on_self,
            } = change_encoding;

            let client_id = clients[client_idx as usize];
            let mut ops = RleVec::<[RemoteOp; 1]>::new();
            let mut delta = 0;
            for op in op_iter.by_ref().take(op_len as usize) {
                let OpEncoding {
                    container: container_idx,
                    prop,
                    value,
                    gc,
                } = op;
                let container_id = containers[container_idx].clone();

                let container_type = container_id.container_type();
                let content = match container_type {
                    ContainerType::Map => {
                        let key = keys[prop].clone();
                        RawOpContent::Map(MapSet { key, value })
                    }
                    ContainerType::List | ContainerType::Text => {
                        let pos = prop;
                        let list_op = match value {
                            LoroValue::I32(len) => ListOp::Delete(DeleteSpan {
                                pos: pos as isize,
                                len: len as isize,
                            }),
                            LoroValue::Null => ListOp::Insert {
                                pos,
                                slice: ListSlice::Unknown(gc),
                            },
                            _ => {
                                let slice = match value {
                                    LoroValue::String(s) => ListSlice::RawStr {
                                        str: std::borrow::Cow::Owned(s.to_string()),
                                        unicode_len: s.chars().count(),
                                    },
                                    LoroValue::List(v) => {
                                        ListSlice::RawData(std::borrow::Cow::Owned((*v).clone()))
                                    }
                                    _ => unreachable!(),
                                };
                                ListOp::Insert { slice, pos }
                            }
                        };
                        RawOpContent::List(list_op)
                    }
                };
                let remote_op = RemoteOp {
                    container: container_id,
                    counter: counter + delta,
                    contents: vec![content].into(),
                };
                delta += remote_op.content_len() as i32;
                ops.push(remote_op);
            }

            let mut deps: Frontiers = (0..deps_len)
                .map(|_| {
                    let raw = deps_iter.next().unwrap();
                    ID::new(clients[raw.client_idx as usize], raw.counter)
                })
                .collect();
            if dep_on_self {
                deps.push(ID::new(client_id, counter - 1));
            }

            let change = Change {
                id: ID {
                    peer: client_id,
                    counter,
                },
                // calc lamport after parsing all changes
                lamport: 0,
                timestamp,
                ops,
                deps,
            };

            counter += delta;
            changes
                .entry(client_id)
                .or_insert_with(VecDeque::new)
                .push_back(change)
        }
    }

    let start_vv: VersionVector = changes
        .iter()
        .map(|(client, changes)| (*client, changes.iter().map(|c| c.id.counter).min().unwrap()))
        .collect::<FxHashMap<_, _>>()
        .into();
    if &start_vv > store.vv() {
        return Err(LoroError::DecodeError(
            format!(
                "Warning: current Loro version is `{:?}`, but remote changes start at version `{:?}`.
                These updates can not be applied",
                store.vv(),
                start_vv
            )
            .into(),
        ));
    }

    let mut lamport_map = FxHashMap::default();
    let mut changes_ans = FxHashMap::default();
    // calculate lamport
    let mut client_ids: VecDeque<_> = changes.keys().copied().collect();
    let len = client_ids.len();
    let mut loop_time = len;
    while let Some(client_id) = client_ids.pop_front() {
        let this_client_changes = changes.get_mut(&client_id).unwrap();
        while let Some(mut change) = this_client_changes.pop_front() {
            match get_lamport_by_deps_oplog(&change.deps, &lamport_map, Some(store)) {
                Ok(lamport) => {
                    change.lamport = lamport;
                    lamport_map.entry(client_id).or_insert_with(Vec::new).push((
                        change.id.counter..change.id.counter + change.content_len() as Counter,
                        lamport,
                    ));
                    changes_ans
                        .entry(client_id)
                        .or_insert_with(Vec::new)
                        .push(change);
                    loop_time = len;
                }
                Err(_not_found_client) => {
                    this_client_changes.push_front(change);
                    client_ids.push_back(client_id);
                    loop_time -= 1;
                    if loop_time == 0 {
                        unreachable!();
                    }
                    break;
                }
            }
        }
    }
    // TODO: using the one with fewer changes to import
    Ok(changes_ans)
}

pub(crate) fn get_lamport_by_deps_oplog(
    deps: &Frontiers,
    lamport_map: &FxHashMap<PeerID, Vec<(Range<Counter>, Lamport)>>,
    oplog: Option<&OpLog>,
) -> Result<Lamport, PeerID> {
    let mut ans = 0;
    for id in deps.iter() {
        if let Some(c) = oplog.and_then(|x| x.lookup_change(*id)) {
            let offset = id.counter - c.id.counter;
            ans = ans.max(c.lamport + offset as u32 + 1);
        } else if let Some(v) = lamport_map.get(&id.peer) {
            if let Some((lamport, offset)) = get_value_from_range_map(v, id.counter) {
                ans = ans.max(lamport + offset + 1);
            } else {
                return Err(id.peer);
            }
        } else {
            return Err(id.peer);
        }
    }
    Ok(ans)
}

fn get_value_from_range_map(
    v: &[(Range<Counter>, Lamport)],
    key: Counter,
) -> Option<(Lamport, u32)> {
    let index = match v.binary_search_by_key(&key, |(range, _)| range.start) {
        Ok(index) => Some(index),

        // If the requested key is smaller than the smallest range in the slice,
        // we would be computing `0 - 1`, which would underflow an `usize`.
        // We use `checked_sub` to get `None` instead.
        Err(index) => index.checked_sub(1),
    };

    if let Some(index) = index {
        let (ref range, value) = v[index];
        if key < range.end {
            return Some((value, (key - range.start) as u32));
        }
    }
    None
}

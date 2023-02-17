use fxhash::FxHashMap;
use itertools::Itertools;
use rle::{HasLength, RleVec};
use serde::{Deserialize, Serialize};
use serde_columnar::{columnar, from_bytes, to_vec};
use tracing::instrument;

use crate::{
    change::{Change, Lamport, Timestamp},
    container::text::text_content::ListSlice,
    container::{
        list::list_op::{DeleteSpan, ListOp},
        map::MapSet,
        ContainerID, ContainerType,
    },
    dag::Dag,
    event::RawEvent,
    hierarchy::Hierarchy,
    id::{ClientID, Counter, ID},
    log_store::RemoteClientChanges,
    op::{RemoteContent, RemoteOp},
    smstring::SmString,
    span::HasIdSpan,
    InternalString, LogStore, LoroError, LoroValue, VersionVector,
};

type ClientIdx = u32;
type Clients = Vec<ClientID>;
type Containers = Vec<ContainerID>;

#[columnar(vec, ser, de)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct ChangeEncoding {
    #[columnar(strategy = "Rle", original_type = "u32")]
    pub(super) client_idx: ClientIdx,
    #[columnar(strategy = "DeltaRle", original_type = "i32")]
    pub(super) counter: Counter,
    #[columnar(strategy = "DeltaRle", original_type = "u32")]
    pub(super) lamport: Lamport,
    #[columnar(strategy = "DeltaRle", original_type = "i64")]
    pub(super) timestamp: Timestamp,
    pub(super) op_len: u32,
    #[columnar(strategy = "Rle")]
    pub(super) deps_len: u32,
}

#[columnar(vec, ser, de)]
#[derive(Debug, Clone, Serialize, Deserialize)]
struct OpEncoding {
    #[columnar(strategy = "Rle", original_type = "usize")]
    container: usize,
    /// key index or insert/delete pos
    #[columnar(strategy = "DeltaRle")]
    prop: usize,
    // TODO: can be compressed
    gc: usize,
    value: LoroValue,
}

#[columnar(vec, ser, de)]
#[derive(Debug, Copy, Clone, Serialize, Deserialize)]
pub(super) struct DepsEncoding {
    #[columnar(strategy = "Rle", original_type = "u32")]
    pub(super) client_idx: ClientIdx,
    #[columnar(strategy = "DeltaRle", original_type = "i32")]
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
#[derive(Debug, Serialize, Deserialize)]
struct DocEncoding {
    #[columnar(type = "vec")]
    changes: Vec<ChangeEncoding>,
    #[columnar(type = "vec")]
    ops: Vec<OpEncoding>,
    #[columnar(type = "vec")]
    deps: Vec<DepsEncoding>,
    clients: Clients,
    containers: Containers,
    keys: Vec<InternalString>,
    start_counter: Vec<Counter>,
    start_lamport: Vec<Lamport>,
}

#[instrument(skip_all)]
pub(super) fn encode_changes(store: &LogStore, vv: &VersionVector) -> Result<Vec<u8>, LoroError> {
    let mut client_id_to_idx: FxHashMap<ClientID, ClientIdx> = FxHashMap::default();
    let mut clients = Vec::with_capacity(store.changes.len());
    let mut container_indexes = Vec::new();
    let mut container_idx2index = FxHashMap::default();
    let mut container_ids = Vec::new();
    let mut change_num = 0;

    let mut diff_changes = Vec::new();
    let self_vv = store.vv();
    let diff = self_vv.diff(vv);

    let mut start_counter = Vec::new();
    let mut start_lamport = Vec::new();

    for span in diff.left.iter() {
        let changes = store.get_changes_slice(span.id_span());
        change_num += changes.len();
        let client_id = *span.0;
        client_id_to_idx.entry(client_id).or_insert_with(|| {
            let idx = clients.len() as ClientIdx;
            clients.push(client_id);
            idx
        });
        start_counter.push(changes.first().unwrap().id.counter);
        start_lamport.push(changes.first().unwrap().lamport);

        diff_changes.extend(changes);
    }

    for change in &diff_changes {
        for deps in change.deps.iter() {
            client_id_to_idx.entry(deps.client_id).or_insert_with(|| {
                let idx = clients.len() as ClientIdx;
                clients.push(deps.client_id);
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
        let client_idx = client_id_to_idx[&change.id.client_id];
        for dep in change.deps.iter() {
            deps.push(DepsEncoding::new(
                *client_id_to_idx.get(&dep.client_id).unwrap(),
                dep.counter,
            ));
        }

        let mut op_len = 0;
        for op in change.ops.iter() {
            let container = op.container;
            let container_idx = *container_idx2index.entry(container).or_insert_with(|| {
                container_indexes.push(container);
                container_ids.push(store.reg.get_id(container).unwrap().clone());
                container_indexes.len() - 1
            });

            let op = store.to_remote_op(op);
            for content in op.contents.into_iter() {
                let (prop, gc, value) = match content {
                    crate::op::RemoteContent::Map(MapSet { key, value }) => (
                        *key_to_idx.entry(key.clone()).or_insert_with(|| {
                            keys.push(key);
                            keys.len() - 1
                        }),
                        0,
                        value,
                    ),
                    crate::op::RemoteContent::List(list) => match list {
                        ListOp::Insert { slice, pos } => (
                            pos,
                            match &slice {
                                ListSlice::Unknown(v) => *v,
                                _ => 0,
                            },
                            match slice {
                                ListSlice::RawData(v) => v.into(),
                                ListSlice::RawStr(s) => s.as_str().into(),
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
            counter: change.id.counter,
            lamport: change.lamport,
            timestamp: change.timestamp,
            deps_len: change.deps.len() as u32,
            op_len,
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
        start_lamport,
    };

    to_vec(&encoded).map_err(|e| LoroError::DecodeError(e.to_string().into()))
    // postcard::to_allocvec(&encoded).map_err(|e| LoroError::DecodeError(e.to_string().into()))
}

#[instrument(skip_all)]
pub(super) fn decode_changes(
    store: &mut LogStore,
    hierarchy: &mut Hierarchy,
    input: &[u8],
) -> Result<Vec<RawEvent>, LoroError> {
    // TODO: using the one with fewer changes to import
    decode_changes_to_inner_format(input).map(|changes| store.import(hierarchy, changes))
}

pub(super) fn decode_changes_to_inner_format(
    input: &[u8],
) -> Result<RemoteClientChanges, LoroError> {
    let encoded: DocEncoding =
        from_bytes(input).map_err(|e| LoroError::DecodeError(e.to_string().into()))?;

    let DocEncoding {
        changes: change_encodings,
        ops,
        deps,
        clients,
        containers,
        keys,
        start_counter,
        start_lamport,
    } = encoded;

    let mut op_iter = ops.into_iter();
    let mut changes = FxHashMap::default();
    let mut deps_iter = deps.into_iter();

    for (client_idx, this_change_encodings) in
        &change_encodings.into_iter().group_by(|c| c.client_idx)
    {
        let mut counter = start_counter[client_idx as usize];
        let mut lamport = start_lamport[client_idx as usize];
        for change_encoding in this_change_encodings {
            let ChangeEncoding {
                client_idx,
                counter: this_counter,
                lamport: this_lamport,
                timestamp,
                op_len,
                deps_len,
            } = change_encoding;

            let client_id = clients[client_idx as usize];
            let mut ops = RleVec::<[RemoteOp; 2]>::new();
            let deps = (0..deps_len)
                .map(|_| {
                    let raw = deps_iter.next().unwrap();
                    ID::new(clients[raw.client_idx as usize], raw.counter)
                })
                .collect();

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
                        RemoteContent::Map(MapSet { key, value })
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
                                    LoroValue::String(s) => ListSlice::RawStr(SmString::from(&*s)),
                                    LoroValue::List(v) => ListSlice::RawData(*v),
                                    _ => unreachable!(),
                                };
                                ListOp::Insert { slice, pos }
                            }
                        };
                        RemoteContent::List(list_op)
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

            let change = Change {
                id: ID { client_id, counter },
                lamport,
                timestamp,
                ops,
                deps,
            };

            counter += delta;
            lamport += delta as u32;

            changes
                .entry(client_id)
                .or_insert_with(Vec::new)
                .push(change);
        }
    }
    // TODO: using the one with fewer changes to import
    Ok(changes)
}

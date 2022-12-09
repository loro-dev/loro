use fxhash::FxHashMap;
use rle::{HasLength, RleVec, RleVecWithIndex};
use serde::{Deserialize, Serialize};
use serde_columnar::{columnar, compress, decompress, from_bytes, to_vec, CompressConfig};
use tracing::instrument;

use crate::{
    change::{Change, ChangeMergeCfg, Lamport, Timestamp},
    container::{
        list::list_op::{DeleteSpan, ListOp},
        map::MapSet,
        text::text_content::ListSlice,
        ContainerID,
    },
    dag::Dag,
    id::{ClientID, Counter, ID},
    op::{Op, RemoteContent, RemoteOp},
    smstring::SmString,
    span::{HasIdSpan, HasLamportSpan},
    ContainerType, InternalString, LogStore, LoroValue, VersionVector,
};

// mod container;
mod snapshot;

type ClientIdx = u32;
type Clients = Vec<ClientID>;
type Containers = Vec<ContainerID>;

#[columnar(vec, ser, de)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct ChangeEncoding {
    #[columnar(strategy = "DeltaRle", original_type = "u32")]
    client_idx: ClientIdx,
    #[columnar(strategy = "DeltaRle", original_type = "i32")]
    counter: Counter,
    #[columnar(strategy = "DeltaRle", original_type = "u32")]
    lamport: Lamport,
    #[columnar(strategy = "DeltaRle", original_type = "i64")]
    timestamp: Timestamp,
    op_len: u32,
    #[columnar(strategy = "Rle")]
    deps_len: u32,
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
    // #[columnar(compress(level = 0))]
    value: LoroValue,
}

#[columnar(vec, ser, de)]
#[derive(Debug, Copy, Clone, Serialize, Deserialize)]
struct DepsEncoding {
    #[columnar(strategy = "Rle", original_type = "u32")]
    client_idx: ClientIdx,
    #[columnar(strategy = "DeltaRle", original_type = "i32")]
    counter: Counter,
}

impl DepsEncoding {
    fn new(client_idx: ClientIdx, counter: Counter) -> Self {
        Self {
            client_idx,
            counter,
        }
    }
}

#[columnar(ser, de)]
#[derive(Debug, Serialize, Deserialize)]
struct Encoded {
    #[columnar(type = "vec")]
    changes: Vec<ChangeEncoding>,
    #[columnar(type = "vec")]
    ops: Vec<OpEncoding>,
    #[columnar(type = "vec")]
    deps: Vec<DepsEncoding>,
    clients: Clients,
    containers: Containers,
    keys: Vec<InternalString>,
}

#[instrument(skip_all)]
fn encode_changes(store: &LogStore, vv: &VersionVector) -> Encoded {
    let mut client_id_to_idx: FxHashMap<ClientID, ClientIdx> = FxHashMap::default();
    let mut clients = Vec::with_capacity(store.changes.len());
    let mut container_indexes = Vec::new();
    let mut container_ids = Vec::new();
    let mut change_num = 0;

    let mut diff_changes = Vec::new();
    let self_vv = store.vv();
    let diff = self_vv.diff(vv);
    for span in diff.left.iter() {
        let changes = store.get_changes_slice(span.id_span());
        for change in changes.into_iter() {
            diff_changes.push(change);
        }
    }

    for change in &diff_changes {
        let client_id = change.id.client_id;
        client_id_to_idx.entry(client_id).or_insert_with(|| {
            let idx = clients.len() as ClientIdx;
            clients.push(client_id);
            idx
        });
        change_num += 1;
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
            let container_idx = if !container_indexes.contains(&container) {
                container_indexes.push(container);
                container_ids.push(store.reg.get_id(container).unwrap().clone());
                container_indexes.len() - 1
            } else {
                container_indexes
                    .iter()
                    .position(|&x| x == container)
                    .unwrap()
            };
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

    Encoded {
        changes,
        ops,
        deps,
        clients,
        containers: container_ids,
        keys,
    }
}

#[instrument(skip_all)]
fn decode_changes(store: &mut LogStore, encoded: Encoded) {
    let Encoded {
        changes: change_encodings,
        ops,
        deps,
        clients,
        containers,
        keys,
    } = encoded;

    if change_encodings.is_empty() && !containers.is_empty() {
        for container in containers.iter() {
            store.get_or_create_container(container);
        }
    }

    let mut op_iter = ops.into_iter();
    let mut changes = FxHashMap::default();
    let mut deps_iter = deps.into_iter();

    for change_encoding in change_encodings {
        let ChangeEncoding {
            client_idx,
            counter,
            lamport,
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

        let mut op_counter = counter;
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
                counter: op_counter,
                contents: vec![content].into(),
            };
            op_counter += remote_op.content_len() as i32;
            ops.push(remote_op);
        }

        let change = Change {
            id: ID { client_id, counter },
            lamport,
            timestamp,
            ops,
            deps,
        };

        changes
            .entry(client_id)
            .or_insert_with(|| Vec::new())
            .push(change);
    }
    // TODO: using the one with fewer changes to import
    store.import(changes);
}

impl LogStore {
    pub fn encode_changes(&self, vv: &VersionVector, compress_cfg: bool) -> Vec<u8> {
        let encoded = encode_changes(self, vv);
        let mut ans = vec![compress_cfg as u8];
        let buf = if compress_cfg {
            // TODO: columnar compress use read/write mode
            compress(&to_vec(&encoded).unwrap(), &CompressConfig::default()).unwrap()
        } else {
            to_vec(&encoded).unwrap()
        };
        ans.extend(buf);
        ans
    }

    pub fn decode_changes(&mut self, input: &[u8]) {
        let compress_cfg = *input.first().unwrap() > 0;
        let encoded = if compress_cfg {
            from_bytes(&decompress(&input[1..]).unwrap()).unwrap()
        } else {
            from_bytes(&input[1..]).unwrap()
        };
        decode_changes(self, encoded);
    }
}

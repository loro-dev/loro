use std::sync::{Arc, RwLock};

use fxhash::FxHashMap;
use rle::{HasLength, RleVec, RleVecWithIndex};
use serde::{Deserialize, Serialize};
use serde_columnar::{columnar, compress, decompress, from_bytes, to_vec, CompressConfig};

use crate::{
    change::{Change, ChangeMergeCfg, Lamport, Timestamp},
    configure::Configure,
    container::{
        list::list_op::{DeleteSpan, ListOp},
        map::MapSet,
        text::text_content::ListSlice,
        Container, ContainerID,
    },
    dag::remove_included_frontiers,
    id::{ClientID, Counter, ID},
    op::{Op, RemoteContent, RemoteOp},
    smstring::SmString,
    span::{HasIdSpan, HasLamportSpan},
    ContainerType, InternalString, LogStore, LoroValue, VersionVector,
};

type ClientIdx = u32;
type Clients = Vec<ClientID>;
type Containers = Vec<ContainerID>;

#[columnar(vec, ser, de)]
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ChangeEncoding {
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
    #[columnar(strategy = "Rle", original_type = "u32")]
    container: u32,
    /// key index or insert/delete pos
    #[columnar(strategy = "DeltaRle")]
    prop: usize, // 18225 bytes
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

fn encode_changes(store: &LogStore) -> Encoded {
    let mut client_id_to_idx: FxHashMap<ClientID, ClientIdx> = FxHashMap::default();
    let mut clients = Vec::with_capacity(store.changes.len());
    let mut change_num = 0;
    for (key, changes) in store.changes.iter() {
        client_id_to_idx.insert(*key, clients.len() as ClientIdx);
        clients.push(*key);
        change_num += changes.merged_len();
    }

    let (_, containers) = store.reg.export();
    let mut changes = Vec::with_capacity(change_num);
    let mut ops = Vec::with_capacity(change_num);
    let mut keys = Vec::new();
    let mut key_to_idx = FxHashMap::default();
    let mut deps = Vec::with_capacity(change_num);
    for (client_idx, (_, change_vec)) in store.changes.iter().enumerate() {
        for change in change_vec.iter() {
            for dep in change.deps.iter() {
                deps.push(DepsEncoding::new(
                    *client_id_to_idx.get(&dep.client_id).unwrap(),
                    dep.counter,
                ));
            }

            let mut remote_ops: RleVec<[RemoteOp; 1]> = RleVec::with_capacity(change.ops.len());
            let mut containers = Vec::with_capacity(change.ops.len());
            for op in change.ops.iter() {
                containers.push(op.container);
                let op = store.to_remote_op(op);
                remote_ops.push(op);
            }

            let mut op_len = 0;
            for (op, container) in remote_ops.into_iter().zip(containers.into_iter()) {
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
                        crate::op::RemoteContent::Dyn(_) => unreachable!(),
                    };
                    op_len += 1;
                    ops.push(OpEncoding {
                        container: container.to_u32(),
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
    }

    Encoded {
        changes,
        ops,
        deps,
        clients,
        containers,
        keys,
    }
}

fn decode_changes(
    encoded: Encoded,
    client_id: Option<ClientID>,
    cfg: Configure,
) -> Arc<RwLock<LogStore>> {
    let Encoded {
        changes: change_encodings,
        ops,
        deps,
        clients,
        containers,
        keys,
    } = encoded;

    if change_encodings.is_empty() {
        let store = LogStore::new(cfg, None);
        if !containers.is_empty() {
            let mut s = store.write().unwrap();
            for container in containers.iter() {
                s.get_or_create_container(container);
            }
            drop(s);
        }
        return store;
    }

    let mut op_iter = ops.into_iter();
    let mut changes = FxHashMap::default();
    let mut deps_iter = deps.into_iter();
    let log_store = LogStore::new(cfg, client_id);
    let mut store = log_store.write().unwrap();
    for container in containers.iter() {
        store.reg.register(container);
    }

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
        let mut ops = RleVec::<[Op; 2]>::new();
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
            let container_id = containers[container_idx as usize].clone();

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

            // TODO: can make this faster
            let container_idx = store.get_container_idx(&container_id).unwrap();
            let container = store.get_container(&container_id).unwrap();
            let op = Op {
                counter: op_counter,
                container: container_idx,
                content: container.lock().unwrap().to_import(content),
            };

            op_counter += op.content_len() as i32;
            ops.push(op);
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
            .or_insert_with(|| RleVecWithIndex::new_with_conf(ChangeMergeCfg::new()))
            .push(change);
    }

    let vv: VersionVector = changes
        .values()
        .map(|changes| changes.last().unwrap().id_last())
        .collect();

    let mut frontiers = vv.clone();
    for (_, changes) in changes.iter() {
        for change in changes.iter() {
            remove_included_frontiers(&mut frontiers, &change.deps);
        }
    }

    store.latest_lamport = changes
        .values()
        .map(|changes| changes.last().unwrap().lamport_last())
        .max()
        .unwrap();
    store.latest_timestamp = changes
        .values()
        .map(|changes| changes.last().unwrap().timestamp)
        .max()
        .unwrap();

    store.changes = changes;
    store.vv = vv;
    store.frontiers = frontiers.get_frontiers();
    drop(store);
    // FIXME: set all
    log_store
}

impl LogStore {
    pub fn encode_snapshot(&self) -> Vec<u8> {
        let encoded = encode_changes(self);
        compress(&to_vec(&encoded).unwrap(), &CompressConfig::default()).unwrap()
    }

    pub fn decode_snapshot(
        input: &[u8],
        client_id: Option<ClientID>,
        cfg: Configure,
    ) -> Arc<RwLock<Self>> {
        let decompress_bytes = decompress(input).unwrap();
        let encoded = from_bytes(&decompress_bytes).unwrap();
        decode_changes(encoded, client_id, cfg)
    }
}

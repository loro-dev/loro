use std::sync::{Arc, RwLock};

use fxhash::FxHashMap;
use im::HashSet;
use num::ToPrimitive;
use rle::{HasLength, RleVec, RleVecWithIndex};
use serde::{Deserialize, Serialize};
use serde_columnar::{columnar, compress, decompress, from_bytes, to_vec, CompressConfig};

use crate::{
    change::{Change, ChangeMergeCfg, Lamport, Timestamp},
    configure::Configure,
    container::{
        encoding::{merge_2_u32_u64, split_u64_2_u32},
        list::list_op::{DeleteSpan, InnerListOp, ListOp},
        map::{InnerMapSet, MapSet},
        registry::{ContainerIdx, ContainerInstance},
        text::text_content::{ListSlice, SliceRange},
        Container, ContainerID,
    },
    dag::{remove_included_frontiers, Dag},
    id::{ClientID, Counter, ID},
    op::{InnerContent, Op, RemoteOp},
    span::{HasIdSpan, HasLamportSpan},
    ContainerType, InternalString, LogStore, VersionVector,
};

type ClientIdx = u32;
type Clients = Vec<ClientID>;
type Containers = Vec<Vec<u8>>;

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
    op_len: usize,
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
    prop: usize,
    // #[columnar(compress(level = 0))]
    // list range or del len or map value index
    value: u64,
    #[columnar(strategy = "BoolRle")]
    is_del: bool,
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

#[derive(Debug, Serialize, Deserialize)]
struct VVEncoding(FxHashMap<u32, Counter>);

impl VVEncoding {
    fn into_vv(self, clients: &Clients) -> VersionVector {
        let mut vv = FxHashMap::default();
        self.0.into_iter().for_each(|(client_idx, counter)| {
            vv.insert(clients[client_idx as usize], counter);
        });
        vv.into()
    }
}

#[columnar(ser, de)]
#[derive(Debug, Serialize, Deserialize)]
struct Encoded {
    #[columnar(type = "vec")]
    pub(crate) changes: Vec<ChangeEncoding>,
    #[columnar(type = "vec")]
    ops: Vec<OpEncoding>,
    #[columnar(type = "vec")]
    deps: Vec<DepsEncoding>,
    clients: Clients,
    containers: Containers,
    keys: Vec<InternalString>,
    vv: VVEncoding,
}

fn encode_changes(store: &LogStore, start_vv: &VersionVector) -> Encoded {
    let mut client_id_to_idx: FxHashMap<ClientID, ClientIdx> = FxHashMap::default();
    let mut clients = Vec::with_capacity(store.changes.len());
    let mut change_num = 0;

    let mut diff_changes = Vec::new();
    let self_vv = store.vv();
    let diff = self_vv.diff(start_vv);
    for span in diff.left.iter() {
        let changes = store.get_changes_slice(span.id_span());
        for change in changes.into_iter() {
            diff_changes.push(change);
        }
    }

    for change in &diff_changes {
        let client_id = change.id.client_id;
        if !client_id_to_idx.contains_key(&client_id) {
            client_id_to_idx.insert(client_id, clients.len() as ClientIdx);
            clients.push(client_id);
        }
        change_num += 1;
    }

    let mut changes = Vec::with_capacity(change_num);
    let mut ops = Vec::with_capacity(change_num);
    let mut keys = Vec::new();
    let mut key_to_idx = FxHashMap::default();
    let mut deps = Vec::with_capacity(change_num);
    let mut container_indexes = Vec::new();
    let mut container_states = Vec::new();

    for change in diff_changes {
        let client_idx = client_id_to_idx[&change.id.client_id];
        for dep in change.deps.iter() {
            deps.push(DepsEncoding::new(
                *client_id_to_idx.get(&dep.client_id).unwrap(),
                dep.counter,
            ));
        }
        let op_len = change.ops.len();
        for op in change.ops.iter() {
            let mut container_idx = op.container.to_u32();
            if !container_indexes.contains(&container_idx) {
                container_indexes.push(container_idx);
                let container = store.reg.get_by_idx(op.container).unwrap();
                // TODO: delta state
                let state = container.lock().unwrap().export_state();
                container_states.push(state);
                container_idx = container_states.len() as u32 - 1;
            } else {
                container_idx = container_indexes
                    .iter()
                    .position(|&x| x == container_idx)
                    .unwrap() as u32;
            }
            let (prop, value, is_del) = match &op.content {
                InnerContent::List(list_op) => match list_op {
                    InnerListOp::Insert { slice, pos } => {
                        (*pos, merge_2_u32_u64(slice.0.start, slice.0.end), false)
                    }
                    InnerListOp::Delete(span) => (span.pos as usize, span.len as u64, true),
                },
                InnerContent::Map(map_set) => {
                    let InnerMapSet { key, value } = map_set;
                    (
                        *key_to_idx.entry(key.clone()).or_insert_with(|| {
                            keys.push(key.clone());
                            keys.len() - 1
                        }),
                        *value as u64,
                        false,
                    )
                }
            };
            ops.push(OpEncoding {
                container: container_idx,
                prop,
                value,
                is_del,
            });
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

    let vv_encoding = VVEncoding(
        start_vv
            .iter()
            .map(|(client, counter)| (*client_id_to_idx.get(client).unwrap(), *counter))
            .collect(),
    );

    Encoded {
        changes,
        ops,
        deps,
        clients,
        containers: container_states,
        keys,
        vv: vv_encoding,
    }
}

fn decode_changes(store: &mut LogStore, encoded: Encoded) {
    let Encoded {
        changes: change_encodings,
        ops,
        deps,
        clients,
        containers,
        keys,
        vv,
    } = encoded;

    if change_encodings.is_empty() {
        // register
        if !containers.is_empty() {
            for buf in containers.into_iter() {
                let container_ins = ContainerInstance::import_state(buf);
                store.reg.insert(container_ins.id().clone(), container_ins);
            }
        }
        return;
    }

    let vv = vv.into_vv(&clients);
    if vv >= store.vv {
        // all
        let mut op_iter = ops.into_iter();
        let mut changes = FxHashMap::default();
        let mut deps_iter = deps.into_iter();
        let mut container_idx_to_id = FxHashMap::default();
        for (idx, buf) in containers.into_iter().enumerate() {
            // TODO: delta state
            let container_ins = ContainerInstance::import_state(buf);
            container_idx_to_id.insert(idx as u32, container_ins.id().clone());
            store.reg.insert(container_ins.id().clone(), container_ins);
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
                    is_del,
                } = op;
                let container = store.reg.get(&container_idx_to_id[&container_idx]).unwrap();
                let content = match container.lock().unwrap().type_() {
                    ContainerType::Map => {
                        let key = keys[prop].clone();
                        InnerContent::Map(InnerMapSet {
                            key,
                            value: value as u32,
                        })
                    }
                    ContainerType::List | ContainerType::Text => {
                        let list_op = if is_del {
                            InnerListOp::Delete(DeleteSpan {
                                pos: prop as isize,
                                len: value as isize,
                            })
                        } else {
                            let (start, end) = split_u64_2_u32(value);
                            InnerListOp::Insert {
                                slice: (start..end).into(),
                                pos: prop,
                            }
                        };
                        InnerContent::List(list_op)
                    }
                };
                let op = Op {
                    counter: op_counter,
                    container: ContainerIdx::new(container_idx),
                    content,
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
    } else {
        // delta
        let mut op_iter = ops.into_iter();
        let mut changes = FxHashMap::default();
        let mut deps_iter = deps.into_iter();
        let mut container_instances = Vec::with_capacity(containers.len());
        for container in containers.into_iter() {
            let container_ins = ContainerInstance::import_state(container);
            container_instances.push(container_ins);
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
                    is_del,
                } = op;
                let container = &mut container_instances[container_idx as usize];
                let content = match container.type_() {
                    ContainerType::Map => {
                        let key = keys[prop].clone();
                        InnerContent::Map(InnerMapSet {
                            key,
                            value: value as u32,
                        })
                    }
                    ContainerType::List | ContainerType::Text => {
                        let list_op = if is_del {
                            InnerListOp::Delete(DeleteSpan {
                                pos: prop as isize,
                                len: value as isize,
                            })
                        } else {
                            let (start, end) = split_u64_2_u32(value);
                            InnerListOp::Insert {
                                slice: (start..end).into(),
                                pos: prop,
                            }
                        };
                        InnerContent::List(list_op)
                    }
                };
                let content_len = content.content_len();
                let remote_op = Op {
                    counter: op_counter,
                    container: ContainerIdx::new(0),
                    content,
                }
                .convert(container, true);

                op_counter += content_len as i32;
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
                .or_insert_with(|| RleVecWithIndex::new_with_conf(ChangeMergeCfg::new()))
                .push(change);
        }
        store.import(changes);
    }
    // FIXME: set all
}

impl LogStore {
    pub fn encode_snapshot(&self, vv: &VersionVector) -> Vec<u8> {
        let encoded = encode_changes(self, vv);
        to_vec(&encoded).unwrap()
        // compress(&to_vec(&encoded).unwrap(), &CompressConfig::default()).unwrap()
    }

    pub fn decode_snapshot(&mut self, input: &[u8]) {
        // let decompress_bytes = decompress(input).unwrap();
        let encoded = from_bytes(input).unwrap();
        decode_changes(self, encoded);
    }
}

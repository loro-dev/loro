use std::sync::{Arc, Mutex, RwLock};

use fxhash::FxHashMap;
use rle::{HasLength, RleVec, RleVecWithIndex};
use serde::{Deserialize, Serialize};
use serde_columnar::{columnar, compress, decompress, from_bytes, to_vec, CompressConfig};
use smallvec::smallvec;
use tracing::instrument;

use crate::{
    change::{Change, ChangeMergeCfg, Lamport, Timestamp},
    configure::Configure,
    container::{
        encoding::{merge_2_u32_u64, split_u64_2_u32},
        list::list_op::{DeleteSpan, InnerListOp, ListOp, ListOp},
        map::MapSet,
        map::{InnerMapSet, MapSet},
        registry::ContainerIdx,
        registry::ContainerInstance,
        text::text_content::{ListSlice, SliceRange},
        text::text_content::{ListSlice, SliceRange},
        Container, ContainerID,
    },
    dag::remove_included_frontiers,
    id::{ClientID, Counter, ID},
    op::{InnerContent, Op, RemoteContent, RemoteOp},
    smstring::SmString,
    span::{HasIdSpan, HasLamportSpan},
    ContainerType, InternalString, LogStore, LoroValue, VersionVector,
};

use super::ImportContext;

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
    containers: Vec<Vec<u8>>,
    keys: Vec<InternalString>,
}

#[instrument(skip_all)]
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
    let mut container_states = Vec::with_capacity(containers.len());
    for container_id in containers.iter() {
        let container = store.reg.get(container_id).unwrap();
        let state = container.lock().unwrap().export_state();
        container_states.push(state);
    }

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
            let op_len = change.ops.len();
            for op in change.ops.iter() {
                let container_idx = op.container;
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

            // let mut op_len = 0;
            // for (op, container) in remote_ops.into_iter().zip(containers.into_iter()) {
            //     for content in op.contents.into_iter() {
            //         let (prop, gc, value) = match content {
            //             crate::op::RemoteContent::Map(MapSet { key, value }) => (
            //                 *key_to_idx.entry(key.clone()).or_insert_with(|| {
            //                     keys.push(key);
            //                     keys.len() - 1
            //                 }),
            //                 0,
            //                 value,
            //             ),
            //             crate::op::RemoteContent::List(list) => match list {
            //                 ListOp::Insert { slice, pos } => (
            //                     pos,
            //                     match &slice {
            //                         ListSlice::Unknown(v) => *v,
            //                         _ => 0,
            //                     },
            //                     match slice {
            //                         ListSlice::RawData(v) => v.into(),
            //                         ListSlice::RawStr(s) => s.as_str().into(),
            //                         ListSlice::Unknown(_) => LoroValue::Null,
            //                     },
            //                 ),
            //                 ListOp::Delete(span) => {
            //                     (span.pos as usize, 0, LoroValue::I32(span.len as i32))
            //                 }
            //             },
            //             crate::op::RemoteContent::Dyn(_) => unreachable!(),
            //         };
            //         op_len += 1;
            //         ops.push(OpEncoding {
            //             container,
            //             prop,
            //             value,
            //             gc,
            //         })
            //     }
            // }

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
        containers: container_states,
        keys,
    }
}

#[instrument(skip_all)]
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
        // register
        if !containers.is_empty() {
            let mut s = store.write().unwrap();
            for buf in containers.into_iter() {
                let container_ins = ContainerInstance::import_state(buf);
                s.reg.insert(container_ins.id().clone(), container_ins);
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

    for buf in containers.into_iter() {
        let container_ins = ContainerInstance::import_state(buf);
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
            let container = store.reg.get_by_idx(container_idx).unwrap();
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
                container: container_idx,
                content,
            };

            op_counter += op.content_len() as i32;
            ops.push(op);

            // container_map.insert(container_idx, Arc::clone(container));
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

    let container_map = container_map
        .into_iter()
        // SAFETY: ignore lifetime issues here, because it's safe for us to store the mutex guard here
        .map(|(k, v)| (k, unsafe { std::mem::transmute(v.lock().unwrap()) }))
        .collect();
    let mut context = ImportContext {
        old_frontiers: smallvec![],
        new_frontiers: frontiers.get_frontiers(),
        old_vv: Default::default(),
        spans: vv.diff(&Default::default()).left,
        new_vv: vv,
        diff: Default::default(),
    };
    store.apply(container_map, &mut context);

    store.vv = context.new_vv;
    store.frontiers = frontiers.get_frontiers();
    drop(store);
    // FIXME: set all
    log_store
}

impl LogStore {
    pub fn encode_snapshot(&self) -> Vec<u8> {
        let encoded = encode_changes(self);
        to_vec(&encoded).unwrap()
        // compress(&to_vec(&encoded).unwrap(), &CompressConfig::default()).unwrap()
    }

    pub fn decode_snapshot(
        input: &[u8],
        client_id: Option<ClientID>,
        cfg: Configure,
    ) -> Arc<RwLock<Self>> {
        // let decompress_bytes = decompress(input).unwrap();
        let encoded = from_bytes(input).unwrap();
        decode_changes(encoded, client_id, cfg)
    }
}

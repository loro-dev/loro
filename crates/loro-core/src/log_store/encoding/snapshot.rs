use fxhash::FxHashMap;
use rle::{HasLength, RleVec, RleVecWithIndex};
use serde::{Deserialize, Serialize};
use serde_columnar::columnar;

use crate::{
    change::{Change, ChangeMergeCfg},
    container::{
        list::list_op::{DeleteSpan, InnerListOp},
        map::InnerMapSet,
        registry::{ContainerIdx, ContainerInstance},
        Container,
    },
    dag::remove_included_frontiers,
    id::{ClientID, ID},
    op::{InnerContent, Op},
    span::{HasIdSpan, HasLamportSpan},
    ContainerType, InternalString, LogStore, VersionVector,
};

use super::{
    container::{merge_2_u32_u64, split_u64_2_u32},
    ChangeEncoding, ClientIdx, Clients, DepsEncoding,
};
type Containers = Vec<Vec<u8>>;

#[columnar(vec, ser, de)]
#[derive(Debug, Clone, Serialize, Deserialize)]
struct SnapshotOpEncoding {
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

#[columnar(ser, de)]
#[derive(Debug, Serialize, Deserialize)]
pub(super) struct SnapshotEncoded {
    #[columnar(type = "vec")]
    pub(crate) changes: Vec<ChangeEncoding>,
    #[columnar(type = "vec")]
    ops: Vec<SnapshotOpEncoding>,
    #[columnar(type = "vec")]
    deps: Vec<DepsEncoding>,
    clients: Clients,
    containers: Containers,
    keys: Vec<InternalString>,
}

pub(super) fn export_snapshot(store: &LogStore) -> SnapshotEncoded {
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
            let op_len = change.ops.len() as u32;
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
                ops.push(SnapshotOpEncoding {
                    container: container_idx.to_u32(),
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
    }

    SnapshotEncoded {
        changes,
        ops,
        deps,
        clients,
        containers: container_states,
        keys,
    }
}

pub(super) fn import_snapshot(store: &mut LogStore, encoded: SnapshotEncoded) {
    let SnapshotEncoded {
        changes: change_encodings,
        ops,
        deps,
        clients,
        containers,
        keys,
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

    let mut op_iter = ops.into_iter();
    let mut changes = FxHashMap::default();
    let mut deps_iter = deps.into_iter();

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
            let SnapshotOpEncoding {
                container: container_idx,
                prop,
                value,
                is_del,
            } = op;
            let container_idx = ContainerIdx::from_u32(container_idx);
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
}

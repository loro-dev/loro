use fxhash::FxHashMap;
use rle::{HasLength, RleVec, RleVecWithIndex};
use serde::{Deserialize, Serialize};
use serde_columnar::{columnar, from_bytes, to_vec};
use smallvec::smallvec;

use crate::{
    change::{Change, ChangeMergeCfg},
    container::text::text_content::SliceRange,
    container::{
        list::list_op::{DeleteSpan, InnerListOp},
        map::{InnerMapSet, ValueSlot},
        pool_mapping::StateContent,
        registry::ContainerIdx,
        Container, ContainerID,
    },
    dag::remove_included_frontiers,
    event::RawEvent,
    hierarchy::Hierarchy,
    id::{ClientID, ID},
    log_store::ImportContext,
    op::{InnerContent, Op},
    span::{HasIdSpan, HasLamportSpan},
    version::TotalOrderStamp,
    ContainerType, InternalString, LogStore, LoroCore, LoroError, LoroValue, VersionVector,
};

use super::encode_changes::{ChangeEncoding, DepsEncoding};

type Containers = Vec<ContainerID>;
type ClientIdx = u32;
type Clients = Vec<ClientID>;

#[derive(Debug, Serialize, Deserialize)]
pub enum EncodedStateContent {
    List {
        pool: Vec<LoroValue>,
        state_len: u32,
    },
    Map {
        pool: Vec<LoroValue>,
        // TODO: rle
        keys: Vec<usize>,
        values: Vec<(u32, u32, u32)>,
    },
    Text {
        pool: Vec<u8>,
        state_len: u32,
    },
}

impl StateContent {
    fn into_encoded(
        self,
        key_to_idx: &FxHashMap<InternalString, usize>,
        client_id_to_idx: &FxHashMap<ClientID, ClientIdx>,
    ) -> EncodedStateContent {
        match self {
            StateContent::List { pool, state_len } => EncodedStateContent::List { pool, state_len },
            StateContent::Map { pool, keys, values } => {
                let mut keys_encoded = Vec::new();
                let mut values_encoded = Vec::new();
                for (k, v) in keys.into_iter().zip(values.into_iter()) {
                    let ValueSlot {
                        value,
                        order: TotalOrderStamp { lamport, client_id },
                    } = v;
                    keys_encoded.push(*key_to_idx.get(&k).unwrap());
                    values_encoded.push((
                        value,
                        lamport,
                        *client_id_to_idx.get(&client_id).unwrap(),
                    ));
                }
                EncodedStateContent::Map {
                    pool,
                    keys: keys_encoded,
                    values: values_encoded,
                }
            }
            StateContent::Text { pool, state_len } => EncodedStateContent::Text { pool, state_len },
        }
    }
}

impl EncodedStateContent {
    pub fn into_state(self, keys: &[InternalString], clients: &[ClientID]) -> StateContent {
        match self {
            EncodedStateContent::List { pool, state_len } => StateContent::List { pool, state_len },
            EncodedStateContent::Map {
                pool,
                keys: m_keys,
                values,
            } => {
                let mut keys_decoded = Vec::new();
                let mut values_decoded = Vec::new();
                for (k, v) in m_keys.into_iter().zip(values.into_iter()) {
                    let (value, lamport, client_idx) = v;
                    keys_decoded.push(keys[k].clone());
                    values_decoded.push(ValueSlot {
                        value,
                        order: TotalOrderStamp {
                            lamport,
                            client_id: clients[client_idx as usize],
                        },
                    });
                }
                StateContent::Map {
                    pool,
                    keys: keys_decoded,
                    values: values_decoded,
                }
            }
            EncodedStateContent::Text { pool, state_len } => StateContent::Text { pool, state_len },
        }
    }
}

#[columnar(vec, ser, de)]
#[derive(Debug, Clone, Serialize, Deserialize)]
struct SnapshotOpEncoding {
    #[columnar(strategy = "Rle", original_type = "u32")]
    container: u32,
    /// key index or insert/delete pos
    #[columnar(strategy = "DeltaRle")]
    prop: usize,
    // list range start or del len or map value index
    value: u64,
    #[columnar(strategy = "Rle")]
    value2: i64,
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
    container_states: Vec<EncodedStateContent>,
    keys: Vec<InternalString>,
}

const ENCODED_UNKNOWN_SLICE: i64 = -2;
const ENCODED_DELETED_CONTENT: i64 = -1;
const ENCODED_PLACEHOLDER: i64 = 0;

fn convert_inner_content(
    op_content: &InnerContent,
    key_to_idx: &mut FxHashMap<InternalString, usize>,
    keys: &mut Vec<InternalString>,
) -> (usize, u64, i64) {
    let (prop, value, is_del) = match &op_content {
        InnerContent::List(list_op) => match list_op {
            InnerListOp::Insert { slice, pos } => {
                if slice.is_unknown() {
                    (*pos, slice.content_len() as u64, ENCODED_UNKNOWN_SLICE)
                } else {
                    (
                        *pos,
                        slice.0.start as u64,
                        (slice.0.end - slice.0.start) as i64,
                    )
                }
            }
            InnerListOp::Delete(span) => {
                (span.pos as usize, span.len as u64, ENCODED_DELETED_CONTENT)
            }
        },
        InnerContent::Map(map_set) => {
            let InnerMapSet { key, value } = map_set;
            (
                *key_to_idx.entry(key.clone()).or_insert_with(|| {
                    keys.push(key.clone());
                    keys.len() - 1
                }),
                *value as u64,
                ENCODED_PLACEHOLDER,
            )
        }
    };
    (prop, value, is_del)
}

pub(super) fn encode_snapshot(store: &LogStore, gc: bool) -> Result<Vec<u8>, LoroError> {
    debug_log::debug_dbg!(&store.vv);
    debug_log::debug_dbg!(&store.changes);
    let mut client_id_to_idx: FxHashMap<ClientID, ClientIdx> = FxHashMap::default();
    let mut clients = Vec::with_capacity(store.changes.len());
    let mut change_num = 0;
    for (key, changes) in store.changes.iter() {
        client_id_to_idx.insert(*key, clients.len() as ClientIdx);
        clients.push(*key);
        change_num += changes.merged_len();
    }

    let (_, containers) = store.reg.export();
    for container_id in containers.iter() {
        let container = store.reg.get(container_id).unwrap();
        container
            .upgrade()
            .unwrap()
            .try_lock()
            .unwrap()
            .initialize_pool_mapping();
    }

    let mut changes = Vec::with_capacity(change_num);
    let mut ops = Vec::with_capacity(change_num);
    let mut keys = Vec::new();
    let mut key_to_idx = FxHashMap::default();
    let mut deps = Vec::with_capacity(change_num);
    for (client_idx, (_, change_vec)) in store.changes.iter().enumerate() {
        for change in change_vec.iter() {
            let mut op_len = 0;
            for dep in change.deps.iter() {
                deps.push(DepsEncoding::new(
                    *client_id_to_idx.get(&dep.client_id).unwrap(),
                    dep.counter,
                ));
            }
            for op in change.ops.iter() {
                let container_idx = op.container;
                let container_id = store.reg.get_id(container_idx).unwrap();
                let container = store.reg.get(container_id).unwrap();
                let new_ops = container
                    .upgrade()
                    .unwrap()
                    .try_lock()
                    .unwrap()
                    .to_export_snapshot(&op.content, gc);
                op_len += new_ops.len();
                for op_content in new_ops {
                    let (prop, value, value2) =
                        convert_inner_content(&op_content, &mut key_to_idx, &mut keys);
                    ops.push(SnapshotOpEncoding {
                        container: container_idx.to_u32(),
                        prop,
                        value,
                        value2,
                    });
                }
            }

            changes.push(ChangeEncoding {
                client_idx: client_idx as ClientIdx,
                counter: change.id.counter,
                lamport: change.lamport,
                timestamp: change.timestamp,
                deps_len: change.deps.len() as u32,
                op_len: op_len as u32,
            });
        }
    }

    let container_states = containers
        .iter()
        .map(|container_id| {
            let container = store.reg.get(container_id).unwrap();
            container
                .upgrade()
                .unwrap()
                .try_lock()
                .unwrap()
                .encode_and_release_pool_mapping()
                .into_encoded(&key_to_idx, &client_id_to_idx)
        })
        .collect();

    let encoded = SnapshotEncoded {
        changes,
        ops,
        deps,
        clients,
        containers,
        container_states,
        keys,
    };
    to_vec(&encoded).map_err(|e| LoroError::DecodeError(e.to_string().into()))
}

pub(super) fn decode_snapshot(
    store: &mut LogStore,
    hierarchy: &mut Hierarchy,
    input: &[u8],
) -> Result<Vec<RawEvent>, LoroError> {
    let encoded: SnapshotEncoded =
        from_bytes(input).map_err(|e| LoroError::DecodeError(e.to_string().into()))?;
    let SnapshotEncoded {
        changes: change_encodings,
        ops,
        deps,
        clients,
        containers,
        container_states,
        keys,
    } = encoded;

    if change_encodings.is_empty() {
        // register
        if !container_states.is_empty() {
            for container_id in containers.into_iter() {
                store.get_or_create_container(&container_id);
            }
        }
        return Ok(vec![]);
    }

    let mut op_iter = ops.into_iter();
    let mut changes = FxHashMap::default();
    let mut deps_iter = deps.into_iter();
    let mut container_idx2type = FxHashMap::default();

    for container_id in containers.iter() {
        let container_idx = store.reg.get_or_create_container_idx(container_id);
        container_idx2type.insert(container_idx, container_id.container_type());
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
                value2,
            } = op;

            let container_idx = ContainerIdx::from_u32(container_idx);
            let container_type = container_idx2type[&container_idx];
            let content = match container_type {
                ContainerType::Map => {
                    let key = keys[prop].clone();
                    InnerContent::Map(InnerMapSet {
                        key,
                        value: value as u32,
                    })
                }
                ContainerType::List | ContainerType::Text => {
                    let is_del = value2 == ENCODED_DELETED_CONTENT;
                    let list_op = if is_del {
                        InnerListOp::Delete(DeleteSpan {
                            pos: prop as isize,
                            len: value as isize,
                        })
                    } else {
                        let is_unknown = value2 == ENCODED_UNKNOWN_SLICE;
                        if is_unknown {
                            InnerListOp::Insert {
                                slice: SliceRange::new_unknown(value as u32),
                                pos: prop,
                            }
                        } else {
                            InnerListOp::Insert {
                                slice: (value as u32..(value as i64 + value2) as u32).into(),
                                pos: prop,
                            }
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

    debug_log::debug_dbg!(&vv, &changes);

    let can_load = match vv.partial_cmp(&store.vv) {
        Some(ord) => match ord {
            std::cmp::Ordering::Less => false,
            std::cmp::Ordering::Equal => return Ok(vec![]),
            std::cmp::Ordering::Greater => true,
        },
        None => false,
    };

    if can_load {
        let mut import_context = load_snapshot(
            store,
            hierarchy,
            vv,
            changes,
            containers,
            container_states,
            container_idx2type,
            &keys,
            &clients,
        );
        Ok(store.get_events(hierarchy, &mut import_context))
    } else {
        let new_loro = LoroCore::default();
        let mut new_store = new_loro.log_store.try_write().unwrap();
        let mut new_hierarchy = new_loro.hierarchy.try_lock().unwrap();
        load_snapshot(
            &mut new_store,
            &mut new_hierarchy,
            vv,
            changes,
            containers,
            container_states,
            container_idx2type,
            &keys,
            &clients,
        );
        let diff_changes = new_store.export(&store.vv);
        Ok(store.import(hierarchy, diff_changes))
    }
}

#[allow(clippy::too_many_arguments)]
fn load_snapshot(
    new_store: &mut LogStore,
    new_hierarchy: &mut Hierarchy,
    vv: VersionVector,
    changes: FxHashMap<ClientID, RleVecWithIndex<Change, ChangeMergeCfg>>,
    containers: Vec<ContainerID>,
    container_states: Vec<EncodedStateContent>,
    mut container_idx2type: FxHashMap<ContainerIdx, ContainerType>,
    keys: &[InternalString],
    clients: &[u64],
) -> ImportContext {
    let mut frontiers = vv.clone();
    for (_, changes) in changes.iter() {
        for change in changes.iter() {
            remove_included_frontiers(&mut frontiers, &change.deps);
        }
    }

    // rebuild states by snapshot
    let mut import_context = ImportContext {
        old_frontiers: smallvec![],
        new_frontiers: frontiers.get_frontiers(),
        old_vv: VersionVector::new(),
        spans: vv.diff(&new_store.vv).left,
        new_vv: vv.clone(),
        diff: Default::default(),
        patched_old_vv: None,
    };
    for (container_id, pool_mapping) in containers.into_iter().zip(container_states.into_iter()) {
        let container_idx = new_store.reg.get_or_create_container_idx(&container_id);
        container_idx2type.insert(container_idx, container_id.container_type());
        let state = pool_mapping.into_state(keys, clients);
        let container = new_store.reg.get_by_idx(container_idx).unwrap();
        let mut container = container.try_lock().unwrap();
        container.to_import_snapshot(state, new_hierarchy, &mut import_context);
    }

    new_store.latest_lamport = changes
        .values()
        .map(|changes| changes.last().unwrap().lamport_last())
        .max()
        .unwrap();
    new_store.latest_timestamp = changes
        .values()
        .map(|changes| changes.last().unwrap().timestamp)
        .max()
        .unwrap();

    new_store.changes = changes;

    new_store.vv = vv;
    new_store.frontiers = frontiers.get_frontiers();
    import_context
}

#[cfg(test)]
mod test {
    use crate::LoroCore;

    #[test]
    fn cannot_load() {
        let mut loro = LoroCore::new(Default::default(), Some(1));
        let mut text = loro.get_text("text");
        text.insert(&loro, 0, "abc").unwrap();
        let snapshot = loro.encode_all();

        let mut loro2 = LoroCore::new(Default::default(), Some(2));
        let mut text2 = loro2.get_text("text");
        text2.insert(&loro2, 0, "efg").unwrap();
        loro2.decode(&snapshot).unwrap();
        assert_eq!(text2.get_value().to_json_pretty(), "\"abcefg\"");
    }
}

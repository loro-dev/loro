use std::{collections::VecDeque, ops::Range};

use fxhash::FxHashMap;
use itertools::Itertools;
use rle::{HasLength, RleVec, RleVecWithIndex};
use serde::{Deserialize, Serialize};
use serde_columnar::{columnar, from_bytes, to_vec};
use smallvec::SmallVec;

use crate::{
    change::{Change, ChangeMergeCfg, Lamport},
    container::text::text_content::SliceRange,
    container::{
        list::list_op::{DeleteSpan, InnerListOp},
        map::{InnerMapSet, ValueSlot},
        pool_mapping::StateContent,
        registry::ContainerIdx,
        ContainerID, ContainerTrait,
    },
    dag::{remove_included_frontiers, Dag},
    event::EventDiff,
    hierarchy::Hierarchy,
    id::{ClientID, Counter, ID},
    log_store::{ImportContext, RemoteClientChanges},
    op::{InnerContent, Op},
    span::HasLamportSpan,
    version::{Frontiers, TotalOrderStamp},
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
    #[columnar(strategy = "Rle", original_type = "usize")]
    container: usize,
    /// key index or insert/delete pos
    #[columnar(strategy = "DeltaRle")]
    prop: usize,
    // list range start or del len or map value index, maybe negative
    value: i64,
    // List: the length of content when inserting, -2 when the inserted content is unknown, and -1 when deleting.
    // Map: always -1
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
) -> (usize, i64, i64) {
    let (prop, value, is_del) = match &op_content {
        InnerContent::List(list_op) => match list_op {
            InnerListOp::Insert { slice, pos } => {
                if slice.is_unknown() {
                    (*pos, slice.content_len() as i64, ENCODED_UNKNOWN_SLICE)
                } else {
                    (
                        *pos,
                        slice.0.start as i64,
                        (slice.0.end - slice.0.start) as i64,
                    )
                }
            }
            InnerListOp::Delete(span) => {
                (span.pos as usize, span.len as i64, ENCODED_DELETED_CONTENT)
            }
        },
        InnerContent::Map(map_set) => {
            let InnerMapSet { key, value } = map_set;
            (
                *key_to_idx.entry(key.clone()).or_insert_with(|| {
                    keys.push(key.clone());
                    keys.len() - 1
                }),
                *value as i64,
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

    let containers = store.reg.export_by_sorted_idx();
    // During a transaction, we may create some containers which are deleted later. And these containers also need a unique ContainerIdx.
    // So when we encode snapshot, we need to sort the containers by ContainerIdx and change the `container` of ops to the index of containers.
    // An empty store decodes the snapshot, it will create these containers in a sequence of natural numbers so that containers and ops can correspond one-to-one
    let container_to_new_idx: FxHashMap<_, _> = containers
        .iter()
        .enumerate()
        .map(|(i, id)| (id, i))
        .collect();
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
            let client_id = change.id.client_id;
            let mut op_len = 0;
            let mut deps_len = 0;
            let mut dep_on_self = false;
            for dep in change.deps.iter() {
                // the first change will encode the self-client deps
                if dep.client_id == client_id {
                    dep_on_self = true;
                } else {
                    deps.push(DepsEncoding::new(
                        *client_id_to_idx.get(&dep.client_id).unwrap(),
                        dep.counter,
                    ));
                    deps_len += 1;
                }
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
                let new_idx = *container_to_new_idx.get(container_id).unwrap();
                op_len += new_ops.len();
                for op_content in new_ops {
                    let (prop, value, value2) =
                        convert_inner_content(&op_content, &mut key_to_idx, &mut keys);
                    ops.push(SnapshotOpEncoding {
                        container: new_idx,
                        prop,
                        value,
                        value2,
                    });
                }
            }

            changes.push(ChangeEncoding {
                client_idx: client_idx as ClientIdx,
                timestamp: change.timestamp,
                deps_len,
                dep_on_self,
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
        .collect::<Vec<_>>();
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
) -> Result<Vec<EventDiff>, LoroError> {
    let (changes, events) = decode_snapshot_to_inner_format(store, hierarchy, input)?;
    let pending_events = store.import(hierarchy, changes);
    if let Some(events) = events {
        Ok(pending_events.into_iter().chain(events).collect())
    } else {
        Ok(pending_events)
    }
}

pub(super) fn decode_snapshot_to_inner_format(
    store: &mut LogStore,
    hierarchy: &mut Hierarchy,
    input: &[u8],
) -> Result<(RemoteClientChanges, Option<Vec<EventDiff>>), LoroError> {
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
        return Ok((Default::default(), None));
    }
    let mut container_idx2type = FxHashMap::default();
    for (idx, container_id) in containers.iter().enumerate() {
        // assert containers are sorted by container_idx
        container_idx2type.insert(idx, container_id.container_type());
    }

    // calc vv
    let vv = calc_vv(&change_encodings, &ops, &clients, &container_idx2type);
    let can_load = match vv.partial_cmp(&store.vv) {
        Some(ord) => match ord {
            std::cmp::Ordering::Less => {
                // TODO warning
                debug_log::debug_log!("[Warning] the vv of encoded snapshot is smaller than self, no change is applied");
                return Ok((Default::default(), None));
            }
            std::cmp::Ordering::Equal => {
                debug_log::debug_log!("vv is equal, no change is applied");
                return Ok((Default::default(), None));
            }
            std::cmp::Ordering::Greater => store.vv.is_empty(),
        },
        None => false,
    };

    let mut op_iter = ops.into_iter();
    let mut changes_dq = FxHashMap::default();
    let mut deps_iter = deps.into_iter();

    for (_, this_changes_encoding) in &change_encodings.into_iter().group_by(|c| c.client_idx) {
        let mut counter = 0;
        for change_encoding in this_changes_encoding {
            let ChangeEncoding {
                client_idx,
                timestamp,
                op_len,
                deps_len,
                dep_on_self,
            } = change_encoding;

            let client_id = clients[client_idx as usize];
            let mut ops = RleVec::<[Op; 2]>::new();
            let mut delta = 0;
            for op in op_iter.by_ref().take(op_len as usize) {
                let SnapshotOpEncoding {
                    container: container_idx,
                    prop,
                    value,
                    value2,
                } = op;
                let container_type = container_idx2type[&container_idx];
                let container_idx = ContainerIdx::from_u32(container_idx as u32);
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
                                    slice: (value as u32..(value + value2) as u32).into(),
                                    pos: prop,
                                }
                            }
                        };
                        InnerContent::List(list_op)
                    }
                };
                let op = Op {
                    counter: counter + delta,
                    container: container_idx,
                    content,
                };
                delta += op.content_len() as i32;
                ops.push(op);
            }

            let mut deps = (0..deps_len)
                .map(|_| {
                    let raw = deps_iter.next().unwrap();
                    ID::new(clients[raw.client_idx as usize], raw.counter)
                })
                .collect::<SmallVec<_>>();

            if dep_on_self {
                deps.push(ID::new(client_id, counter - 1));
            }
            let change = Change {
                id: ID { client_id, counter },
                // cal lamport after parsing all changes
                lamport: 0,
                timestamp,
                ops,
                deps,
            };

            counter += delta;

            changes_dq
                .entry(client_id)
                .or_insert_with(VecDeque::new)
                .push_back(change)
        }
    }
    // calculate lamport
    let mut lamport_map = FxHashMap::default();
    let mut changes = FxHashMap::default();
    let mut client_ids: VecDeque<_> = changes_dq.keys().copied().collect();
    let len = client_ids.len();
    let mut loop_time = len;
    while let Some(client_id) = client_ids.pop_front() {
        let this_client_changes = changes_dq.get_mut(&client_id).unwrap();
        while let Some(mut change) = this_client_changes.pop_front() {
            match get_lamport_by_deps(&change.deps, &lamport_map, None) {
                Ok(lamport) => {
                    change.lamport = lamport;
                    lamport_map.entry(client_id).or_insert_with(Vec::new).push((
                        change.id.counter..change.id.counter + change.content_len() as Counter,
                        lamport,
                    ));
                    changes
                        .entry(client_id)
                        .or_insert_with(|| RleVecWithIndex::new_with_conf(ChangeMergeCfg::new()))
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

    if can_load {
        let mut import_context = load_snapshot(
            store,
            hierarchy,
            vv,
            changes,
            containers,
            container_states,
            &keys,
            &clients,
        );
        let mut changes = FxHashMap::default();
        for client_id in import_context.new_vv.keys() {
            changes.insert(*client_id, Vec::new());
        }
        let snapshot_events = store.get_events(hierarchy, &mut import_context);
        Ok((changes, Some(snapshot_events)))
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
            &keys,
            &clients,
        );
        let diff_changes = new_store.export(&store.vv);
        Ok((diff_changes, None))
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
        // // old_frontiers: smallvec![],
        new_frontiers: frontiers.get_frontiers(),
        old_vv: new_store.vv(),
        spans: vv.diff(&new_store.vv).left,
        new_vv: vv.clone(),
        diff: Default::default(),
        patched_old_vv: None,
    };
    for (container_id, pool_mapping) in containers.into_iter().zip(container_states.into_iter()) {
        let state = pool_mapping.into_state(keys, clients);
        let container = new_store.reg.get_or_create(&container_id);
        let container = container.upgrade().unwrap();
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

fn calc_vv(
    changes_encoding: &[ChangeEncoding],
    ops_encoding: &[SnapshotOpEncoding],
    clients: &[ClientID],
    idx_to_container_type: &FxHashMap<usize, ContainerType>,
) -> VersionVector {
    let mut vv = FxHashMap::default();
    let mut op_iter = ops_encoding.iter();
    for (client_idx, this_changes_encoding) in &changes_encoding.iter().group_by(|c| c.client_idx) {
        let client_id = clients[client_idx as usize];
        let mut counter = 0;
        for change_encoding in this_changes_encoding {
            let op_len = change_encoding.op_len;
            let mut delta = 0;
            for op in op_iter.by_ref().take(op_len as usize) {
                let SnapshotOpEncoding {
                    container: container_idx,
                    prop: _,
                    value,
                    value2,
                } = *op;
                let container_type = idx_to_container_type[&container_idx];
                let op_content_len = match container_type {
                    ContainerType::Map => 1,
                    _ => {
                        let is_del = value2 == ENCODED_DELETED_CONTENT;
                        if is_del {
                            value.unsigned_abs()
                        } else {
                            let is_unknown = value2 == ENCODED_UNKNOWN_SLICE;
                            if is_unknown {
                                value as u64
                            } else {
                                value2 as u64
                            }
                        }
                    }
                };
                delta += op_content_len as i32;
            }
            counter += delta;
        }
        vv.insert(client_id, counter);
    }
    vv.into()
}

pub(crate) fn get_lamport_by_deps(
    deps: &Frontiers,
    lamport_map: &FxHashMap<ClientID, Vec<(Range<Counter>, Lamport)>>,
    store: Option<&LogStore>,
) -> Result<Lamport, ClientID> {
    let mut ans = Vec::new();
    for id in deps.iter() {
        if let Some(c) = store.and_then(|x| x.lookup_change(*id)) {
            let offset = id.counter - c.id.counter;
            ans.push(c.lamport + offset as u32);
        } else if let Some(v) = lamport_map.get(&id.client_id) {
            if let Some((lamport, offset)) = get_value_from_range_map(v, id.counter) {
                ans.push(lamport + offset);
            } else {
                return Err(id.client_id);
            }
        } else {
            return Err(id.client_id);
        }
    }
    Ok(ans.into_iter().max().unwrap_or(0) + 1)
}

fn get_value_from_range_map(
    v: &[(Range<Counter>, Lamport)],
    key: Counter,
) -> Option<(Lamport, u32)> {
    let index = match v.binary_search_by_key(&key, |&(ref range, _)| range.start) {
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

#[cfg(test)]
mod test {
    use crate::{ContainerType, LoroCore};

    #[test]
    fn cannot_load() {
        let mut loro = LoroCore::new(Default::default(), Some(1));
        let mut map = loro.get_map("map");
        map.insert(&loro, "0", ContainerType::List).unwrap();
        map.delete(&loro, "0").unwrap();
        let mut loro2 = LoroCore::new(Default::default(), Some(2));
        loro.decode(&loro2.encode_all()).unwrap();
        loro2.decode(&loro.encode_all()).unwrap();
    }
}

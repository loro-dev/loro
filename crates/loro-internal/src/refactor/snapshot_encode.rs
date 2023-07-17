#![allow(warnings)]

use std::{borrow::Cow, collections::VecDeque, mem::take};

use compact_bytes::CompactBytes;
use debug_log::debug_dbg;
use fxhash::{FxHashMap, FxHashSet};
use loro_common::{HasLamport, ID};
use loro_preload::{
    CommonArena, EncodedAppState, EncodedContainerState, FinalPhase, MapEntry, TempArena,
};
use postcard::to_allocvec;
use rle::{HasLength, RleVec};
use serde::{Deserialize, Serialize};
use serde_columnar::{columnar, to_vec};
use smallvec::smallvec;

use crate::{
    change::{Change, Lamport, Timestamp},
    container::{
        list::list_op::InnerListOp, map::InnerMapSet, registry::ContainerIdx, ContainerID,
    },
    delta::{MapDelta, MapValue},
    id::{Counter, PeerID},
    log_store::encoding::{ENCODE_SCHEMA_VERSION, MAGIC_BYTES},
    op::{InnerContent, Op},
    version::Frontiers,
    EncodeMode, InternalString, LoroError, LoroValue,
};

use super::{
    arena::SharedArena,
    loro::LoroApp,
    oplog::OpLog,
    state::{AppState, AppStateDiff, ListState, MapState, State, TextState},
};

type Containers = Vec<ContainerID>;
type ClientIdx = u32;
type Clients = Vec<PeerID>;

#[columnar(ser, de)]
#[derive(Debug, Serialize, Deserialize)]
pub(super) struct OplogEncoded {
    #[columnar(type = "vec")]
    pub(crate) changes: Vec<EncodedChange>,
    #[columnar(type = "vec")]
    ops: Vec<EncodedSnapshotOp>,
    #[columnar(type = "vec")]
    deps: Vec<DepsEncoding>,
}

impl OplogEncoded {
    fn decode(data: &FinalPhase) -> Result<Self, LoroError> {
        serde_columnar::from_bytes(&data.oplog)
            .map_err(|e| LoroError::DecodeError(e.to_string().into_boxed_str()))
    }

    fn encode(&self) -> Vec<u8> {
        to_vec(self).unwrap()
    }
}

#[columnar(vec, ser, de)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncodedChange {
    #[columnar(strategy = "Rle", original_type = "u32")]
    pub(super) peer_idx: ClientIdx,
    #[columnar(strategy = "DeltaRle", original_type = "i64")]
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

#[columnar(vec, ser, de)]
#[derive(Debug, Clone, Serialize, Deserialize)]
struct EncodedSnapshotOp {
    #[columnar(strategy = "Rle", original_type = "usize")]
    container: u32,
    /// key index or insert/delete pos
    #[columnar(strategy = "DeltaRle")]
    prop: usize,
    // List range start | del len | map value index
    // This value can be negative
    value: i64,
    // List: the length of content when inserting. -2 when the inserted content is unknown. -1 when it's a deletion
    // Map: always -1
    #[columnar(strategy = "Rle")]
    value2: i64,
}

enum SnapshotOp {
    ListInsert { pos: usize, start: u32, len: u32 },
    ListDelete { pos: usize, len: isize },
    ListUnknown { pos: usize, len: usize },
    Map { key: usize, value_idx_plus_one: u32 },
}

impl EncodedSnapshotOp {
    pub fn get_list(&self) -> SnapshotOp {
        if self.value2 == -1 {
            SnapshotOp::ListDelete {
                pos: self.prop as usize,
                len: self.value as isize,
            }
        } else if self.value2 == -2 {
            SnapshotOp::ListUnknown {
                pos: self.prop,
                len: self.value as usize,
            }
        } else {
            SnapshotOp::ListInsert {
                pos: self.prop,
                start: self.value as u32,
                len: self.value2 as u32,
            }
        }
    }

    pub fn get_map(&self) -> SnapshotOp {
        SnapshotOp::Map {
            key: self.prop,
            value_idx_plus_one: self.value as u32,
        }
    }

    pub fn from(value: SnapshotOp, container: u32) -> Self {
        match value {
            SnapshotOp::ListInsert { pos, start, len } => Self {
                container,
                prop: pos,
                value: start as i64,
                value2: len as i64,
            },
            SnapshotOp::ListDelete { pos, len } => Self {
                container,
                prop: pos as usize,
                value: len as i64,
                value2: -1,
            },
            SnapshotOp::ListUnknown { pos, len } => Self {
                container,
                prop: pos,
                value: len as i64,
                value2: -2,
            },
            SnapshotOp::Map {
                key,
                value_idx_plus_one: value,
            } => Self {
                container,
                prop: key,
                value: value as i64,
                value2: -1,
            },
        }
    }
}

#[columnar(vec, ser, de)]
#[derive(Debug, Copy, Clone, Serialize, Deserialize)]
pub(super) struct DepsEncoding {
    #[columnar(strategy = "Rle", original_type = "u32")]
    pub(super) peer_idx: ClientIdx,
    #[columnar(strategy = "DeltaRle", original_type = "i32")]
    pub(super) counter: Counter,
}

impl DepsEncoding {
    pub(super) fn new(client_idx: ClientIdx, counter: Counter) -> Self {
        Self {
            peer_idx: client_idx,
            counter,
        }
    }
}

pub fn encode_app_snapshot(app: &LoroApp) -> Vec<u8> {
    let pre_encoded_state = preprocess_app_state(&app.app_state().lock().unwrap());
    let f = encode_oplog(&app.oplog().lock().unwrap(), Some(pre_encoded_state));
    // f.diagnose_size();
    miniz_oxide::deflate::compress_to_vec(&f.encode(), 6)
}

pub fn decode_app_snapshot(app: &LoroApp, bytes: &[u8]) -> Result<(), LoroError> {
    assert!(app.is_empty());
    let bytes = miniz_oxide::inflate::decompress_to_vec(bytes).unwrap();
    let data = FinalPhase::decode(&bytes)?;
    let mut app_state = app.app_state().lock().unwrap();
    decode_state(&mut app_state, &data)?;
    let arena = app_state.arena.clone();
    let oplog = decode_oplog(&mut app.oplog().lock().unwrap(), &data, Some(arena))?;
    Ok(())
}

#[derive(Default)]
struct PreEncodedState {
    common: CommonArena<'static>,
    arena: TempArena<'static>,
    key_lookup: FxHashMap<InternalString, usize>,
    value_lookup: FxHashMap<LoroValue, usize>,
    peer_lookup: FxHashMap<PeerID, usize>,
    app_state: EncodedAppState,
}

fn preprocess_app_state(app_state: &AppState) -> PreEncodedState {
    assert!(!app_state.is_in_txn());
    let mut peers = Vec::new();
    let mut peer_lookup = FxHashMap::default();
    let mut bytes = Vec::new();
    let mut keywords = Vec::new();
    let mut values = Vec::new();
    let mut key_lookup = FxHashMap::default();
    let mut value_lookup = FxHashMap::default();
    let mut encoded = EncodedAppState {
        frontiers: app_state.frontiers.iter().cloned().collect(),
        states: Vec::new(),
        parents: app_state
            .arena
            .export_parents()
            .into_iter()
            .map(|x| x.map(|x| x.to_index()))
            .collect(),
    };

    let mut record_key = |key: &InternalString| {
        if let Some(idx) = key_lookup.get(key) {
            return *idx;
        }

        keywords.push(key.clone());
        key_lookup
            .entry(key.clone())
            .or_insert_with(|| keywords.len() - 1);
        keywords.len() - 1
    };

    let mut record_value = |value: &LoroValue| {
        if let Some(idx) = value_lookup.get(value) {
            return *idx;
        }

        let idx = values.len();
        values.push(value.clone());
        value_lookup.entry(value.clone()).or_insert_with(|| idx);
        idx
    };

    let mut record_peer = |peer: PeerID| {
        if let Some(idx) = peer_lookup.get(&peer) {
            return *idx as u32;
        }

        peers.push(peer);
        peer_lookup.entry(peer).or_insert_with(|| peers.len() - 1);
        peers.len() as u32 - 1
    };

    for (container_idx, state) in app_state.states.iter() {
        match state {
            State::ListState(list) => {
                let v = list.iter().map(|value| record_value(&value)).collect();
                encoded.states.push(EncodedContainerState::List((v)))
            }
            State::MapState(map) => {
                let v = map
                    .iter()
                    .map(|(key, value)| {
                        let key = record_key(key);
                        let peer = value;
                        MapEntry {
                            key,
                            value: if let Some(value) = &value.value {
                                record_value(value) + 1
                            } else {
                                0
                            },
                            peer: record_peer(value.lamport.1),
                            counter: value.counter as u32,
                            lamport: value.lamport(),
                        }
                    })
                    .collect();
                encoded.states.push(EncodedContainerState::Map(v))
            }
            State::TextState(text) => {
                for span in text.iter() {
                    bytes.extend_from_slice(span.as_bytes());
                }
                encoded
                    .states
                    .push(EncodedContainerState::Text { len: text.len() })
            }
        }
    }

    let mut common = CommonArena {
        peer_ids: peers.into(),
        container_ids: app_state.arena.export_containers(),
    };

    let mut arena = TempArena {
        text: bytes.into(),
        keywords,
        values,
    };

    PreEncodedState {
        common,
        arena,
        key_lookup,
        value_lookup,
        peer_lookup,
        app_state: encoded,
    }
}

fn encode_oplog(oplog: &OpLog, state_ref: Option<PreEncodedState>) -> FinalPhase<'static> {
    let state_ref = state_ref.unwrap_or_default();
    let PreEncodedState {
        mut common,
        arena,
        mut key_lookup,
        mut value_lookup,
        mut peer_lookup,
        mut app_state,
    } = state_ref;
    if common.container_ids.is_empty() {
        common.container_ids = oplog.arena.export_containers();
    }
    let mut bytes = CompactBytes::new();
    bytes.append(&arena.text);
    let mut extra_keys = Vec::new();
    let mut extra_values = Vec::new();

    let mut record_key = |key: &InternalString| {
        if let Some(idx) = key_lookup.get(key) {
            return *idx;
        }

        let idx = extra_keys.len() + arena.keywords.len();
        extra_keys.push(key.clone());
        key_lookup.entry(key.clone()).or_insert_with(|| idx);
        idx
    };

    let mut record_value = |value: &LoroValue| {
        if let Some(idx) = value_lookup.get(value) {
            return *idx;
        }

        let idx = extra_values.len() + arena.values.len();
        extra_values.push(value.clone());
        value_lookup.entry(value.clone()).or_insert_with(|| idx);
        idx
    };

    let Cow::Owned(mut peers) = take(&mut common.peer_ids) else {unreachable!()};
    let mut record_peer = |peer: PeerID| {
        if let Some(idx) = peer_lookup.get(&peer) {
            return *idx as u32;
        }

        peers.push(peer);
        peer_lookup.entry(peer).or_insert_with(|| peers.len() - 1);
        peers.len() as u32 - 1
    };

    let mut record_str = |s: &[u8], mut pos: usize, container_idx: u32| {
        let slices = bytes.alloc_advance_with_min_match_size(s, 8);
        slices
            .into_iter()
            .map(|range| {
                let ans = SnapshotOp::ListInsert {
                    pos,
                    start: range.start as u32,
                    len: range.len() as u32,
                };
                pos += range.len();
                EncodedSnapshotOp::from(ans, container_idx)
            })
            .collect::<Vec<_>>()
    };

    // Add all changes
    let mut changes: Vec<&Change> = Vec::with_capacity(oplog.len_changes());
    for (peer, peer_changes) in oplog.changes.iter() {
        for change in peer_changes.iter() {
            changes.push(change);
        }
    }

    // Sort changes by lamport. So it's in causal order
    changes.sort_by_key(|x| x.lamport());
    let mut encoded_changes = Vec::with_capacity(changes.len());
    let mut encoded_ops: Vec<EncodedSnapshotOp> =
        Vec::with_capacity(changes.iter().map(|x| x.ops.len()).sum());
    let mut deps = Vec::with_capacity(changes.iter().map(|x| x.deps.len()).sum());
    for change in changes {
        let peer_idx = record_peer(change.id.peer);
        let mut lamport = change.lamport();
        let op_index_start = encoded_ops.len();
        for op in change.ops.iter() {
            let counter = op.counter;
            match &op.content {
                InnerContent::List(list) => match list {
                    InnerListOp::Insert { slice, pos } => {
                        if slice.is_unknown() {
                            encoded_ops.push(EncodedSnapshotOp::from(
                                SnapshotOp::ListUnknown {
                                    pos: *pos as usize,
                                    len: slice.atom_len(),
                                },
                                op.container.to_index(),
                            ));
                        } else {
                            match op.container.get_type() {
                                loro_common::ContainerType::Text => {
                                    let slice = oplog
                                        .arena
                                        .slice_bytes(slice.0.start as usize..slice.0.end as usize);
                                    encoded_ops.extend(record_str(
                                        &slice,
                                        *pos as usize,
                                        op.container.to_index(),
                                    ));
                                }
                                loro_common::ContainerType::List => {
                                    let values = oplog
                                        .arena
                                        .get_values(slice.0.start as usize..slice.0.end as usize);
                                    let mut pos = *pos;
                                    for value in values {
                                        let idx = record_value(&value);
                                        encoded_ops.push(EncodedSnapshotOp::from(
                                            SnapshotOp::ListInsert {
                                                pos,
                                                start: idx as u32,
                                                len: 1,
                                            },
                                            op.container.to_index(),
                                        ));
                                        pos += 1;
                                    }
                                }
                                loro_common::ContainerType::Map => unreachable!(),
                            }
                        }
                    }
                    InnerListOp::Delete(del) => {
                        encoded_ops.push(EncodedSnapshotOp::from(
                            SnapshotOp::ListDelete {
                                pos: del.pos as usize,
                                len: del.len,
                            },
                            op.container.to_index(),
                        ));
                    }
                },
                InnerContent::Map(map) => {
                    let key = record_key(&map.key);
                    let value = oplog.arena.get_value(map.value as usize);
                    // FIXME: delete in map
                    let value = if let Some(value) = value {
                        record_value(&value) + 1
                    } else {
                        0
                    };
                    encoded_ops.push(EncodedSnapshotOp::from(
                        SnapshotOp::Map {
                            key,
                            value_idx_plus_one: value as u32,
                        },
                        op.container.to_index(),
                    ));
                }
            }
            lamport += op.atom_len() as Lamport;
        }
        let op_len = encoded_ops.len() - op_index_start;
        let mut dep_on_self = false;
        let dep_start = deps.len();
        for dep in change.deps.iter() {
            if dep.peer == change.id.peer {
                dep_on_self = true;
            } else {
                let peer_idx = record_peer(dep.peer);
                deps.push(DepsEncoding {
                    peer_idx,
                    counter: dep.counter,
                });
            }
        }

        let deps_len = deps.len() - dep_start;
        encoded_changes.push(EncodedChange {
            peer_idx,
            timestamp: change.timestamp,
            op_len: op_len as u32,
            deps_len: deps_len as u32,
            dep_on_self,
        })
    }

    common.peer_ids = Cow::Owned(peers);
    let bytes = bytes.take();
    let mut extra_text = (&bytes[arena.text.len()..]).to_vec();
    let oplog_encoded = OplogEncoded {
        changes: encoded_changes,
        ops: encoded_ops,
        deps,
    };
    // println!("OplogEncoded:");
    // println!("changes {}", oplog_encoded.changes.len());
    // println!("ops {}", oplog_encoded.ops.len());
    // println!("deps {}", oplog_encoded.deps.len());
    // println!("\n");
    let ans = FinalPhase {
        common: Cow::Owned(common.encode()),
        app_state: Cow::Owned(app_state.encode()),
        state_arena: Cow::Owned(arena.encode()),
        additional_arena: Cow::Owned(
            TempArena {
                text: Cow::Owned(extra_text),
                keywords: extra_keys,
                values: extra_values,
            }
            .encode(),
        ),
        oplog: Cow::Owned(oplog_encoded.encode()),
    };

    ans
}

pub fn decode_oplog(
    oplog: &mut OpLog,
    data: &FinalPhase,
    arena: Option<SharedArena>,
) -> Result<(), LoroError> {
    let arena = arena.unwrap_or_else(SharedArena::default);
    oplog.arena = arena.clone();
    let state_arena = TempArena::decode_state_arena(&data)?;
    let mut extra_arena = TempArena::decode_additional_arena(&data)?;
    arena.alloc_str_fast(&*state_arena.text);
    arena.alloc_str_fast(&*extra_arena.text);
    arena.alloc_values(state_arena.values.into_iter());
    arena.alloc_values(extra_arena.values.into_iter());
    let mut keys = state_arena.keywords;
    keys.append(&mut extra_arena.keywords);

    let common = CommonArena::decode(&data)?;
    let oplog_data = OplogEncoded::decode(data)?;

    let mut changes = Vec::new();
    let mut dep_iter = oplog_data.deps.iter();
    let mut op_iter = oplog_data.ops.iter();
    let mut counters = FxHashMap::default();
    for change in oplog_data.changes.iter() {
        let peer_idx = change.peer_idx as usize;
        let peer_id = common.peer_ids[peer_idx];
        let timestamp = change.timestamp;
        let deps_len = change.deps_len;
        let dep_on_self = change.dep_on_self;
        let mut ops = RleVec::new();
        let counter_mut = counters.entry(peer_idx).or_insert(0);
        let start_counter = *counter_mut;

        // calc ops
        let mut total_len = 0;
        for _ in 0..change.op_len {
            // calc op
            let id = ID::new(peer_id, *counter_mut);
            let encoded_op = op_iter.next().unwrap();
            let container = common.container_ids[encoded_op.container as usize].clone();
            let container_idx = arena.register_container(&container);
            let op = match container.container_type() {
                loro_common::ContainerType::Text | loro_common::ContainerType::List => {
                    let op = encoded_op.get_list();
                    match op {
                        SnapshotOp::ListInsert { start, len, pos } => Op::new(
                            id,
                            InnerContent::List(InnerListOp::new_insert(start..start + len, pos)),
                            container_idx,
                        ),
                        SnapshotOp::ListDelete { len, pos } => Op::new(
                            id,
                            InnerContent::List(InnerListOp::new_del(pos, len)),
                            container_idx,
                        ),
                        SnapshotOp::ListUnknown { len, pos } => Op::new(
                            id,
                            InnerContent::List(InnerListOp::new_unknown(pos, len)),
                            container_idx,
                        ),
                        SnapshotOp::Map { .. } => {
                            unreachable!()
                        }
                    }
                }
                loro_common::ContainerType::Map => {
                    let op = encoded_op.get_map();
                    match op {
                        SnapshotOp::Map {
                            key,
                            value_idx_plus_one,
                        } => Op::new(
                            id,
                            InnerContent::Map(InnerMapSet {
                                key: (&*keys[key]).into(),
                                value: value_idx_plus_one - 1,
                            }),
                            container_idx,
                        ),
                        _ => unreachable!(),
                    }
                }
            };
            *counter_mut += op.content_len() as Counter;
            ops.push(op);
        }

        // calc deps
        let mut deps: smallvec::SmallVec<[ID; 2]> = smallvec![];
        if dep_on_self {
            assert!(start_counter > 0);
            deps.push(ID::new(peer_id, start_counter - 1));
        }

        for _ in 0..deps_len {
            let dep = dep_iter.next().unwrap();
            let peer = common.peer_ids[dep.peer_idx as usize];
            deps.push(ID::new(peer, dep.counter));
        }

        changes.push(Change {
            deps: Frontiers::from(deps),
            ops,
            timestamp,
            id: ID::new(peer_id, start_counter),
            lamport: 0, // calculate lamport when importing
        });
    }

    // we assume changes are already sorted by lamport already
    for mut change in changes {
        let lamport = oplog.dag.frontiers_to_next_lamport(&change.deps);
        change.lamport = lamport;
        oplog.import_local_change(change)?;
    }

    Ok(())
}

pub fn decode_state(app_state: &mut AppState, data: &FinalPhase) -> Result<(), LoroError> {
    assert!(app_state.is_empty());
    assert!(!app_state.is_in_txn());
    let arena = app_state.arena.clone();
    let common = CommonArena::decode(&data)?;
    let state_arena = TempArena::decode_state_arena(&data)?;
    let encoded_app_state = EncodedAppState::decode(&data)?;
    app_state.frontiers = Frontiers::from(&encoded_app_state.frontiers);
    let mut text_index = 0;
    // this part should be moved to encode.rs in preload
    for ((id, parent), state) in common
        .container_ids
        .iter()
        .zip(encoded_app_state.parents.iter())
        .zip(encoded_app_state.states.iter())
    {
        let idx = arena.register_container(id);
        let parent_idx =
            (*parent).map(|x| ContainerIdx::from_index_and_type(x, state.container_type()));
        arena.set_parent(idx, parent_idx);
        match state {
            loro_preload::EncodedContainerState::Text { len } => {
                let index = text_index;
                app_state.set_state(
                    idx,
                    State::TextState(TextState::from_str(
                        std::str::from_utf8(&state_arena.text[index..index + len]).unwrap(),
                    )),
                );
                text_index += len;
            }
            loro_preload::EncodedContainerState::Map(map_data) => {
                let mut map = MapState::new();
                for entry in map_data.iter() {
                    map.insert(
                        InternalString::from(&*state_arena.keywords[entry.key]),
                        MapValue {
                            counter: entry.counter as Counter,
                            value: if entry.value == 0 {
                                None
                            } else {
                                Some(state_arena.values[entry.value as usize - 1].clone())
                            },
                            lamport: (entry.lamport, common.peer_ids[entry.peer as usize]),
                        },
                    )
                }
                app_state.set_state(idx, State::MapState(map));
            }
            loro_preload::EncodedContainerState::List(list_data) => {
                let mut list = ListState::new();
                list.insert_batch(0, list_data.iter().map(|&x| state_arena.values[x].clone()));
                app_state.set_state(idx, State::ListState(list));
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_snapshot_encode() {
        use std::borrow::Cow;

        dbg!(FinalPhase {
            common: Cow::Owned(vec![0, 1, 2, 253, 254, 255]),
            app_state: Cow::Owned(vec![255]),
            state_arena: Cow::Owned(vec![255]),
            additional_arena: Cow::Owned(vec![255]),
            oplog: Cow::Owned(vec![255]),
        }
        .encode());
    }

    #[test]
    fn text_edit_snapshot_encode_decode() {
        // test import snapshot directly
        let mut app = LoroApp::new();
        let mut txn = app.txn().unwrap();
        let text = txn.get_text("id");
        text.insert(&mut txn, 0, "hello");
        txn.commit();
        let snapshot = app.export_snapshot();
        let mut app2 = LoroApp::new();
        app2.import(&snapshot);
        let actual = app2
            .app_state()
            .lock()
            .unwrap()
            .get_text("id")
            .unwrap()
            .to_string();
        assert_eq!("hello", &actual);

        // test import snapshot to a LoroApp that is already changed
        let mut txn = app2.txn().unwrap();
        let text = txn.get_text("id");
        text.insert(&mut txn, 2, " ");
        txn.commit();
        debug_log::group!("app2 export");
        let snapshot = app2.export_snapshot();
        debug_log::group_end!();
        debug_log::group!("import snapshot to a LoroApp that is already changed");
        app.import(&snapshot).unwrap();
        debug_log::group_end!();
        let actual = app
            .app_state()
            .lock()
            .unwrap()
            .get_text("id")
            .unwrap()
            .to_string();
        assert_eq!("he llo", &actual);
    }
}

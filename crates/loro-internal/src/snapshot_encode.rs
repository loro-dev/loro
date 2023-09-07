use std::borrow::Cow;

use fxhash::FxHashMap;
use itertools::Itertools;
use loro_common::{ContainerType, HasLamport, ID};
use loro_preload::{
    CommonArena, EncodedAppState, EncodedContainerState, FinalPhase, MapEntry, TempArena,
};
use rle::{HasLength, RleVec};
use serde::{Deserialize, Serialize};
use serde_columnar::{columnar, to_vec};
use smallvec::smallvec;

use crate::{
    change::{Change, Timestamp},
    container::{idx::ContainerIdx, list::list_op::InnerListOp, map::InnerMapSet},
    delta::MapValue,
    id::{Counter, PeerID},
    op::{InnerContent, Op},
    version::Frontiers,
    InternalString, LoroError, LoroValue,
};

use super::{
    arena::SharedArena,
    loro::LoroDoc,
    oplog::OpLog,
    state::{DocState, ListState, MapState, State, TextState},
};

pub fn encode_app_snapshot(app: &LoroDoc) -> Vec<u8> {
    let pre_encoded_state = preprocess_app_state(&app.app_state().lock().unwrap());
    let f = encode_oplog(&app.oplog().lock().unwrap(), Some(pre_encoded_state));
    // f.diagnose_size();
    miniz_oxide::deflate::compress_to_vec(&f.encode(), 6)
}

pub fn decode_app_snapshot(app: &LoroDoc, bytes: &[u8], with_state: bool) -> Result<(), LoroError> {
    assert!(app.is_empty());
    let bytes = miniz_oxide::inflate::decompress_to_vec(bytes)
        .map_err(|_| LoroError::DecodeError("".into()))?;
    let data = FinalPhase::decode(&bytes)?;
    if with_state {
        let mut app_state = app.app_state().lock().unwrap();
        let (state_arena, common) = decode_state(&mut app_state, &data)?;
        let arena = app_state.arena.clone();
        decode_oplog(
            &mut app.oplog().lock().unwrap(),
            &data,
            Some((arena, state_arena, common)),
        )?;
    } else {
        decode_oplog(&mut app.oplog().lock().unwrap(), &data, None)?;
    }
    Ok(())
}

pub fn decode_oplog(
    oplog: &mut OpLog,
    data: &FinalPhase,
    arena: Option<(SharedArena, TempArena, CommonArena)>,
) -> Result<(), LoroError> {
    let (arena, state_arena, common) = arena.unwrap_or_else(|| {
        (
            Default::default(),
            TempArena::decode_state_arena(data).unwrap(),
            CommonArena::decode(data).unwrap(),
        )
    });
    oplog.arena = arena.clone();
    let mut extra_arena = TempArena::decode_additional_arena(data)?;
    arena.alloc_str_fast(&extra_arena.text);
    arena.alloc_values(state_arena.values.into_iter());
    arena.alloc_values(extra_arena.values.into_iter());
    let mut keys = state_arena.keywords;
    keys.append(&mut extra_arena.keywords);

    let oplog_data = OplogEncoded::decode(data)?;

    let mut changes = Vec::new();
    let mut dep_iter = oplog_data.deps.iter();
    let mut op_iter = oplog_data.ops.iter();
    let mut counters = FxHashMap::default();
    let mut text_idx = 0;
    for change in oplog_data.changes.iter() {
        let peer_idx = change.peer_idx as usize;
        let peer_id = common.peer_ids[peer_idx];
        let timestamp = change.timestamp;
        let deps_len = change.deps_len;
        let dep_on_self = change.dep_on_self;
        let mut ops = RleVec::new();
        let counter_mut = counters.entry(peer_idx).or_insert(0);
        let start_counter = *counter_mut;

        // decode ops
        for _ in 0..change.op_len {
            let id = ID::new(peer_id, *counter_mut);
            let encoded_op = op_iter.next().unwrap();
            let container = common.container_ids[encoded_op.container as usize].clone();
            let container_idx = arena.register_container(&container);
            let op = match container.container_type() {
                loro_common::ContainerType::Text | loro_common::ContainerType::List => {
                    let op = if container.container_type() == ContainerType::List {
                        encoded_op.get_list()
                    } else {
                        encoded_op.get_text()
                    };
                    match op {
                        SnapshotOp::ListInsert {
                            value_idx: start,
                            pos,
                        } => Op::new(
                            id,
                            InnerContent::List(InnerListOp::new_insert(start..start + 1, pos)),
                            container_idx,
                        ),
                        SnapshotOp::TextOrListDelete { len, pos } => Op::new(
                            id,
                            InnerContent::List(InnerListOp::new_del(pos, len)),
                            container_idx,
                        ),
                        SnapshotOp::TextOrListUnknown { len, pos } => Op::new(
                            id,
                            InnerContent::List(InnerListOp::new_unknown(pos, len)),
                            container_idx,
                        ),
                        SnapshotOp::Map { .. } => {
                            unreachable!()
                        }
                        SnapshotOp::TextInsert { pos, len } => {
                            let op = Op::new(
                                id,
                                InnerContent::List(InnerListOp::new_insert(
                                    text_idx..text_idx + (len as u32),
                                    pos,
                                )),
                                container_idx,
                            );
                            text_idx += len as u32;
                            op
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
        let mut deps: smallvec::SmallVec<[ID; 1]> = smallvec![];
        if dep_on_self {
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

pub fn decode_state<'b>(
    app_state: &'_ mut DocState,
    data: &'b FinalPhase,
) -> Result<(TempArena<'b>, CommonArena<'b>), LoroError> {
    assert!(app_state.is_empty());
    assert!(!app_state.is_in_txn());
    let arena = app_state.arena.clone();
    let common = CommonArena::decode(data)?;
    let state_arena = TempArena::decode_state_arena(data)?;
    let encoded_app_state = EncodedAppState::decode(data)?;
    let mut text_index = 0;
    let mut container_states =
        FxHashMap::with_capacity_and_hasher(common.container_ids.len(), Default::default());
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
                container_states.insert(
                    idx,
                    State::TextState(TextState::from_str(
                        std::str::from_utf8(&state_arena.text[index..index + len]).unwrap(),
                    )),
                );
                text_index += len;
            }
            loro_preload::EncodedContainerState::Map(map_data) => {
                let mut map = MapState::new(idx);
                for entry in map_data.iter() {
                    map.insert(
                        InternalString::from(&*state_arena.keywords[entry.key]),
                        MapValue {
                            counter: entry.counter as Counter,
                            value: if entry.value == 0 {
                                None
                            } else {
                                Some(state_arena.values[entry.value - 1].clone())
                            },
                            lamport: (entry.lamport, common.peer_ids[entry.peer as usize]),
                        },
                    )
                }
                container_states.insert(idx, State::MapState(map));
            }
            loro_preload::EncodedContainerState::List(list_data) => {
                let mut list = ListState::new(idx);
                list.insert_batch(
                    0,
                    list_data
                        .iter()
                        .map(|&x| state_arena.values[x].clone())
                        .collect_vec(),
                );
                container_states.insert(idx, State::ListState(list));
            }
        }
    }

    let frontiers = Frontiers::from(&encoded_app_state.frontiers);
    app_state.init_with_states_and_version(container_states, frontiers);
    Ok((state_arena, common))
}

type ClientIdx = u32;

#[columnar(ser, de)]
#[derive(Debug, Serialize, Deserialize)]
struct OplogEncoded {
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
struct EncodedChange {
    #[columnar(strategy = "Rle", original_type = "u32")]
    pub(super) peer_idx: ClientIdx,
    #[columnar(strategy = "DeltaRle", original_type = "i64")]
    pub(super) timestamp: Timestamp,
    #[columnar(strategy = "Rle")]
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
    // Text: insert len | del len (can be neg)
    // List: 0 | del len (can be neg)
    // Map: always 0
    #[columnar(strategy = "DeltaRle")]
    len: i64,
    // List: insert 0 | unkonwn -2 | deletion -1
    // Text: insert 0 | unkonwn -2 | deletion -1
    // Map: always 0
    #[columnar(strategy = "Rle")]
    kind: i64,
    // Text: 0
    // List: 0 | value index
    // Map: value index
    #[columnar(strategy = "DeltaRle")]
    value: usize,
}

enum SnapshotOp {
    TextInsert { pos: usize, len: usize },
    ListInsert { pos: usize, value_idx: u32 },
    TextOrListDelete { pos: usize, len: isize },
    TextOrListUnknown { pos: usize, len: usize },
    Map { key: usize, value_idx_plus_one: u32 },
}

impl EncodedSnapshotOp {
    pub fn get_text(&self) -> SnapshotOp {
        if self.kind == -1 {
            SnapshotOp::TextOrListDelete {
                pos: self.prop,
                len: self.len as isize,
            }
        } else if self.kind == -2 {
            SnapshotOp::TextOrListUnknown {
                pos: self.prop,
                len: self.len as usize,
            }
        } else {
            SnapshotOp::TextInsert {
                pos: self.prop,
                len: self.len as usize,
            }
        }
    }

    pub fn get_list(&self) -> SnapshotOp {
        if self.kind == -1 {
            SnapshotOp::TextOrListDelete {
                pos: self.prop,
                len: self.len as isize,
            }
        } else if self.kind == -2 {
            SnapshotOp::TextOrListUnknown {
                pos: self.prop,
                len: self.len as usize,
            }
        } else {
            SnapshotOp::ListInsert {
                pos: self.prop,
                value_idx: self.value as u32,
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
            SnapshotOp::ListInsert {
                pos,
                value_idx: start,
            } => Self {
                container,
                prop: pos,
                len: 0,
                kind: 0,
                value: start as usize,
            },
            SnapshotOp::TextOrListDelete { pos, len } => Self {
                container,
                prop: pos,
                len: len as i64,
                kind: -1,
                value: 0,
            },
            SnapshotOp::TextOrListUnknown { pos, len } => Self {
                container,
                prop: pos,
                len: len as i64,
                kind: -2,
                value: 0,
            },
            SnapshotOp::Map {
                key,
                value_idx_plus_one: value,
            } => Self {
                container,
                prop: key,
                len: 0,
                kind: 0,
                value: value as usize,
            },
            SnapshotOp::TextInsert { pos, len } => Self {
                container,
                prop: pos,
                len: len as i64,
                kind: 0,
                value: 0,
            },
        }
    }
}

#[columnar(vec, ser, de)]
#[derive(Debug, Copy, Clone, Serialize, Deserialize)]
struct DepsEncoding {
    #[columnar(strategy = "Rle", original_type = "u32")]
    peer_idx: ClientIdx,
    #[columnar(strategy = "DeltaRle", original_type = "i32")]
    counter: Counter,
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

fn preprocess_app_state(app_state: &DocState) -> PreEncodedState {
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

    for (_, state) in app_state.states.iter() {
        match state {
            State::ListState(list) => {
                let v = list.iter().map(|value| record_value(value)).collect();
                encoded.states.push(EncodedContainerState::List(v))
            }
            State::MapState(map) => {
                let v = map
                    .iter()
                    .map(|(key, value)| {
                        let key = record_key(key);
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

    let common = CommonArena {
        peer_ids: peers.into(),
        container_ids: app_state.arena.export_containers(),
    };

    let arena = TempArena {
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
        app_state,
    } = state_ref;
    if common.container_ids.is_empty() {
        common.container_ids = oplog.arena.export_containers();
    }
    // need to rebuild bytes from ops, because arena.text may contain garbage
    let mut bytes = Vec::with_capacity(arena.text.len());
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

    let Cow::Owned(mut peers) = std::mem::take(&mut common.peer_ids) else {
        unreachable!()
    };
    let mut record_peer = |peer: PeerID| {
        if let Some(idx) = peer_lookup.get(&peer) {
            return *idx as u32;
        }

        peers.push(peer);
        peer_lookup.entry(peer).or_insert_with(|| peers.len() - 1);
        peers.len() as u32 - 1
    };

    let mut record_str = |s: &[u8], pos: usize, container_idx: u32| {
        bytes.extend_from_slice(s);
        let ans = SnapshotOp::TextInsert { pos, len: s.len() };
        EncodedSnapshotOp::from(ans, container_idx)
    };

    // Add all changes
    let mut changes: Vec<&Change> = Vec::with_capacity(oplog.len_changes());
    for (_, peer_changes) in oplog.changes.iter() {
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
        let op_index_start = encoded_ops.len();
        for op in change.ops.iter() {
            match &op.content {
                InnerContent::List(list) => match list {
                    InnerListOp::Insert { slice, pos } => {
                        if slice.is_unknown() {
                            encoded_ops.push(EncodedSnapshotOp::from(
                                SnapshotOp::TextOrListUnknown {
                                    pos: *pos,
                                    len: slice.atom_len(),
                                },
                                op.container.to_index(),
                            ));
                        } else {
                            match op.container.get_type() {
                                loro_common::ContainerType::Text => {
                                    let range = slice.0.start as usize..slice.0.end as usize;
                                    let mut pos = *pos;
                                    oplog.arena.with_text_slice(range, |slice| {
                                        encoded_ops.push(record_str(
                                            slice.as_bytes(),
                                            pos,
                                            op.container.to_index(),
                                        ));

                                        pos += slice.chars().count();
                                    })
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
                                                value_idx: idx as u32,
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
                            SnapshotOp::TextOrListDelete {
                                pos: del.pos as usize,
                                len: del.len,
                            },
                            op.container.to_index(),
                        ));
                    }
                    InnerListOp::Style {
                        start,
                        end,
                        key,
                        info,
                    } => unimplemented!("style encode"),
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
        oplog_extra_arena: Cow::Owned(
            TempArena {
                text: Cow::Borrowed(&bytes),
                keywords: extra_keys,
                values: extra_values,
            }
            .encode(),
        ),
        oplog: Cow::Owned(oplog_encoded.encode()),
    };

    ans
}

#[cfg(test)]
mod test {
    use debug_log::debug_dbg;

    use super::*;

    #[test]
    fn test_snapshot_encode() {
        use std::borrow::Cow;

        FinalPhase {
            common: Cow::Owned(vec![0, 1, 2, 253, 254, 255]),
            app_state: Cow::Owned(vec![255]),
            state_arena: Cow::Owned(vec![255]),
            oplog_extra_arena: Cow::Owned(vec![255]),
            oplog: Cow::Owned(vec![255]),
        }
        .encode();
    }

    #[test]
    fn text_edit_snapshot_encode_decode() {
        // test import snapshot directly
        let app = LoroDoc::new();
        let mut txn = app.txn().unwrap();
        let text = txn.get_text("id");
        text.insert(&mut txn, 0, "hello").unwrap();
        txn.commit().unwrap();
        let snapshot = app.export_snapshot();
        let app2 = LoroDoc::new();
        app2.import(&snapshot).unwrap();
        let actual = app2
            .app_state()
            .lock()
            .unwrap()
            .get_text("id")
            .unwrap()
            .to_string();
        assert_eq!("hello", &actual);
        debug_dbg!(&app2.oplog().lock().unwrap());

        // test import snapshot to a LoroApp that is already changed
        let mut txn = app2.txn().unwrap();
        let text = txn.get_text("id");
        text.insert(&mut txn, 2, " ").unwrap();
        txn.commit().unwrap();
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

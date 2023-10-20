use std::{borrow::Cow, ops::Deref};

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
    container::{
        idx::ContainerIdx, list::list_op::InnerListOp, map::InnerMapSet,
        richtext::TextStyleInfoFlag,
    },
    delta::MapValue,
    id::{Counter, PeerID},
    op::{InnerContent, Op},
    state::RichtextState,
    version::Frontiers,
    InternalString, LoroError, LoroValue,
};

use super::{
    arena::SharedArena,
    loro::LoroDoc,
    oplog::OpLog,
    state::{DocState, ListState, MapState, State},
};

pub fn encode_app_snapshot(app: &LoroDoc) -> Vec<u8> {
    let state = app.app_state().lock().unwrap();
    let pre_encoded_state = preprocess_app_state(&state);
    let f = encode_oplog(&app.oplog().lock().unwrap(), Some(pre_encoded_state));
    // f.diagnose_size();
    f.encode()
}

pub fn decode_app_snapshot(app: &LoroDoc, bytes: &[u8], with_state: bool) -> Result<(), LoroError> {
    assert!(app.is_empty());
    let data = FinalPhase::decode(bytes)?;
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
        let arena = SharedArena::default();
        let state_arena = TempArena::decode_state_arena(data).unwrap();
        arena.alloc_str_fast(&state_arena.text);
        (arena, state_arena, CommonArena::decode(data).unwrap())
    });
    oplog.arena = arena.clone();
    let mut extra_arena = TempArena::decode_additional_arena(data)?;
    arena.alloc_str_fast(&extra_arena.text);
    arena.alloc_values(state_arena.values.into_iter());
    arena.alloc_values(extra_arena.values.into_iter());
    let mut keys = state_arena.keywords;
    keys.append(&mut extra_arena.keywords);

    let oplog_data = OplogEncoded::decode_iter(data)?;
    let mut style_iter = oplog_data.styles.iter();
    let mut changes = Vec::new();
    let mut dep_iter = oplog_data.deps;
    let mut op_iter = oplog_data.ops;
    let mut counters = FxHashMap::default();
    for change in oplog_data.changes {
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
                loro_common::ContainerType::List | loro_common::ContainerType::Text => {
                    let op = match container.container_type() {
                        ContainerType::List => encoded_op.get_list(),
                        ContainerType::Text => encoded_op.get_richtext(),
                        _ => unreachable!(),
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
                        SnapshotOp::Map { .. } => {
                            unreachable!()
                        }
                        SnapshotOp::RichtextStyleStart { start, end } => {
                            let style = style_iter.next().unwrap();
                            let key = keys[style.key_idx as usize].clone();
                            let info = style.info;
                            Op::new(
                                id,
                                InnerContent::List(InnerListOp::StyleStart {
                                    start: start as u32,
                                    end: end as u32,
                                    key,
                                    info: TextStyleInfoFlag::from_byte(info),
                                }),
                                container_idx,
                            )
                        }
                        SnapshotOp::RichtextStyleEnd => {
                            Op::new(id, InnerContent::List(InnerListOp::StyleEnd), container_idx)
                        }
                        SnapshotOp::RichtextInsert { pos, start, len } => Op::new(
                            id,
                            InnerContent::List(InnerListOp::new_insert(
                                start as u32..start as u32 + (len as u32),
                                pos,
                            )),
                            container_idx,
                        ),
                    }
                }
                loro_common::ContainerType::Map => {
                    let op = encoded_op.get_map();
                    match op {
                        SnapshotOp::Map {
                            key,
                            value_idx_plus_one,
                        } => {
                            let value = if value_idx_plus_one == 0 {
                                None
                            } else {
                                Some(value_idx_plus_one - 1)
                            };
                            Op::new(
                                id,
                                InnerContent::Map(InnerMapSet {
                                    key: (&*keys[key]).into(),
                                    value,
                                }),
                                container_idx,
                            )
                        }
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
    arena.alloc_str_fast(&state_arena.text);
    let encoded_app_state = EncodedAppState::decode(data)?;
    let mut container_states =
        FxHashMap::with_capacity_and_hasher(common.container_ids.len(), Default::default());
    // this part should be moved to encode.rs in preload
    for ((id, parent), state) in common
        .container_ids
        .iter()
        .zip(encoded_app_state.parents.iter())
        .zip(encoded_app_state.states.into_iter())
    {
        let idx = arena.register_container(id);
        let parent_idx =
            (*parent).map(|x| ContainerIdx::from_index_and_type(x, state.container_type()));
        arena.set_parent(idx, parent_idx);
        match state {
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
            loro_preload::EncodedContainerState::Richtext(richtext_data) => {
                let mut richtext = RichtextState::new(idx);
                richtext.decode_snapshot(richtext_data, &state_arena, &common, &arena);
                container_states.insert(idx, State::RichtextState(richtext));
            }
        }
    }

    let frontiers = Frontiers::from(&encoded_app_state.frontiers);
    app_state.init_with_states_and_version(container_states, frontiers);
    Ok((state_arena, common))
}

type ClientIdx = u32;

#[columnar(ser, de)]
#[derive(Debug)]
struct OplogEncoded {
    #[columnar(class = "vec", iter = "EncodedChange")]
    pub(crate) changes: Vec<EncodedChange>,
    #[columnar(class = "vec", iter = "EncodedSnapshotOp")]
    ops: Vec<EncodedSnapshotOp>,
    #[columnar(class = "vec", iter = "DepsEncoding")]
    deps: Vec<DepsEncoding>,

    styles: Vec<StyleInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StyleInfo {
    key_idx: u32,
    info: u8,
}

impl OplogEncoded {
    fn decode_iter<'f: 'iter, 'iter>(
        data: &'f FinalPhase,
    ) -> Result<<Self as TableIter<'iter>>::Iter, LoroError> {
        serde_columnar::iter_from_bytes::<Self>(&data.oplog)
            .map_err(|e| LoroError::DecodeError(e.to_string().into_boxed_str()))
    }

    fn encode(&self) -> Vec<u8> {
        to_vec(self).unwrap()
    }
}

#[columnar(vec, ser, de, iterable)]
#[derive(Debug, Clone)]
struct EncodedChange {
    #[columnar(strategy = "Rle")]
    pub(super) peer_idx: ClientIdx,
    #[columnar(strategy = "DeltaRle")]
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

#[columnar(vec, ser, de, iterable)]
#[derive(Debug, Clone)]
struct EncodedSnapshotOp {
    #[columnar(strategy = "Rle")]
    container: u32,
    /// key index or insert/delete pos
    #[columnar(strategy = "DeltaRle")]
    prop: usize,
    /// Richtext: insert range start
    /// Text: 0
    /// List: 0
    /// Map: 0
    #[columnar(strategy = "DeltaRle")]
    prop2: usize,
    /// Richtext: insert len | del len | end position (for style)
    /// Text: insert len | del len (can be neg)
    /// List: 0 | del len (can be neg)
    /// Map: always 0
    #[columnar(strategy = "DeltaRle")]
    len: i64,
    #[columnar(strategy = "BoolRle")]
    is_del: bool,
    /// Richtext: 0 (text) | 1 (style_start) | 2 (style_end)
    /// Text: 0
    /// List: 0 | value index
    /// Map: 0 (deleted) | value index + 1
    #[columnar(strategy = "DeltaRle")]
    value: isize,
}

enum SnapshotOp {
    RichtextStyleStart {
        start: usize,
        end: usize,
    },
    RichtextStyleEnd,
    RichtextInsert {
        pos: usize,
        start: usize,
        len: usize,
    },
    ListInsert {
        pos: usize,
        value_idx: u32,
    },
    TextOrListDelete {
        pos: usize,
        len: isize,
    },
    Map {
        key: usize,
        value_idx_plus_one: u32,
    },
}

impl EncodedSnapshotOp {
    pub fn get_richtext(&self) -> SnapshotOp {
        if self.is_del {
            SnapshotOp::TextOrListDelete {
                pos: self.prop,
                len: self.len as isize,
            }
        } else {
            match self.value {
                0 => SnapshotOp::RichtextInsert {
                    pos: self.prop,
                    start: self.prop2,
                    len: self.len as usize,
                },
                1 => SnapshotOp::RichtextStyleStart {
                    start: self.prop,
                    end: self.len as usize,
                },
                2 => SnapshotOp::RichtextStyleEnd,
                _ => unreachable!(),
            }
        }
    }

    pub fn get_list(&self) -> SnapshotOp {
        if self.is_del {
            SnapshotOp::TextOrListDelete {
                pos: self.prop,
                len: self.len as isize,
            }
        } else {
            SnapshotOp::ListInsert {
                pos: self.prop,
                value_idx: self.value as u32,
            }
        }
    }

    pub fn get_map(&self) -> SnapshotOp {
        let value_idx_plus_one = if self.value < 0 { 0 } else { self.value as u32 };
        SnapshotOp::Map {
            key: self.prop,
            value_idx_plus_one,
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
                prop2: 0,
                len: 0,
                is_del: false,
                value: start as isize,
            },
            SnapshotOp::TextOrListDelete { pos, len } => Self {
                container,
                prop: pos,
                prop2: 0,
                len: len as i64,
                is_del: true,
                value: 0,
            },
            SnapshotOp::Map {
                key,
                value_idx_plus_one: value,
            } => {
                let value = if value == 0 { -1 } else { value as isize };
                Self {
                    container,
                    prop: key,
                    prop2: 0,
                    len: 0,
                    is_del: false,
                    value,
                }
            }
            SnapshotOp::RichtextStyleStart { start, end } => Self {
                container,
                prop: start,
                prop2: 0,
                len: end as i64,
                is_del: false,
                value: 1,
            },
            SnapshotOp::RichtextStyleEnd => Self {
                container,
                prop: 0,
                prop2: 0,
                len: 0,
                is_del: false,
                value: 2,
            },
            SnapshotOp::RichtextInsert { pos, start, len } => Self {
                container,
                prop: pos,
                prop2: start,
                len: len as i64,
                is_del: false,
                value: 0,
            },
        }
    }
}

#[columnar(vec, ser, de, iterable)]
#[derive(Debug, Copy, Clone)]
struct DepsEncoding {
    #[columnar(strategy = "Rle")]
    peer_idx: ClientIdx,
    #[columnar(strategy = "DeltaRle")]
    counter: Counter,
}

#[derive(Default)]
struct PreEncodedState<'a> {
    common: CommonArena<'static>,
    arena: TempArena<'static>,
    key_lookup: FxHashMap<InternalString, usize>,
    value_lookup: FxHashMap<LoroValue, usize>,
    peer_lookup: FxHashMap<PeerID, usize>,
    app_state: EncodedAppState<'a>,
}

fn preprocess_app_state(app_state: &DocState) -> PreEncodedState {
    assert!(!app_state.is_in_txn());
    let mut peers = Vec::new();
    let mut peer_lookup = FxHashMap::default();
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
                let v = list.iter().map(&mut record_value).collect();
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
            State::RichtextState(text) => {
                let result = text.encode_snapshot(&mut record_peer, &mut record_key);
                encoded.states.push(EncodedContainerState::Richtext(result));
            }
        }
    }

    let common = CommonArena {
        peer_ids: peers.into(),
        container_ids: app_state.arena.export_containers(),
    };

    let arena = TempArena {
        values,
        keywords,
        text: app_state.arena.slice_by_unicode(..).deref().to_vec().into(),
    };

    // debug_log::debug_dbg!(&encoded);
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

    let mut styles = Vec::new();
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
                    InnerListOp::Insert { slice, pos } => match op.container.get_type() {
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
                        loro_common::ContainerType::Text => {
                            encoded_ops.push(EncodedSnapshotOp::from(
                                SnapshotOp::RichtextInsert {
                                    pos: *pos,
                                    start: slice.0.start as usize,
                                    len: slice.0.len(),
                                },
                                op.container.to_index(),
                            ))
                        }
                        loro_common::ContainerType::Map => unreachable!(),
                    },
                    InnerListOp::Delete(del) => {
                        encoded_ops.push(EncodedSnapshotOp::from(
                            SnapshotOp::TextOrListDelete {
                                pos: del.pos as usize,
                                len: del.signed_len,
                            },
                            op.container.to_index(),
                        ));
                    }
                    InnerListOp::StyleStart {
                        start,
                        end,
                        key,
                        info,
                    } => {
                        encoded_ops.push(EncodedSnapshotOp::from(
                            SnapshotOp::RichtextStyleStart {
                                start: *start as usize,
                                end: *end as usize,
                            },
                            op.container.to_index(),
                        ));
                        styles.push(StyleInfo {
                            key_idx: record_key(key) as u32,
                            info: info.to_byte(),
                        })
                    }
                    InnerListOp::StyleEnd => {
                        encoded_ops.push(EncodedSnapshotOp::from(
                            SnapshotOp::RichtextStyleEnd,
                            op.container.to_index(),
                        ));
                    }
                },
                InnerContent::Map(map) => {
                    let key = record_key(&map.key);
                    let value = map.value.and_then(|v| oplog.arena.get_value(v as usize));
                    let value = if let Some(value) = value {
                        (record_value(&value) + 1) as u32
                    } else {
                        0
                    };
                    encoded_ops.push(EncodedSnapshotOp::from(
                        SnapshotOp::Map {
                            key,
                            value_idx_plus_one: value,
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
        styles,
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
                text: Cow::Borrowed(&[]),
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

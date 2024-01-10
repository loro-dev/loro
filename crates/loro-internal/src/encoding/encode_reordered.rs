use std::{borrow::Cow, cmp::Ordering, mem::take, sync::Arc};

use fxhash::{FxHashMap, FxHashSet};
use itertools::Itertools;
use loro_common::{
    ContainerID, ContainerType, Counter, HasCounterSpan, HasIdSpan, HasLamportSpan, IdSpan,
    InternalString, LoroError, LoroResult, PeerID, ID,
};
use num_traits::FromPrimitive;
use rle::HasLength;
use serde_columnar::columnar;

use crate::{
    arena::SharedArena,
    change::Change,
    container::{idx::ContainerIdx, list::list_op::DeleteSpan, richtext::TextStyleInfoFlag},
    encoding::{
        encode_reordered::value::{ValueKind, ValueWriter},
        StateSnapshotDecodeContext,
    },
    op::{Op, OpWithId, SliceRange},
    state::ContainerState,
    utils::id_int_map::IdIntMap,
    version::Frontiers,
    DocState, LoroDoc, OpLog, VersionVector,
};

use self::{
    arena::{decode_arena, encode_arena, ContainerArena, DecodedArenas},
    encode::{encode_changes, encode_ops, init_encode, TempOp, ValueRegister},
    value::ValueReader,
};

/// If any section of the document is longer than this, we will not decode it.
/// It will return an data corruption error instead.
const MAX_DECODED_SIZE: usize = 1 << 30;
/// If any collection in the document is longer than this, we will not decode it.
/// It will return an data corruption error instead.
const MAX_COLLECTION_SIZE: usize = 1 << 28;

pub(crate) fn encode_updates(oplog: &OpLog, vv: &VersionVector) -> Vec<u8> {
    let mut peer_register: ValueRegister<PeerID> = ValueRegister::new();
    let mut key_register: ValueRegister<InternalString> = ValueRegister::new();
    let (start_counters, diff_changes) = init_encode(oplog, vv, &mut peer_register);
    let ExtractedContainer {
        containers,
        cid_idx_pairs: _,
        idx_to_index: container_idx2index,
    } = extract_containers_in_order(
        &mut diff_changes
            .iter()
            .flat_map(|x| x.ops.iter())
            .map(|x| x.container),
        &oplog.arena,
    );
    let mut cid_register: ValueRegister<ContainerID> = ValueRegister::from_existing(containers);
    let mut dep_arena = arena::DepsArena::default();
    let mut value_writer = ValueWriter::new();
    let mut ops: Vec<TempOp> = Vec::new();
    let arena = &oplog.arena;
    let changes = encode_changes(
        &diff_changes,
        &mut dep_arena,
        &mut peer_register,
        &mut |op| ops.push(op),
        &mut key_register,
        &container_idx2index,
    );

    ops.sort_by(move |a, b| {
        a.container_index
            .cmp(&b.container_index)
            .then_with(|| a.prop_that_used_for_sort.cmp(&b.prop_that_used_for_sort))
            .then_with(|| a.peer_idx.cmp(&b.peer_idx))
            .then_with(|| a.lamport.cmp(&b.lamport))
    });

    let encoded_ops = encode_ops(
        ops,
        arena,
        &mut value_writer,
        &mut key_register,
        &mut cid_register,
        &mut peer_register,
    );

    let container_arena = ContainerArena::from_containers(
        cid_register.unwrap_vec(),
        &mut peer_register,
        &mut key_register,
    );

    let frontiers = oplog
        .frontiers()
        .iter()
        .map(|x| (peer_register.register(&x.peer), x.counter))
        .collect();
    let doc = EncodedDoc {
        ops: encoded_ops,
        changes,
        states: Vec::new(),
        start_counters,
        raw_values: Cow::Owned(value_writer.finish()),
        arenas: Cow::Owned(encode_arena(
            peer_register.unwrap_vec(),
            container_arena,
            key_register.unwrap_vec(),
            dep_arena,
            &[],
        )),
        frontiers,
    };

    serde_columnar::to_vec(&doc).unwrap()
}

pub(crate) fn decode_updates(oplog: &mut OpLog, bytes: &[u8]) -> LoroResult<()> {
    let iter = serde_columnar::iter_from_bytes::<EncodedDoc>(bytes)?;
    let DecodedArenas {
        peer_ids,
        containers,
        keys,
        deps,
        state_blob_arena: _,
    } = decode_arena(&iter.arenas)?;
    let ops_map = extract_ops(
        &iter.raw_values,
        iter.ops,
        &oplog.arena,
        &containers,
        &keys,
        &peer_ids,
        false,
    )?
    .ops_map;

    let changes = decode_changes(iter.changes, iter.start_counters, peer_ids, deps, ops_map)?;
    // debug_log::debug_dbg!(&changes);
    let (latest_ids, pending_changes) = import_changes_to_oplog(changes, oplog)?;
    if oplog.try_apply_pending(latest_ids).should_update && !oplog.batch_importing {
        oplog.dag.refresh_frontiers();
    }

    oplog.import_unknown_lamport_pending_changes(pending_changes)?;
    Ok(())
}

fn import_changes_to_oplog(
    changes: Vec<Change>,
    oplog: &mut OpLog,
) -> Result<(Vec<ID>, Vec<Change>), LoroError> {
    let mut pending_changes = Vec::new();
    let mut latest_ids = Vec::new();
    for mut change in changes {
        if change.ctr_end() <= oplog.vv().get(&change.id.peer).copied().unwrap_or(0) {
            // skip included changes
            continue;
        }

        latest_ids.push(change.id_last());
        // calc lamport or pending if its deps are not satisfied
        match oplog.dag.get_change_lamport_from_deps(&change.deps) {
            Some(lamport) => change.lamport = lamport,
            None => {
                pending_changes.push(change);
                continue;
            }
        }

        let Some(change) = oplog.trim_the_known_part_of_change(change) else {
            continue;
        };
        // update dag and push the change
        let mark = oplog.update_dag_on_new_change(&change);
        oplog.next_lamport = oplog.next_lamport.max(change.lamport_end());
        oplog.latest_timestamp = oplog.latest_timestamp.max(change.timestamp);
        oplog.dag.vv.extend_to_include_end_id(ID {
            peer: change.id.peer,
            counter: change.id.counter + change.atom_len() as Counter,
        });
        oplog.insert_new_change(change, mark);
    }
    if !oplog.batch_importing {
        oplog.dag.refresh_frontiers();
    }

    Ok((latest_ids, pending_changes))
}

fn decode_changes<'a>(
    encoded_changes: IterableEncodedChange<'_>,
    mut counters: Vec<i32>,
    peer_ids: arena::PeerIdArena,
    mut deps: impl Iterator<Item = arena::EncodedDep> + 'a,
    mut ops_map: std::collections::HashMap<
        u64,
        Vec<Op>,
        std::hash::BuildHasherDefault<fxhash::FxHasher>,
    >,
) -> LoroResult<Vec<Change>> {
    let mut changes = Vec::with_capacity(encoded_changes.size_hint().0);
    for EncodedChange {
        peer_idx,
        mut len,
        timestamp,
        deps_len,
        dep_on_self,
        msg_len: _,
    } in encoded_changes
    {
        if peer_ids.peer_ids.len() <= peer_idx || counters.len() <= peer_idx {
            return Err(LoroError::DecodeDataCorruptionError);
        }

        let counter = counters[peer_idx];
        counters[peer_idx] += len as Counter;
        let peer = peer_ids.peer_ids[peer_idx];
        let mut change: Change = Change {
            id: ID::new(peer, counter),
            ops: Default::default(),
            deps: Frontiers::with_capacity((deps_len + if dep_on_self { 1 } else { 0 }) as usize),
            lamport: 0,
            timestamp,
            has_dependents: false,
        };

        if dep_on_self {
            if counter <= 0 {
                return Err(LoroError::DecodeDataCorruptionError);
            }

            change.deps.push(ID::new(peer, counter - 1));
        }

        for _ in 0..deps_len {
            let dep = deps.next().ok_or(LoroError::DecodeDataCorruptionError)?;
            change
                .deps
                .push(ID::new(peer_ids.peer_ids[dep.peer_idx], dep.counter));
        }

        let ops = ops_map
            .get_mut(&peer)
            .ok_or(LoroError::DecodeDataCorruptionError)?;
        while len > 0 {
            let op = ops.pop().ok_or(LoroError::DecodeDataCorruptionError)?;
            len -= op.atom_len();
            change.ops.push(op);
        }

        changes.push(change);
    }

    Ok(changes)
}

struct ExtractedOps {
    ops_map: FxHashMap<PeerID, Vec<Op>>,
    ops: Vec<OpWithId>,
    containers: Vec<ContainerID>,
}

fn extract_ops(
    raw_values: &[u8],
    iter: impl Iterator<Item = EncodedOp>,
    arena: &SharedArena,
    containers: &ContainerArena,
    keys: &arena::KeyArena,
    peer_ids: &arena::PeerIdArena,
    should_extract_ops_with_ids: bool,
) -> LoroResult<ExtractedOps> {
    let mut value_reader = ValueReader::new(raw_values);
    let mut ops_map: FxHashMap<PeerID, Vec<Op>> = FxHashMap::default();
    let containers: Vec<_> = containers
        .containers
        .iter()
        .map(|x| x.as_container_id(&keys.keys, &peer_ids.peer_ids))
        .try_collect()?;
    let mut ops = Vec::new();
    for EncodedOp {
        container_index,
        prop,
        peer_idx,
        value_type,
        counter,
    } in iter
    {
        if containers.len() <= container_index as usize
            || peer_ids.peer_ids.len() <= peer_idx as usize
        {
            return Err(LoroError::DecodeDataCorruptionError);
        }

        let cid = &containers[container_index as usize];
        let c_idx = arena.register_container(cid);
        let kind = ValueKind::from_u8(value_type).expect("Unknown value type");
        let content = decode_op(
            cid,
            kind,
            &mut value_reader,
            arena,
            prop,
            keys,
            &peer_ids.peer_ids,
            &containers,
        )?;

        let peer = peer_ids.peer_ids[peer_idx as usize];
        let op = Op {
            counter,
            container: c_idx,
            content,
        };

        if should_extract_ops_with_ids {
            ops.push(OpWithId {
                peer,
                op: op.clone(),
            });
        }

        ops_map.entry(peer).or_default().push(op);
    }

    for (_, ops) in ops_map.iter_mut() {
        // sort op by counter in the reversed order
        ops.sort_by_key(|x| -x.counter);
    }

    Ok(ExtractedOps {
        ops_map,
        ops,
        containers,
    })
}

pub(crate) fn encode_snapshot(oplog: &OpLog, state: &DocState, vv: &VersionVector) -> Vec<u8> {
    assert!(!state.is_in_txn());
    assert_eq!(oplog.frontiers(), &state.frontiers);
    let mut peer_register: ValueRegister<PeerID> = ValueRegister::new();
    let mut key_register: ValueRegister<InternalString> = ValueRegister::new();
    let (start_counters, diff_changes) = init_encode(oplog, vv, &mut peer_register);
    let ExtractedContainer {
        containers,
        cid_idx_pairs: c_pairs,
        idx_to_index: container_idx2index,
    } = extract_containers_in_order(
        &mut state.iter().map(|x| x.container_idx()).chain(
            diff_changes
                .iter()
                .flat_map(|x| x.ops.iter())
                .map(|x| x.container),
        ),
        &oplog.arena,
    );
    let mut cid_register: ValueRegister<ContainerID> = ValueRegister::from_existing(containers);
    let mut dep_arena = arena::DepsArena::default();
    let mut value_writer = ValueWriter::new();
    let mut ops: Vec<TempOp> = Vec::new();

    // This stores the required op positions of each container state.
    // The states can be encoded in these positions in the next step.
    // This data structure stores that mapping from op id to the required total order.
    let mut map_op_to_pos = IdIntMap::new();
    let mut states = Vec::new();
    let mut state_bytes = Vec::new();
    for (_, c_idx) in c_pairs.iter() {
        let container_index = *container_idx2index.get(c_idx).unwrap() as u32;
        let state = match state.get_state(*c_idx) {
            Some(state) if !state.is_state_empty() => state,
            _ => {
                states.push(EncodedStateInfo {
                    container_index,
                    op_len: 0,
                    state_bytes_len: 0,
                });
                continue;
            }
        };

        let mut op_len = 0;
        let bytes = state.encode_snapshot(super::StateSnapshotEncoder {
            check_idspan: &|id_span| {
                if let Some(counter) = vv.intersect_span(id_span) {
                    Err(IdSpan {
                        client_id: id_span.client_id,
                        counter,
                    })
                } else {
                    Ok(())
                }
            },
            encoder_by_op: &mut |op| {
                ops.push(TempOp {
                    op: Cow::Owned(op.op),
                    peer_idx: peer_register.register(&op.peer) as u32,
                    peer_id: op.peer,
                    container_index,
                    prop_that_used_for_sort: -1,
                    // lamport value is fake, but it's only used for sorting and will not be encoded
                    lamport: 0,
                });
            },
            record_idspan: &mut |id_span| {
                op_len += id_span.atom_len();
                map_op_to_pos.insert(id_span);
            },
            mode: super::EncodeMode::Snapshot,
        });

        states.push(EncodedStateInfo {
            container_index,
            op_len: op_len as u32,
            state_bytes_len: bytes.len() as u32,
        });
        state_bytes.extend(bytes);
    }

    let changes = encode_changes(
        &diff_changes,
        &mut dep_arena,
        &mut peer_register,
        &mut |op| {
            let mut count = 0;
            let o_len = op.atom_len();
            ops.extend(map_op_to_pos.split(op).map(|x| {
                count += x.atom_len();
                x
            }));

            debug_assert_eq!(count, o_len);
        },
        &mut key_register,
        &container_idx2index,
    );
    ops.sort_by(move |a, b| {
        a.container_index.cmp(&b.container_index).then_with(|| {
            match (map_op_to_pos.get(a.id()), map_op_to_pos.get(b.id())) {
                (None, None) => a
                    .prop_that_used_for_sort
                    .cmp(&b.prop_that_used_for_sort)
                    .then_with(|| a.peer_idx.cmp(&b.peer_idx))
                    .then_with(|| a.lamport.cmp(&b.lamport)),
                (None, Some(_)) => Ordering::Greater,
                (Some(_), None) => Ordering::Less,
                (Some(a), Some(b)) => a.0.cmp(&b.0),
            }
        })
    });

    let encoded_ops = encode_ops(
        ops,
        &oplog.arena,
        &mut value_writer,
        &mut key_register,
        &mut cid_register,
        &mut peer_register,
    );

    let container_arena = ContainerArena::from_containers(
        cid_register.unwrap_vec(),
        &mut peer_register,
        &mut key_register,
    );

    let frontiers = oplog
        .frontiers()
        .iter()
        .map(|x| (peer_register.register(&x.peer), x.counter))
        .collect();
    let doc = EncodedDoc {
        ops: encoded_ops,
        changes,
        states,
        start_counters,
        raw_values: Cow::Owned(value_writer.finish()),
        arenas: Cow::Owned(encode_arena(
            peer_register.unwrap_vec(),
            container_arena,
            key_register.unwrap_vec(),
            dep_arena,
            &state_bytes,
        )),
        frontiers,
    };

    serde_columnar::to_vec(&doc).unwrap()
}

pub(crate) fn decode_snapshot(doc: &LoroDoc, bytes: &[u8]) -> LoroResult<()> {
    let mut state = doc.app_state().try_lock().map_err(|_| {
        LoroError::DecodeError(
            "decode_snapshot: failed to lock app state"
                .to_string()
                .into_boxed_str(),
        )
    })?;

    state.check_before_decode_snapshot()?;
    let mut oplog = doc.oplog().try_lock().map_err(|_| {
        LoroError::DecodeError(
            "decode_snapshot: failed to lock oplog"
                .to_string()
                .into_boxed_str(),
        )
    })?;

    if !oplog.is_empty() {
        unimplemented!("You can only import snapshot to a empty loro doc now");
    }

    assert!(state.frontiers.is_empty());
    assert!(oplog.frontiers().is_empty());
    let iter = serde_columnar::iter_from_bytes::<EncodedDoc>(bytes)?;
    let DecodedArenas {
        peer_ids,
        containers,
        keys,
        deps,
        state_blob_arena,
    } = decode_arena(&iter.arenas)?;
    let frontiers: Frontiers = iter
        .frontiers
        .iter()
        .map(|x| {
            let peer = peer_ids
                .peer_ids
                .get(x.0)
                .ok_or(LoroError::DecodeDataCorruptionError)?;
            let ans: Result<ID, LoroError> = Ok(ID::new(*peer, x.1));
            ans
        })
        .try_collect()?;

    let ExtractedOps {
        ops_map,
        ops,
        containers,
    } = extract_ops(
        &iter.raw_values,
        iter.ops,
        &oplog.arena,
        &containers,
        &keys,
        &peer_ids,
        true,
    )?;

    decode_snapshot_states(
        &mut state,
        frontiers,
        iter.states,
        containers,
        state_blob_arena,
        ops,
        &oplog,
    )
    .unwrap();
    let changes = decode_changes(iter.changes, iter.start_counters, peer_ids, deps, ops_map)?;
    let (new_ids, pending_changes) = import_changes_to_oplog(changes, &mut oplog)?;
    assert!(pending_changes.is_empty());
    assert_eq!(&state.frontiers, oplog.frontiers());
    if !oplog.pending_changes.is_empty() {
        drop(oplog);
        drop(state);
        // TODO: Fix this origin value
        doc.update_oplog_and_apply_delta_to_state_if_needed(
            |oplog| {
                if oplog.try_apply_pending(new_ids).should_update && !oplog.batch_importing {
                    oplog.dag.refresh_frontiers();
                }

                Ok(())
            },
            "".into(),
        )?;
    }

    Ok(())
}

fn decode_snapshot_states(
    state: &mut DocState,
    frontiers: Frontiers,
    encoded_state_iter: IterableEncodedStateInfo<'_>,
    containers: Vec<ContainerID>,
    state_blob_arena: &[u8],
    ops: Vec<OpWithId>,
    oplog: &std::sync::MutexGuard<'_, OpLog>,
) -> LoroResult<()> {
    let mut state_blob_index: usize = 0;
    let mut ops_index: usize = 0;
    for EncodedStateInfo {
        container_index,
        mut op_len,
        state_bytes_len,
    } in encoded_state_iter
    {
        if op_len == 0 && state_bytes_len == 0 {
            continue;
        }

        if container_index >= containers.len() as u32 {
            return Err(LoroError::DecodeDataCorruptionError);
        }

        let container_id = &containers[container_index as usize];
        let idx = state.arena.register_container(container_id);
        if state_blob_arena.len() < state_blob_index + state_bytes_len as usize {
            return Err(LoroError::DecodeDataCorruptionError);
        }

        let state_bytes =
            &state_blob_arena[state_blob_index..state_blob_index + state_bytes_len as usize];
        state_blob_index += state_bytes_len as usize;

        if ops.len() < ops_index {
            return Err(LoroError::DecodeDataCorruptionError);
        }

        let mut next_ops = ops[ops_index..]
            .iter()
            .skip_while(|x| x.op.container != idx)
            .take_while(|x| {
                if op_len == 0 {
                    false
                } else {
                    op_len -= x.op.atom_len() as u32;
                    ops_index += 1;
                    true
                }
            })
            .cloned();
        state.init_container(
            container_id.clone(),
            StateSnapshotDecodeContext {
                oplog,
                ops: &mut next_ops,
                blob: state_bytes,
                mode: crate::encoding::EncodeMode::Snapshot,
            },
        );
    }

    let s = take(&mut state.states);
    state.init_with_states_and_version(s, frontiers);
    Ok(())
}

mod encode {
    use fxhash::FxHashMap;
    use loro_common::{ContainerID, ContainerType, HasId, PeerID, ID};
    use num_traits::ToPrimitive;
    use rle::{HasLength, Sliceable};
    use std::borrow::Cow;

    use crate::{
        change::{Change, Lamport},
        container::idx::ContainerIdx,
        encoding::encode_reordered::value::{EncodedTreeMove, ValueWriter},
        op::Op,
        InternalString,
    };

    #[derive(Debug)]
    pub(super) struct TempOp<'a> {
        pub op: Cow<'a, Op>,
        pub lamport: Lamport,
        pub peer_idx: u32,
        pub peer_id: PeerID,
        pub container_index: u32,
        /// Prop is fake and will be encoded in the snapshot.
        /// But it will not be used when decoding, because this op is not included in the vv so it's not in the encoded changes.
        pub prop_that_used_for_sort: i32,
    }

    impl TempOp<'_> {
        pub(crate) fn id(&self) -> loro_common::ID {
            loro_common::ID {
                peer: self.peer_id,
                counter: self.op.counter,
            }
        }
    }

    impl HasId for TempOp<'_> {
        fn id_start(&self) -> loro_common::ID {
            ID::new(self.peer_id, self.op.counter)
        }
    }

    impl HasLength for TempOp<'_> {
        #[inline(always)]
        fn atom_len(&self) -> usize {
            self.op.atom_len()
        }

        #[inline(always)]
        fn content_len(&self) -> usize {
            self.op.atom_len()
        }
    }
    impl<'a> generic_btree::rle::HasLength for TempOp<'a> {
        #[inline(always)]
        fn rle_len(&self) -> usize {
            self.op.atom_len()
        }
    }

    impl<'a> generic_btree::rle::Sliceable for TempOp<'a> {
        fn _slice(&self, range: std::ops::Range<usize>) -> TempOp<'a> {
            Self {
                op: if range.start == 0 && range.end == self.op.atom_len() {
                    match &self.op {
                        Cow::Borrowed(o) => Cow::Borrowed(o),
                        Cow::Owned(o) => Cow::Owned(o.clone()),
                    }
                } else {
                    let op = self.op.slice(range.start, range.end);
                    Cow::Owned(op)
                },
                lamport: self.lamport + range.start as Lamport,
                peer_idx: self.peer_idx,
                peer_id: self.peer_id,
                container_index: self.container_index,
                prop_that_used_for_sort: self.prop_that_used_for_sort,
            }
        }
    }

    pub(super) fn encode_ops(
        ops: Vec<TempOp<'_>>,
        arena: &crate::arena::SharedArena,
        value_writer: &mut ValueWriter,
        key_register: &mut ValueRegister<InternalString>,
        cid_register: &mut ValueRegister<ContainerID>,
        peer_register: &mut ValueRegister<u64>,
    ) -> Vec<EncodedOp> {
        let mut encoded_ops = Vec::with_capacity(ops.len());
        for TempOp {
            op,
            peer_idx,
            container_index,
            ..
        } in ops
        {
            let value_type = encode_op(
                &op,
                arena,
                value_writer,
                key_register,
                cid_register,
                peer_register,
            );

            let prop = get_op_prop(&op, key_register);
            encoded_ops.push(EncodedOp {
                container_index,
                peer_idx,
                counter: op.counter,
                prop,
                value_type: value_type.to_u8().unwrap(),
            });
        }
        encoded_ops
    }

    pub(super) fn encode_changes<'a>(
        diff_changes: &'a Vec<Cow<'a, Change>>,
        dep_arena: &mut super::arena::DepsArena,
        peer_register: &mut ValueRegister<u64>,
        push_op: &mut impl FnMut(TempOp<'a>),
        key_register: &mut ValueRegister<InternalString>,
        container_idx2index: &FxHashMap<ContainerIdx, usize>,
    ) -> Vec<EncodedChange> {
        let mut changes: Vec<EncodedChange> = Vec::with_capacity(diff_changes.len());
        for change in diff_changes.iter() {
            let mut dep_on_self = false;
            let mut deps_len = 0;
            for dep in change.deps.iter() {
                if dep.peer == change.id.peer {
                    dep_on_self = true;
                } else {
                    deps_len += 1;
                    dep_arena.push(peer_register.register(&dep.peer), dep.counter);
                }
            }

            let peer_idx = peer_register.register(&change.id.peer);
            changes.push(EncodedChange {
                dep_on_self,
                deps_len,
                peer_idx,
                len: change.atom_len(),
                timestamp: change.timestamp,
                msg_len: 0,
            });

            for (i, op) in change.ops().iter().enumerate() {
                let lamport = i as Lamport + change.lamport();
                push_op(TempOp {
                    op: Cow::Borrowed(op),
                    lamport,
                    prop_that_used_for_sort: get_sorting_prop(op, key_register),
                    peer_idx: peer_idx as u32,
                    peer_id: change.id.peer,
                    container_index: container_idx2index[&op.container] as u32,
                });
            }
        }
        changes
    }

    use crate::{OpLog, VersionVector};
    pub(super) use value_register::ValueRegister;

    use super::{
        value::{MarkStart, Value, ValueKind},
        EncodedChange, EncodedOp,
    };
    mod value_register {
        use fxhash::FxHashMap;

        pub struct ValueRegister<T> {
            map_value_to_index: FxHashMap<T, usize>,
            vec: Vec<T>,
        }

        impl<T: std::hash::Hash + Clone + PartialEq + Eq> ValueRegister<T> {
            pub fn new() -> Self {
                Self {
                    map_value_to_index: FxHashMap::default(),
                    vec: Vec::new(),
                }
            }

            pub fn from_existing(vec: Vec<T>) -> Self {
                let mut map = FxHashMap::with_capacity_and_hasher(vec.len(), Default::default());
                for (i, value) in vec.iter().enumerate() {
                    map.insert(value.clone(), i);
                }

                Self {
                    map_value_to_index: map,
                    vec,
                }
            }

            /// Return the index of the given value. If it does not exist,
            /// insert it and return the new index.
            pub fn register(&mut self, key: &T) -> usize {
                if let Some(index) = self.map_value_to_index.get(key) {
                    *index
                } else {
                    let idx = self.vec.len();
                    self.vec.push(key.clone());
                    self.map_value_to_index.insert(key.clone(), idx);
                    idx
                }
            }

            pub fn contains(&self, key: &T) -> bool {
                self.map_value_to_index.contains_key(key)
            }

            pub fn unwrap_vec(self) -> Vec<T> {
                self.vec
            }
        }
    }

    pub(super) fn init_encode<'a>(
        oplog: &'a OpLog,
        vv: &'_ VersionVector,
        peer_register: &mut ValueRegister<PeerID>,
    ) -> (Vec<i32>, Vec<Cow<'a, Change>>) {
        let self_vv = oplog.vv();
        let start_vv = vv.trim(&oplog.vv());
        let mut start_counters = Vec::new();

        let mut diff_changes: Vec<Cow<'a, Change>> = Vec::new();
        for change in oplog.iter_changes(&start_vv, self_vv) {
            let start_cnt = start_vv.get(&change.id.peer).copied().unwrap_or(0);
            if !peer_register.contains(&change.id.peer) {
                peer_register.register(&change.id.peer);
                start_counters.push(start_cnt);
            }
            if change.id.counter < start_cnt {
                let offset = start_cnt - change.id.counter;
                diff_changes.push(Cow::Owned(change.slice(offset as usize, change.atom_len())));
            } else {
                diff_changes.push(Cow::Borrowed(change));
            }
        }

        diff_changes.sort_by_key(|x| x.lamport);
        (start_counters, diff_changes)
    }

    fn get_op_prop(op: &Op, register_key: &mut ValueRegister<InternalString>) -> i32 {
        match &op.content {
            crate::op::InnerContent::List(list) => match list {
                crate::container::list::list_op::InnerListOp::Insert { pos, .. } => *pos as i32,
                crate::container::list::list_op::InnerListOp::InsertText { pos, .. } => *pos as i32,
                crate::container::list::list_op::InnerListOp::Delete(span) => span.pos as i32,
                crate::container::list::list_op::InnerListOp::StyleStart { start, .. } => {
                    *start as i32
                }
                crate::container::list::list_op::InnerListOp::StyleEnd => 0,
            },
            crate::op::InnerContent::Map(map) => {
                let key = register_key.register(&map.key);
                key as i32
            }
            crate::op::InnerContent::Tree(..) => 0,
        }
    }

    fn get_sorting_prop(op: &Op, register_key: &mut ValueRegister<InternalString>) -> i32 {
        match &op.content {
            crate::op::InnerContent::List(_) => 0,
            crate::op::InnerContent::Map(map) => {
                let key = register_key.register(&map.key);
                key as i32
            }
            crate::op::InnerContent::Tree(..) => 0,
        }
    }

    #[inline]
    fn encode_op(
        op: &Op,
        arena: &crate::arena::SharedArena,
        value_writer: &mut ValueWriter,
        register_key: &mut ValueRegister<InternalString>,
        register_cid: &mut ValueRegister<ContainerID>,
        register_peer: &mut ValueRegister<PeerID>,
    ) -> ValueKind {
        match &op.content {
            crate::op::InnerContent::List(list) => match list {
                crate::container::list::list_op::InnerListOp::Insert { slice, .. } => {
                    assert_eq!(op.container.get_type(), ContainerType::List);
                    let value = arena.get_values(slice.0.start as usize..slice.0.end as usize);
                    value_writer.write_value_content(&value.into(), register_key, register_cid);
                    ValueKind::Array
                }
                crate::container::list::list_op::InnerListOp::InsertText {
                    slice,
                    unicode_start: _,
                    unicode_len: _,
                    ..
                } => {
                    // TODO: refactor this from_utf8 can be done internally without checking
                    value_writer.write(
                        &Value::Str(std::str::from_utf8(slice.as_bytes()).unwrap()),
                        register_key,
                        register_cid,
                    );
                    ValueKind::Str
                }
                crate::container::list::list_op::InnerListOp::Delete(span) => {
                    value_writer.write(
                        &Value::DeleteSeq(span.signed_len as i32),
                        register_key,
                        register_cid,
                    );
                    ValueKind::DeleteSeq
                }
                crate::container::list::list_op::InnerListOp::StyleStart {
                    start,
                    end,
                    key,
                    value,
                    info,
                } => {
                    value_writer.write(
                        &Value::MarkStart(MarkStart {
                            len: end - start,
                            key_idx: register_key.register(key) as u32,
                            value: value.clone(),
                            info: info.to_byte(),
                        }),
                        register_key,
                        register_cid,
                    );
                    ValueKind::MarkStart
                }
                crate::container::list::list_op::InnerListOp::StyleEnd => ValueKind::Null,
            },
            crate::op::InnerContent::Map(map) => {
                assert_eq!(op.container.get_type(), ContainerType::Map);
                match &map.value {
                    Some(v) => value_writer.write_value_content(v, register_key, register_cid),
                    None => ValueKind::DeleteOnce,
                }
            }
            crate::op::InnerContent::Tree(t) => {
                assert_eq!(op.container.get_type(), ContainerType::Tree);
                let op = EncodedTreeMove::from_tree_op(t, register_peer);
                value_writer.write(&Value::TreeMove(op), register_key, register_cid);
                ValueKind::TreeMove
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
#[inline]
fn decode_op(
    cid: &ContainerID,
    kind: ValueKind,
    value_reader: &mut ValueReader<'_>,
    arena: &crate::arena::SharedArena,
    prop: i32,
    keys: &arena::KeyArena,
    peers: &[u64],
    cids: &[ContainerID],
) -> LoroResult<crate::op::InnerContent> {
    let content = match cid.container_type() {
        ContainerType::Text => match kind {
            ValueKind::Str => {
                let s = value_reader.read_str()?;
                let (slice, result) = arena.alloc_str_with_slice(s);
                crate::op::InnerContent::List(
                    crate::container::list::list_op::InnerListOp::InsertText {
                        slice,
                        unicode_start: result.start as u32,
                        unicode_len: (result.end - result.start) as u32,
                        pos: prop as u32,
                    },
                )
            }
            ValueKind::DeleteSeq => {
                let len = value_reader.read_i32()?;
                crate::op::InnerContent::List(crate::container::list::list_op::InnerListOp::Delete(
                    DeleteSpan::new(prop as isize, len as isize),
                ))
            }
            ValueKind::MarkStart => {
                let mark = value_reader.read_mark(&keys.keys, cids)?;
                let key = keys
                    .keys
                    .get(mark.key_idx as usize)
                    .ok_or_else(|| LoroError::DecodeDataCorruptionError)?
                    .clone();
                crate::op::InnerContent::List(
                    crate::container::list::list_op::InnerListOp::StyleStart {
                        start: prop as u32,
                        end: prop as u32 + mark.len,
                        key,
                        value: mark.value,
                        info: TextStyleInfoFlag::from_byte(mark.info),
                    },
                )
            }
            ValueKind::Null => crate::op::InnerContent::List(
                crate::container::list::list_op::InnerListOp::StyleEnd,
            ),
            _ => unreachable!(),
        },
        ContainerType::Map => {
            let key = keys
                .keys
                .get(prop as usize)
                .ok_or(LoroError::DecodeDataCorruptionError)?
                .clone();
            match kind {
                ValueKind::DeleteOnce => {
                    crate::op::InnerContent::Map(crate::container::map::MapSet { key, value: None })
                }
                kind => {
                    let value = value_reader.read_value_content(kind, &keys.keys, cids)?;
                    crate::op::InnerContent::Map(crate::container::map::MapSet {
                        key,
                        value: Some(value),
                    })
                }
            }
        }
        ContainerType::List => {
            let pos = prop as usize;
            match kind {
                ValueKind::Array => {
                    let arr =
                        value_reader.read_value_content(ValueKind::Array, &keys.keys, cids)?;
                    let range = arena.alloc_values(
                        Arc::try_unwrap(
                            arr.into_list()
                                .map_err(|_| LoroError::DecodeDataCorruptionError)?,
                        )
                        .unwrap()
                        .into_iter(),
                    );
                    crate::op::InnerContent::List(
                        crate::container::list::list_op::InnerListOp::Insert {
                            slice: SliceRange::new(range.start as u32..range.end as u32),
                            pos,
                        },
                    )
                }
                ValueKind::DeleteSeq => {
                    let len = value_reader.read_i32()?;
                    crate::op::InnerContent::List(
                        crate::container::list::list_op::InnerListOp::Delete(DeleteSpan::new(
                            pos as isize,
                            len as isize,
                        )),
                    )
                }
                _ => unreachable!(),
            }
        }
        ContainerType::Tree => match kind {
            ValueKind::TreeMove => {
                let op = value_reader.read_tree_move()?;
                crate::op::InnerContent::Tree(op.as_tree_op(peers)?)
            }
            _ => unreachable!(),
        },
    };

    Ok(content)
}

type PeerIdx = usize;

struct ExtractedContainer {
    containers: Vec<ContainerID>,
    cid_idx_pairs: Vec<(ContainerID, ContainerIdx)>,
    idx_to_index: FxHashMap<ContainerIdx, usize>,
}

/// Extract containers from oplog changes.
///
/// Containers are sorted by their peer_id and counter so that
/// they can be compressed by using delta encoding.
fn extract_containers_in_order(
    c_iter: &mut dyn Iterator<Item = ContainerIdx>,
    arena: &SharedArena,
) -> ExtractedContainer {
    let mut containers = Vec::new();
    let mut visited = FxHashSet::default();
    for c in c_iter {
        if visited.contains(&c) {
            continue;
        }

        visited.insert(c);
        let id = arena.get_container_id(c).unwrap();
        containers.push((id, c));
    }

    containers.sort_unstable_by(|(a, _), (b, _)| {
        a.is_root()
            .cmp(&b.is_root())
            .then_with(|| a.container_type().cmp(&b.container_type()))
            .then_with(|| match (a, b) {
                (ContainerID::Root { name: a, .. }, ContainerID::Root { name: b, .. }) => a.cmp(b),
                (
                    ContainerID::Normal {
                        peer: peer_a,
                        counter: counter_a,
                        ..
                    },
                    ContainerID::Normal {
                        peer: peer_b,
                        counter: counter_b,
                        ..
                    },
                ) => peer_a.cmp(peer_b).then_with(|| counter_a.cmp(counter_b)),
                _ => unreachable!(),
            })
    });

    let container_idx2index = containers
        .iter()
        .enumerate()
        .map(|(i, (_, c))| (*c, i))
        .collect();

    ExtractedContainer {
        containers: containers.iter().map(|x| x.0.clone()).collect(),
        cid_idx_pairs: containers,
        idx_to_index: container_idx2index,
    }
}

#[columnar(ser, de)]
struct EncodedDoc<'a> {
    #[columnar(class = "vec", iter = "EncodedOp")]
    ops: Vec<EncodedOp>,
    #[columnar(class = "vec", iter = "EncodedChange")]
    changes: Vec<EncodedChange>,
    /// Container states snapshot.
    ///
    /// It's empty when the encoding mode is not snapshot.
    #[columnar(class = "vec", iter = "EncodedStateInfo")]
    states: Vec<EncodedStateInfo>,
    /// The first counter value for each change of each peer in `changes`
    start_counters: Vec<Counter>,
    frontiers: Vec<(PeerIdx, Counter)>,
    #[columnar(borrow)]
    raw_values: Cow<'a, [u8]>,

    /// A list of encoded arenas, in the following order
    /// - `peer_id_arena`
    /// - `container_arena`
    /// - `key_arena`
    /// - `deps_arena`
    /// - `state_arena`
    /// - `others`, left for future use
    #[columnar(borrow)]
    arenas: Cow<'a, [u8]>,
}

#[columnar(vec, ser, de, iterable)]
#[derive(Debug, Clone)]
struct EncodedOp {
    #[columnar(strategy = "DeltaRle")]
    container_index: u32,
    #[columnar(strategy = "DeltaRle")]
    prop: i32,
    #[columnar(strategy = "Rle")]
    peer_idx: u32,
    #[columnar(strategy = "DeltaRle")]
    value_type: u8,
    #[columnar(strategy = "DeltaRle")]
    counter: i32,
}

#[columnar(vec, ser, de, iterable)]
#[derive(Debug, Clone)]
struct EncodedChange {
    #[columnar(strategy = "Rle")]
    peer_idx: usize,
    #[columnar(strategy = "DeltaRle")]
    len: usize,
    #[columnar(strategy = "DeltaRle")]
    timestamp: i64,
    #[columnar(strategy = "DeltaRle")]
    deps_len: i32,
    #[columnar(strategy = "BoolRle")]
    dep_on_self: bool,
    #[columnar(strategy = "DeltaRle")]
    msg_len: i32,
}

#[columnar(vec, ser, de, iterable)]
#[derive(Debug, Clone)]
struct EncodedStateInfo {
    #[columnar(strategy = "DeltaRle")]
    container_index: u32,
    #[columnar(strategy = "DeltaRle")]
    op_len: u32,
    #[columnar(strategy = "DeltaRle")]
    state_bytes_len: u32,
}

mod value {
    use std::sync::Arc;

    use fxhash::FxHashMap;
    use loro_common::{
        ContainerID, Counter, InternalString, LoroError, LoroResult, LoroValue, PeerID, TreeID,
    };
    use num_derive::{FromPrimitive, ToPrimitive};
    use num_traits::{FromPrimitive, ToPrimitive};

    use crate::container::tree::tree_op::TreeOp;

    use super::{encode::ValueRegister, MAX_COLLECTION_SIZE};

    #[allow(unused)]
    #[non_exhaustive]
    pub enum Value<'a> {
        Null,
        True,
        False,
        DeleteOnce,
        ContainerIdx(usize),
        I32(i32),
        F64(f64),
        Str(&'a str),
        DeleteSeq(i32),
        DeltaInt(i32),
        Array(Vec<Value<'a>>),
        Map(FxHashMap<InternalString, Value<'a>>),
        Binary(&'a [u8]),
        MarkStart(MarkStart),
        TreeMove(EncodedTreeMove),
        Unknown { kind: u8, data: &'a [u8] },
    }

    pub struct MarkStart {
        pub len: u32,
        pub key_idx: u32,
        pub value: LoroValue,
        pub info: u8,
    }

    pub struct EncodedTreeMove {
        pub subject_peer_idx: usize,
        pub subject_cnt: usize,
        pub is_parent_null: bool,
        pub parent_peer_idx: usize,
        pub parent_cnt: usize,
    }

    impl EncodedTreeMove {
        pub fn as_tree_op(&self, peer_ids: &[u64]) -> LoroResult<TreeOp> {
            Ok(TreeOp {
                target: TreeID::new(
                    *(peer_ids
                        .get(self.subject_peer_idx)
                        .ok_or(LoroError::DecodeDataCorruptionError)?),
                    self.subject_cnt as Counter,
                ),
                parent: if self.is_parent_null {
                    None
                } else {
                    Some(TreeID::new(
                        *(peer_ids
                            .get(self.parent_peer_idx)
                            .ok_or(LoroError::DecodeDataCorruptionError)?),
                        self.parent_cnt as Counter,
                    ))
                },
            })
        }

        pub fn from_tree_op(op: &TreeOp, register_peer_id: &mut ValueRegister<PeerID>) -> Self {
            EncodedTreeMove {
                subject_peer_idx: register_peer_id.register(&op.target.peer),
                subject_cnt: op.target.counter as usize,
                is_parent_null: op.parent.is_none(),
                parent_peer_idx: op.parent.map_or(0, |x| register_peer_id.register(&x.peer)),
                parent_cnt: op.parent.map_or(0, |x| x.counter as usize),
            }
        }
    }

    #[non_exhaustive]
    #[derive(Debug, FromPrimitive, ToPrimitive)]
    pub enum ValueKind {
        Null = 0,
        True = 1,
        False = 2,
        DeleteOnce = 3,
        I32 = 4,
        ContainerIdx = 5,
        F64 = 6,
        Str = 7,
        DeleteSeq = 8,
        DeltaInt = 9,
        Array = 10,
        Map = 11,
        MarkStart = 12,
        TreeMove = 13,
        Binary = 14,
        Unknown = 65536,
    }

    impl<'a> Value<'a> {
        pub fn kind(&self) -> ValueKind {
            match self {
                Value::Null => ValueKind::Null,
                Value::True => ValueKind::True,
                Value::False => ValueKind::False,
                Value::DeleteOnce => ValueKind::DeleteOnce,
                Value::I32(_) => ValueKind::I32,
                Value::ContainerIdx(_) => ValueKind::ContainerIdx,
                Value::F64(_) => ValueKind::F64,
                Value::Str(_) => ValueKind::Str,
                Value::DeleteSeq(_) => ValueKind::DeleteSeq,
                Value::DeltaInt(_) => ValueKind::DeltaInt,
                Value::Array(_) => ValueKind::Array,
                Value::Map(_) => ValueKind::Map,
                Value::MarkStart { .. } => ValueKind::MarkStart,
                Value::TreeMove(_) => ValueKind::TreeMove,
                Value::Binary(_) => ValueKind::Binary,
                Value::Unknown { .. } => ValueKind::Unknown,
            }
        }
    }

    fn get_loro_value_kind(value: &LoroValue) -> ValueKind {
        match value {
            LoroValue::Null => ValueKind::Null,
            LoroValue::Bool(true) => ValueKind::True,
            LoroValue::Bool(false) => ValueKind::False,
            LoroValue::I32(_) => ValueKind::I32,
            LoroValue::Double(_) => ValueKind::F64,
            LoroValue::String(_) => ValueKind::Str,
            LoroValue::List(_) => ValueKind::Array,
            LoroValue::Map(_) => ValueKind::Map,
            LoroValue::Binary(_) => ValueKind::Binary,
            LoroValue::Container(_) => ValueKind::ContainerIdx,
        }
    }

    pub struct ValueWriter {
        buffer: Vec<u8>,
    }

    impl ValueWriter {
        pub fn new() -> Self {
            ValueWriter { buffer: Vec::new() }
        }

        pub fn write_value_type_and_content(
            &mut self,
            value: &LoroValue,
            register_key: &mut ValueRegister<InternalString>,
            register_cid: &mut ValueRegister<ContainerID>,
        ) -> ValueKind {
            self.write_u8(get_loro_value_kind(value).to_u8().unwrap());
            self.write_value_content(value, register_key, register_cid)
        }

        pub fn write_value_content(
            &mut self,
            value: &LoroValue,
            register_key: &mut ValueRegister<InternalString>,
            register_cid: &mut ValueRegister<ContainerID>,
        ) -> ValueKind {
            match value {
                LoroValue::Null => ValueKind::Null,
                LoroValue::Bool(true) => ValueKind::True,
                LoroValue::Bool(false) => ValueKind::False,
                LoroValue::I32(value) => {
                    self.write_i32(*value);
                    ValueKind::I32
                }
                LoroValue::Double(value) => {
                    self.write_f64(*value);
                    ValueKind::F64
                }
                LoroValue::String(value) => {
                    self.write_str(value);
                    ValueKind::Str
                }
                LoroValue::List(value) => {
                    self.write_usize(value.len());
                    for value in value.iter() {
                        self.write_value_type_and_content(value, register_key, register_cid);
                    }
                    ValueKind::Array
                }
                LoroValue::Map(value) => {
                    self.write_usize(value.len());
                    for (key, value) in value.iter() {
                        let key_idx = register_key.register(&key.as_str().into());
                        self.write_usize(key_idx);
                        self.write_value_type_and_content(value, register_key, register_cid);
                    }
                    ValueKind::Map
                }
                LoroValue::Binary(value) => {
                    self.write_binary(value);
                    ValueKind::Binary
                }
                LoroValue::Container(c) => {
                    let idx = register_cid.register(c);
                    self.write_usize(idx);
                    ValueKind::ContainerIdx
                }
            }
        }

        pub fn write(
            &mut self,
            value: &Value,
            register_key: &mut ValueRegister<InternalString>,
            register_cid: &mut ValueRegister<ContainerID>,
        ) {
            match value {
                Value::Null => {}
                Value::True => {}
                Value::False => {}
                Value::DeleteOnce => {}
                Value::I32(value) => self.write_i32(*value),
                Value::F64(value) => self.write_f64(*value),
                Value::Str(value) => self.write_str(value),
                Value::DeleteSeq(value) => self.write_i32(*value),
                Value::DeltaInt(value) => self.write_i32(*value),
                Value::Array(value) => self.write_array(value, register_key, register_cid),
                Value::Map(value) => self.write_map(value, register_key, register_cid),
                Value::MarkStart(value) => self.write_mark(value, register_key, register_cid),
                Value::TreeMove(op) => self.write_tree_move(op),
                Value::Binary(value) => self.write_binary(value),
                Value::ContainerIdx(value) => self.write_usize(*value),
                Value::Unknown { kind: _, data: _ } => unreachable!(),
            }
        }

        fn write_i32(&mut self, value: i32) {
            leb128::write::signed(&mut self.buffer, value as i64).unwrap();
        }

        fn write_usize(&mut self, value: usize) {
            leb128::write::unsigned(&mut self.buffer, value as u64).unwrap();
        }

        fn write_f64(&mut self, value: f64) {
            self.buffer.extend_from_slice(&value.to_be_bytes());
        }

        fn write_str(&mut self, value: &str) {
            self.write_usize(value.len());
            self.buffer.extend_from_slice(value.as_bytes());
        }

        fn write_u8(&mut self, value: u8) {
            self.buffer.push(value);
        }

        pub fn write_kind(&mut self, kind: ValueKind) {
            self.write_u8(kind.to_u8().unwrap());
        }

        fn write_array(
            &mut self,
            value: &[Value],
            register_key: &mut ValueRegister<InternalString>,
            register_cid: &mut ValueRegister<ContainerID>,
        ) {
            self.write_usize(value.len());
            for value in value {
                self.write_kind(value.kind());
                self.write(value, register_key, register_cid);
            }
        }

        fn write_map(
            &mut self,
            value: &FxHashMap<InternalString, Value>,
            register_key: &mut ValueRegister<InternalString>,
            register_cid: &mut ValueRegister<ContainerID>,
        ) {
            self.write_usize(value.len());
            for (key, value) in value {
                let key_idx = register_key.register(key);
                self.write_usize(key_idx);
                self.write_kind(value.kind());
                self.write(value, register_key, register_cid);
            }
        }

        fn write_binary(&mut self, value: &[u8]) {
            self.write_usize(value.len());
            self.buffer.extend_from_slice(value);
        }

        fn write_mark(
            &mut self,
            mark: &MarkStart,
            register_key: &mut ValueRegister<InternalString>,
            register_cid: &mut ValueRegister<ContainerID>,
        ) {
            self.write_u8(mark.info);
            self.write_usize(mark.len as usize);
            self.write_usize(mark.key_idx as usize);
            self.write_value_type_and_content(&mark.value, register_key, register_cid);
        }

        fn write_tree_move(&mut self, op: &EncodedTreeMove) {
            self.write_usize(op.subject_peer_idx);
            self.write_usize(op.subject_cnt);
            self.write_u8(op.is_parent_null as u8);
            if op.is_parent_null {
                return;
            }

            self.write_usize(op.parent_peer_idx);
            self.write_usize(op.parent_cnt);
        }

        pub(crate) fn finish(self) -> Vec<u8> {
            self.buffer
        }
    }

    pub struct ValueReader<'a> {
        raw: &'a [u8],
    }

    impl<'a> ValueReader<'a> {
        pub fn new(raw: &'a [u8]) -> Self {
            ValueReader { raw }
        }

        #[allow(unused)]
        pub fn read(
            &mut self,
            kind: u8,
            keys: &[InternalString],
            cids: &[ContainerID],
        ) -> LoroResult<Value<'a>> {
            let Some(kind) = ValueKind::from_u8(kind) else {
                return Ok(Value::Unknown {
                    kind,
                    data: self.read_binary()?,
                });
            };

            Ok(match kind {
                ValueKind::Null => Value::Null,
                ValueKind::True => Value::True,
                ValueKind::False => Value::False,
                ValueKind::DeleteOnce => Value::DeleteOnce,
                ValueKind::I32 => Value::I32(self.read_i32()?),
                ValueKind::F64 => Value::F64(self.read_f64()?),
                ValueKind::Str => Value::Str(self.read_str()?),
                ValueKind::DeleteSeq => Value::DeleteSeq(self.read_i32()?),
                ValueKind::DeltaInt => Value::DeltaInt(self.read_i32()?),
                ValueKind::Array => Value::Array(self.read_array(keys, cids)?),
                ValueKind::Map => Value::Map(self.read_map(keys, cids)?),
                ValueKind::Binary => Value::Binary(self.read_binary()?),
                ValueKind::MarkStart => Value::MarkStart(self.read_mark(keys, cids)?),
                ValueKind::TreeMove => Value::TreeMove(self.read_tree_move()?),
                ValueKind::ContainerIdx => Value::ContainerIdx(self.read_usize()?),
                ValueKind::Unknown => unreachable!(),
            })
        }

        pub fn read_value_type_and_content(
            &mut self,
            keys: &[InternalString],
            cids: &[ContainerID],
        ) -> LoroResult<LoroValue> {
            let kind = self.read_u8()?;
            self.read_value_content(
                ValueKind::from_u8(kind).expect("Unknown value type"),
                keys,
                cids,
            )
        }

        pub fn read_value_content(
            &mut self,
            kind: ValueKind,
            keys: &[InternalString],
            cids: &[ContainerID],
        ) -> LoroResult<LoroValue> {
            Ok(match kind {
                ValueKind::Null => LoroValue::Null,
                ValueKind::True => LoroValue::Bool(true),
                ValueKind::False => LoroValue::Bool(false),
                ValueKind::I32 => LoroValue::I32(self.read_i32()?),
                ValueKind::F64 => LoroValue::Double(self.read_f64()?),
                ValueKind::Str => LoroValue::String(Arc::new(self.read_str()?.to_owned())),
                ValueKind::DeltaInt => LoroValue::I32(self.read_i32()?),
                ValueKind::Array => {
                    let len = self.read_usize()?;
                    if len > MAX_COLLECTION_SIZE {
                        return Err(LoroError::DecodeDataCorruptionError);
                    }

                    let mut ans = Vec::with_capacity(len);
                    for _ in 0..len {
                        ans.push(self.recursive_read_value_type_and_content(keys, cids)?);
                    }
                    ans.into()
                }
                ValueKind::Map => {
                    let len = self.read_usize()?;
                    if len > MAX_COLLECTION_SIZE {
                        return Err(LoroError::DecodeDataCorruptionError);
                    }

                    let mut ans = FxHashMap::with_capacity_and_hasher(len, Default::default());
                    for _ in 0..len {
                        let key_idx = self.read_usize()?;
                        let key = keys
                            .get(key_idx)
                            .ok_or(LoroError::DecodeDataCorruptionError)?
                            .to_string();
                        let value = self.recursive_read_value_type_and_content(keys, cids)?;
                        ans.insert(key, value);
                    }
                    ans.into()
                }
                ValueKind::Binary => LoroValue::Binary(Arc::new(self.read_binary()?.to_owned())),
                ValueKind::ContainerIdx => LoroValue::Container(
                    cids.get(self.read_usize()?)
                        .ok_or(LoroError::DecodeDataCorruptionError)?
                        .clone(),
                ),
                a => unreachable!("Unexpected value kind {:?}", a),
            })
        }

        /// Read a value that may be very deep efficiently.
        ///
        /// This method avoids using recursive calls to read deeply nested values.
        /// Otherwise, it may cause stack overflow.
        fn recursive_read_value_type_and_content(
            &mut self,
            keys: &[InternalString],
            cids: &[ContainerID],
        ) -> LoroResult<LoroValue> {
            #[derive(Debug)]
            enum Task {
                Init,
                ReadList {
                    left: usize,
                    vec: Vec<LoroValue>,

                    key_idx_in_parent: usize,
                },
                ReadMap {
                    left: usize,
                    map: FxHashMap<String, LoroValue>,

                    key_idx_in_parent: usize,
                },
            }
            impl Task {
                fn should_read(&self) -> bool {
                    !matches!(
                        self,
                        Self::ReadList { left: 0, .. } | Self::ReadMap { left: 0, .. }
                    )
                }

                fn key_idx(&self) -> usize {
                    match self {
                        Self::ReadList {
                            key_idx_in_parent, ..
                        } => *key_idx_in_parent,
                        Self::ReadMap {
                            key_idx_in_parent, ..
                        } => *key_idx_in_parent,
                        _ => unreachable!(),
                    }
                }

                fn into_value(self) -> LoroValue {
                    match self {
                        Self::ReadList { vec, .. } => vec.into(),
                        Self::ReadMap { map, .. } => map.into(),
                        _ => unreachable!(),
                    }
                }
            }
            let mut stack = vec![Task::Init];
            while let Some(mut task) = stack.pop() {
                if task.should_read() {
                    let key_idx = if matches!(task, Task::ReadMap { .. }) {
                        self.read_usize()?
                    } else {
                        0
                    };
                    let kind = self.read_u8()?;
                    let kind = ValueKind::from_u8(kind).expect("Unknown value type");
                    let value = match kind {
                        ValueKind::Null => LoroValue::Null,
                        ValueKind::True => LoroValue::Bool(true),
                        ValueKind::False => LoroValue::Bool(false),
                        ValueKind::I32 => LoroValue::I32(self.read_i32()?),
                        ValueKind::F64 => LoroValue::Double(self.read_f64()?),
                        ValueKind::Str => LoroValue::String(Arc::new(self.read_str()?.to_owned())),
                        ValueKind::DeltaInt => LoroValue::I32(self.read_i32()?),
                        ValueKind::Array => {
                            let len = self.read_usize()?;
                            if len > MAX_COLLECTION_SIZE {
                                return Err(LoroError::DecodeDataCorruptionError);
                            }

                            let ans = Vec::with_capacity(len);
                            stack.push(task);
                            stack.push(Task::ReadList {
                                left: len,
                                vec: ans,
                                key_idx_in_parent: key_idx,
                            });
                            continue;
                        }
                        ValueKind::Map => {
                            let len = self.read_usize()?;
                            if len > MAX_COLLECTION_SIZE {
                                return Err(LoroError::DecodeDataCorruptionError);
                            }

                            let ans = FxHashMap::with_capacity_and_hasher(len, Default::default());
                            stack.push(task);
                            stack.push(Task::ReadMap {
                                left: len,
                                map: ans,
                                key_idx_in_parent: key_idx,
                            });
                            continue;
                        }
                        ValueKind::Binary => {
                            LoroValue::Binary(Arc::new(self.read_binary()?.to_owned()))
                        }
                        ValueKind::ContainerIdx => LoroValue::Container(
                            cids.get(self.read_usize()?)
                                .ok_or(LoroError::DecodeDataCorruptionError)?
                                .clone(),
                        ),
                        a => unreachable!("Unexpected value kind {:?}", a),
                    };

                    task = match task {
                        Task::Init => {
                            return Ok(value);
                        }
                        Task::ReadList {
                            mut left,
                            mut vec,
                            key_idx_in_parent,
                        } => {
                            left -= 1;
                            vec.push(value);
                            let task = Task::ReadList {
                                left,
                                vec,
                                key_idx_in_parent,
                            };
                            if left != 0 {
                                stack.push(task);
                                continue;
                            }

                            task
                        }
                        Task::ReadMap {
                            mut left,
                            mut map,
                            key_idx_in_parent,
                        } => {
                            left -= 1;
                            let key = keys
                                .get(key_idx)
                                .ok_or(LoroError::DecodeDataCorruptionError)?
                                .to_string();
                            map.insert(key, value);
                            let task = Task::ReadMap {
                                left,
                                map,
                                key_idx_in_parent,
                            };
                            if left != 0 {
                                stack.push(task);
                                continue;
                            }
                            task
                        }
                    };
                }

                let key_index = task.key_idx();
                let value = task.into_value();
                if let Some(last) = stack.last_mut() {
                    match last {
                        Task::Init => {
                            return Ok(value);
                        }
                        Task::ReadList { left, vec, .. } => {
                            *left -= 1;
                            vec.push(value);
                        }
                        Task::ReadMap { left, map, .. } => {
                            *left -= 1;
                            let key = keys
                                .get(key_index)
                                .ok_or(LoroError::DecodeDataCorruptionError)?
                                .to_string();
                            map.insert(key, value);
                        }
                    }
                } else {
                    return Ok(value);
                }
            }

            unreachable!();
        }

        pub fn read_i32(&mut self) -> LoroResult<i32> {
            leb128::read::signed(&mut self.raw)
                .map(|x| x as i32)
                .map_err(|_| LoroError::DecodeDataCorruptionError)
        }

        fn read_f64(&mut self) -> LoroResult<f64> {
            if self.raw.len() < 8 {
                return Err(LoroError::DecodeDataCorruptionError);
            }

            let mut bytes = [0; 8];
            bytes.copy_from_slice(&self.raw[..8]);
            self.raw = &self.raw[8..];
            Ok(f64::from_be_bytes(bytes))
        }

        pub fn read_usize(&mut self) -> LoroResult<usize> {
            Ok(leb128::read::unsigned(&mut self.raw)
                .map_err(|_| LoroError::DecodeDataCorruptionError)? as usize)
        }

        pub fn read_str(&mut self) -> LoroResult<&'a str> {
            let len = self.read_usize()?;
            if self.raw.len() < len {
                return Err(LoroError::DecodeDataCorruptionError);
            }

            let ans = std::str::from_utf8(&self.raw[..len]).unwrap();
            self.raw = &self.raw[len..];
            Ok(ans)
        }

        fn read_u8(&mut self) -> LoroResult<u8> {
            if self.raw.is_empty() {
                return Err(LoroError::DecodeDataCorruptionError);
            }

            let ans = self.raw[0];
            self.raw = &self.raw[1..];
            Ok(ans)
        }

        fn read_array(
            &mut self,
            keys: &[InternalString],
            cids: &[ContainerID],
        ) -> LoroResult<Vec<Value<'a>>> {
            let len = self.read_usize()?;
            if len > MAX_COLLECTION_SIZE {
                return Err(LoroError::DecodeDataCorruptionError);
            }

            let mut ans = Vec::with_capacity(len);
            for _ in 0..len {
                let kind = self.read_u8()?;
                ans.push(self.read(kind, keys, cids)?);
            }
            Ok(ans)
        }

        fn read_map(
            &mut self,
            keys: &[InternalString],
            cids: &[ContainerID],
        ) -> LoroResult<FxHashMap<InternalString, Value<'a>>> {
            let len = self.read_usize()?;
            if len > MAX_COLLECTION_SIZE {
                return Err(LoroError::DecodeDataCorruptionError);
            }

            let mut ans = FxHashMap::with_capacity_and_hasher(len, Default::default());
            for _ in 0..len {
                let key_idx = self.read_usize()?;
                let key = keys
                    .get(key_idx)
                    .ok_or(LoroError::DecodeDataCorruptionError)?
                    .clone();
                let kind = self.read_u8()?;
                let value = self.read(kind, keys, cids)?;
                ans.insert(key, value);
            }
            Ok(ans)
        }

        fn read_binary(&mut self) -> LoroResult<&'a [u8]> {
            let len = self.read_usize()?;
            if self.raw.len() < len {
                return Err(LoroError::DecodeDataCorruptionError);
            }

            let ans = &self.raw[..len];
            self.raw = &self.raw[len..];
            Ok(ans)
        }

        pub fn read_mark(
            &mut self,
            keys: &[InternalString],
            cids: &[ContainerID],
        ) -> LoroResult<MarkStart> {
            let info = self.read_u8()?;
            let len = self.read_usize()?;
            let key_idx = self.read_usize()?;
            let value = self.read_value_type_and_content(keys, cids)?;
            Ok(MarkStart {
                len: len as u32,
                key_idx: key_idx as u32,
                value,
                info,
            })
        }

        pub fn read_tree_move(&mut self) -> LoroResult<EncodedTreeMove> {
            let subject_peer_idx = self.read_usize()?;
            let subject_cnt = self.read_usize()?;
            let is_parent_null = self.read_u8()? != 0;
            let mut parent_peer_idx = 0;
            let mut parent_cnt = 0;
            if !is_parent_null {
                parent_peer_idx = self.read_usize()?;
                parent_cnt = self.read_usize()?;
            }

            Ok(EncodedTreeMove {
                subject_peer_idx,
                subject_cnt,
                is_parent_null,
                parent_peer_idx,
                parent_cnt,
            })
        }
    }
}

mod arena {
    use crate::InternalString;
    use loro_common::{ContainerID, ContainerType, LoroError, LoroResult, PeerID};
    use serde::{Deserialize, Serialize};
    use serde_columnar::columnar;

    use super::{encode::ValueRegister, PeerIdx, MAX_DECODED_SIZE};

    pub fn encode_arena(
        peer_ids_arena: Vec<u64>,
        containers: ContainerArena,
        keys: Vec<InternalString>,
        deps: DepsArena,
        state_blob_arena: &[u8],
    ) -> Vec<u8> {
        let peer_ids = PeerIdArena {
            peer_ids: peer_ids_arena,
        };

        let key_arena = KeyArena { keys };
        let encoded = EncodedArenas {
            peer_id_arena: &peer_ids.encode(),
            container_arena: &containers.encode(),
            key_arena: &key_arena.encode(),
            deps_arena: &deps.encode(),
            state_blob_arena,
        };

        encoded.encode_arenas()
    }

    pub struct DecodedArenas<'a> {
        pub peer_ids: PeerIdArena,
        pub containers: ContainerArena,
        pub keys: KeyArena,
        pub deps: Box<dyn Iterator<Item = EncodedDep> + 'a>,
        pub state_blob_arena: &'a [u8],
    }

    pub fn decode_arena(bytes: &[u8]) -> LoroResult<DecodedArenas> {
        let arenas = EncodedArenas::decode_arenas(bytes)?;
        Ok(DecodedArenas {
            peer_ids: PeerIdArena::decode(arenas.peer_id_arena)?,
            containers: ContainerArena::decode(arenas.container_arena)?,
            keys: KeyArena::decode(arenas.key_arena)?,
            deps: Box::new(DepsArena::decode_iter(arenas.deps_arena)?),
            state_blob_arena: arenas.state_blob_arena,
        })
    }

    struct EncodedArenas<'a> {
        peer_id_arena: &'a [u8],
        container_arena: &'a [u8],
        key_arena: &'a [u8],
        deps_arena: &'a [u8],
        state_blob_arena: &'a [u8],
    }

    impl EncodedArenas<'_> {
        fn encode_arenas(self) -> Vec<u8> {
            let mut ans = Vec::with_capacity(
                self.peer_id_arena.len()
                    + self.container_arena.len()
                    + self.key_arena.len()
                    + self.deps_arena.len()
                    + 4 * 4,
            );

            write_arena(&mut ans, self.peer_id_arena);
            write_arena(&mut ans, self.container_arena);
            write_arena(&mut ans, self.key_arena);
            write_arena(&mut ans, self.deps_arena);
            write_arena(&mut ans, self.state_blob_arena);
            ans
        }

        fn decode_arenas(bytes: &[u8]) -> LoroResult<EncodedArenas> {
            let (peer_id_arena, rest) = read_arena(bytes)?;
            let (container_arena, rest) = read_arena(rest)?;
            let (key_arena, rest) = read_arena(rest)?;
            let (deps_arena, rest) = read_arena(rest)?;
            let (state_blob_arena, _) = read_arena(rest)?;
            Ok(EncodedArenas {
                peer_id_arena,
                container_arena,
                key_arena,
                deps_arena,
                state_blob_arena,
            })
        }
    }

    #[derive(Serialize, Deserialize)]
    pub(super) struct PeerIdArena {
        pub(super) peer_ids: Vec<u64>,
    }

    impl PeerIdArena {
        fn encode(&self) -> Vec<u8> {
            let mut ans = Vec::with_capacity(self.peer_ids.len() * 8);
            leb128::write::unsigned(&mut ans, self.peer_ids.len() as u64).unwrap();
            for &peer_id in &self.peer_ids {
                ans.extend_from_slice(&peer_id.to_be_bytes());
            }
            ans
        }

        fn decode(peer_id_arena: &[u8]) -> LoroResult<Self> {
            let mut reader = peer_id_arena;
            let len = leb128::read::unsigned(&mut reader)
                .map_err(|_| LoroError::DecodeDataCorruptionError)?;
            if len > MAX_DECODED_SIZE as u64 {
                return Err(LoroError::DecodeDataCorruptionError);
            }

            let mut peer_ids = Vec::with_capacity(len as usize);
            if reader.len() < len as usize * 8 {
                return Err(LoroError::DecodeDataCorruptionError);
            }

            for _ in 0..len {
                let mut peer_id_bytes = [0; 8];
                peer_id_bytes.copy_from_slice(&reader[..8]);
                peer_ids.push(u64::from_be_bytes(peer_id_bytes));
                reader = &reader[8..];
            }
            Ok(PeerIdArena { peer_ids })
        }
    }

    #[columnar(vec, ser, de, iterable)]
    #[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
    pub(super) struct EncodedContainer {
        #[columnar(strategy = "BoolRle")]
        is_root: bool,
        #[columnar(strategy = "Rle")]
        kind: u8,
        #[columnar(strategy = "Rle")]
        peer_idx: usize,
        #[columnar(strategy = "DeltaRle")]
        key_idx_or_counter: i32,
    }

    impl EncodedContainer {
        pub fn as_container_id(
            &self,
            key_arena: &[InternalString],
            peer_arena: &[u64],
        ) -> LoroResult<ContainerID> {
            if self.is_root {
                Ok(ContainerID::Root {
                    container_type: ContainerType::try_from_u8(self.kind)?,
                    name: key_arena
                        .get(self.key_idx_or_counter as usize)
                        .ok_or(LoroError::DecodeDataCorruptionError)?
                        .clone(),
                })
            } else {
                Ok(ContainerID::Normal {
                    container_type: ContainerType::try_from_u8(self.kind)?,
                    peer: *(peer_arena
                        .get(self.peer_idx)
                        .ok_or(LoroError::DecodeDataCorruptionError)?),
                    counter: self.key_idx_or_counter,
                })
            }
        }
    }

    #[columnar(ser, de)]
    #[derive(Default)]
    pub(super) struct ContainerArena {
        #[columnar(class = "vec", iter = "EncodedContainer")]
        pub(super) containers: Vec<EncodedContainer>,
    }

    impl ContainerArena {
        fn encode(&self) -> Vec<u8> {
            serde_columnar::to_vec(&self.containers).unwrap()
        }

        fn decode(bytes: &[u8]) -> LoroResult<Self> {
            Ok(ContainerArena {
                containers: serde_columnar::from_bytes(bytes)?,
            })
        }

        pub fn from_containers(
            cids: Vec<ContainerID>,
            peer_register: &mut ValueRegister<PeerID>,
            key_reg: &mut ValueRegister<InternalString>,
        ) -> Self {
            let mut ans = Self {
                containers: Vec::with_capacity(cids.len()),
            };
            for cid in cids {
                ans.push(cid, peer_register, key_reg);
            }

            ans
        }

        pub fn push(
            &mut self,
            id: ContainerID,
            peer_register: &mut ValueRegister<PeerID>,
            register_key: &mut ValueRegister<InternalString>,
        ) {
            let (is_root, kind, peer_idx, key_idx_or_counter) = match id {
                ContainerID::Root {
                    container_type,
                    name,
                } => (true, container_type, 0, register_key.register(&name) as i32),
                ContainerID::Normal {
                    container_type,
                    peer,
                    counter,
                } => (
                    false,
                    container_type,
                    peer_register.register(&peer),
                    counter,
                ),
            };
            self.containers.push(EncodedContainer {
                is_root,
                kind: kind.to_u8(),
                peer_idx,
                key_idx_or_counter,
            });
        }
    }

    #[columnar(vec, ser, de, iterable)]
    #[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
    pub struct EncodedDep {
        #[columnar(strategy = "Rle")]
        pub peer_idx: usize,
        #[columnar(strategy = "DeltaRle")]
        pub counter: i32,
    }

    #[columnar(ser, de)]
    #[derive(Default)]
    pub(super) struct DepsArena {
        #[columnar(class = "vec", iter = "EncodedDep")]
        deps: Vec<EncodedDep>,
    }

    impl DepsArena {
        pub fn push(&mut self, peer_idx: PeerIdx, counter: i32) {
            self.deps.push(EncodedDep { peer_idx, counter });
        }

        pub fn encode(&self) -> Vec<u8> {
            serde_columnar::to_vec(&self).unwrap()
        }

        pub fn decode_iter(bytes: &[u8]) -> LoroResult<impl Iterator<Item = EncodedDep> + '_> {
            let iter = serde_columnar::iter_from_bytes::<DepsArena>(bytes)?;
            Ok(iter.deps)
        }
    }

    #[derive(Serialize, Deserialize, Default)]
    pub(super) struct KeyArena {
        pub(super) keys: Vec<InternalString>,
    }

    impl KeyArena {
        pub fn encode(&self) -> Vec<u8> {
            serde_columnar::to_vec(&self).unwrap()
        }

        pub fn decode(bytes: &[u8]) -> LoroResult<Self> {
            Ok(serde_columnar::from_bytes(bytes)?)
        }
    }

    fn write_arena(buffer: &mut Vec<u8>, arena: &[u8]) {
        leb128::write::unsigned(buffer, arena.len() as u64).unwrap();
        buffer.extend_from_slice(arena);
    }

    /// Return (next_arena, rest)
    fn read_arena(mut buffer: &[u8]) -> LoroResult<(&[u8], &[u8])> {
        let reader = &mut buffer;
        let len = leb128::read::unsigned(reader)
            .map_err(|_| LoroError::DecodeDataCorruptionError)? as usize;
        if len > MAX_DECODED_SIZE {
            return Err(LoroError::DecodeDataCorruptionError);
        }

        if len > reader.len() {
            return Err(LoroError::DecodeDataCorruptionError);
        }

        Ok((reader[..len as usize].as_ref(), &reader[len as usize..]))
    }
}

#[cfg(test)]
mod test {
    use loro_common::LoroValue;

    use crate::fx_map;

    use super::*;

    fn test_loro_value_read_write(v: impl Into<LoroValue>) {
        let v = v.into();
        let mut key_reg: ValueRegister<InternalString> = ValueRegister::new();
        let mut cid_reg: ValueRegister<ContainerID> = ValueRegister::new();
        let mut writer = ValueWriter::new();
        let kind = writer.write_value_content(&v, &mut key_reg, &mut cid_reg);

        let binding = writer.finish();
        let mut reader = ValueReader::new(binding.as_slice());
        let keys = &key_reg.unwrap_vec();
        let cids = &cid_reg.unwrap_vec();
        let ans = reader.read_value_content(kind, keys, cids).unwrap();
        assert_eq!(v, ans)
    }

    #[test]
    fn test_value_read_write() {
        test_loro_value_read_write(true);
        test_loro_value_read_write(false);
        test_loro_value_read_write(123);
        test_loro_value_read_write(1.23);
        test_loro_value_read_write(LoroValue::Null);
        test_loro_value_read_write(LoroValue::Binary(Arc::new(vec![123, 223, 255, 0, 1, 2, 3])));
        test_loro_value_read_write("sldk;ajfas;dlkfas测试");
        test_loro_value_read_write(LoroValue::Container(ContainerID::new_root(
            "name",
            ContainerType::Text,
        )));
        test_loro_value_read_write(LoroValue::Container(ContainerID::new_normal(
            ID::new(u64::MAX, 123),
            ContainerType::Tree,
        )));
        test_loro_value_read_write(vec![1i32, 2, 3]);
        test_loro_value_read_write(LoroValue::Map(Arc::new(fx_map![
            "1".into() => 123.into(),
            "2".into() => "123".into(),
            "3".into() => vec![true].into()
        ])));
    }
}

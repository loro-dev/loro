use std::{borrow::Cow, cell::RefCell, cmp::Ordering, mem::take, rc::Rc};

use fxhash::{FxHashMap, FxHashSet};
use generic_btree::rle::Sliceable;
use itertools::Itertools;
use loro_common::{
    ContainerID, ContainerType, Counter, HasCounterSpan, HasId, HasIdSpan, HasLamportSpan, IdLp,
    InternalString, LoroError, LoroResult, PeerID, ID,
};
use rle::HasLength;
use serde_columnar::{columnar, ColumnarError};
use tracing::instrument;

use crate::{
    arena::SharedArena,
    change::{Change, Lamport, Timestamp},
    container::{idx::ContainerIdx, list::list_op::DeleteSpanWithId, richtext::TextStyleInfoFlag},
    encoding::StateSnapshotDecodeContext,
    op::{FutureInnerContent, Op, OpWithId, SliceRange},
    state::ContainerState,
    version::Frontiers,
    DocState, LoroDoc, OpLog, VersionVector,
};
pub(super) use encode::ValueRegister;

use self::encode::{encode_changes, encode_ops, init_encode, TempOp};

use super::{
    arena::*,
    parse_header_and_body,
    value::{Value, ValueKind, ValueReader, ValueWriter},
    ImportBlobMetadata,
};

/// If any section of the document is longer than this, we will not decode it.
/// It will return an data corruption error instead.
pub(super) const MAX_DECODED_SIZE: usize = 1 << 30;
/// If any collection in the document is longer than this, we will not decode it.
/// It will return an data corruption error instead.
pub(super) const MAX_COLLECTION_SIZE: usize = 1 << 28;

pub(crate) fn encode_updates(oplog: &OpLog, vv: &VersionVector) -> Vec<u8> {
    // skip the ops that current oplog does not have
    let actual_start_vv: VersionVector = vv
        .iter()
        .filter_map(|(&peer, &end_counter)| {
            if end_counter == 0 {
                return None;
            }

            let this_end = oplog.vv().get(&peer).cloned().unwrap_or(0);
            if this_end <= end_counter {
                return Some((peer, this_end));
            }

            Some((peer, end_counter))
        })
        .collect();

    let vv = &actual_start_vv;
    let mut peer_register: ValueRegister<PeerID> = ValueRegister::new();
    let key_register: ValueRegister<InternalString> = ValueRegister::new();
    let (start_counters, diff_changes) = init_encode(oplog, vv, &mut peer_register);
    let ExtractedContainer {
        containers,
        cid_idx_pairs: _,
        container_to_index: container2index,
    } = extract_containers_in_order(
        &mut diff_changes
            .iter()
            .flat_map(|x| x.ops.iter())
            .map(|x| x.container),
        &oplog.arena,
    );

    let cid_register: ValueRegister<ContainerID> = ValueRegister::from_existing(containers);

    let mut registers = EncodedRegisters {
        peer: peer_register,
        container: cid_register,
        key: key_register,
    };
    let mut dep_arena = DepsArena::default();
    let mut value_writer = ValueWriter::new();
    let mut ops: Vec<TempOp> = Vec::new();
    let arena = &oplog.arena;
    let changes = encode_changes(
        &diff_changes,
        &mut dep_arena,
        &mut |op| ops.push(op),
        &container2index,
        &mut registers,
    );

    ops.sort_by(move |a, b| {
        a.container_index
            .cmp(&b.container_index)
            .then_with(|| a.prop_that_used_for_sort.cmp(&b.prop_that_used_for_sort))
            .then_with(|| a.peer_idx.cmp(&b.peer_idx))
            .then_with(|| a.lamport.cmp(&b.lamport))
    });

    let (encoded_ops, del_starts) = encode_ops(ops, arena, &mut value_writer, &mut registers);

    let EncodedRegisters {
        peer: mut peer_register,
        container: cid_register,
        key: mut key_register,
    } = registers;

    let container_arena = ContainerArena::from_containers(
        cid_register.unwrap_vec(),
        &mut peer_register,
        &mut key_register,
    );

    let frontiers = oplog
        .dag
        .vv_to_frontiers(&actual_start_vv)
        .iter()
        .map(|x| (peer_register.register(&x.peer), x.counter))
        .collect();
    let doc = EncodedDoc {
        ops: encoded_ops,
        delete_starts: del_starts,
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
        start_frontiers: frontiers,
    };

    serde_columnar::to_vec(&doc).unwrap()
}

#[instrument(skip_all)]
pub(crate) fn decode_updates(oplog: &mut OpLog, bytes: &[u8]) -> LoroResult<()> {
    let iter = serde_columnar::iter_from_bytes::<EncodedDoc>(bytes)?;
    let arenas = decode_arena(&iter.arenas)?;
    let ops_map = extract_ops(
        &iter.raw_values,
        iter.ops,
        iter.delete_starts,
        &oplog.arena,
        &arenas,
        false,
    )?
    .ops_map;

    let DecodedArenas {
        peer_ids,
        deps,
        state_blob_arena: _,
        ..
    } = arenas;

    let changes = decode_changes(iter.changes, iter.start_counters, &peer_ids, deps, ops_map)?;

    let (latest_ids, pending_changes) = import_changes_to_oplog(changes, oplog)?;
    if oplog.try_apply_pending(latest_ids).should_update && !oplog.batch_importing {
        oplog.dag.refresh_frontiers();
    }

    oplog.import_unknown_lamport_pending_changes(pending_changes)?;
    Ok(())
}

pub fn decode_import_blob_meta(bytes: &[u8]) -> LoroResult<ImportBlobMetadata> {
    let parsed = parse_header_and_body(bytes)?;
    let is_snapshot = parsed.mode.is_snapshot();
    let iterators = serde_columnar::iter_from_bytes::<EncodedDoc>(parsed.body)?;
    let DecodedArenas { peer_ids, .. } = decode_arena(&iterators.arenas)?;
    let start_vv: VersionVector = iterators
        .start_counters
        .iter()
        .enumerate()
        .filter_map(|(peer_idx, counter)| {
            if *counter == 0 {
                None
            } else {
                Some(ID::new(peer_ids.peer_ids[peer_idx], *counter - 1))
            }
        })
        .collect();
    let frontiers = iterators
        .start_frontiers
        .iter()
        .map(|x| ID::new(peer_ids.peer_ids[x.0], x.1))
        .collect();
    let mut end_vv_counters = iterators.start_counters;
    let mut change_num = 0;
    let mut start_timestamp = Timestamp::MAX;
    let mut end_timestamp = Timestamp::MIN;

    for iter in iterators.changes {
        let EncodedChange {
            peer_idx,
            len,
            timestamp,
            ..
        } = iter?;
        end_vv_counters[peer_idx] += len as Counter;
        start_timestamp = start_timestamp.min(timestamp);
        end_timestamp = end_timestamp.max(timestamp);
        change_num += 1;
    }

    Ok(ImportBlobMetadata {
        is_snapshot,
        start_frontiers: frontiers,
        partial_start_vv: start_vv,
        partial_end_vv: VersionVector::from_iter(
            end_vv_counters
                .iter()
                .enumerate()
                .map(|(peer_idx, counter)| ID::new(peer_ids.peer_ids[peer_idx], *counter - 1)),
        ),
        start_timestamp,
        end_timestamp,
        change_num,
    })
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
    peer_ids: &PeerIdArena,
    mut deps: impl Iterator<Item = Result<EncodedDep, ColumnarError>> + 'a,
    mut ops_map: std::collections::HashMap<
        u64,
        Vec<Op>,
        std::hash::BuildHasherDefault<fxhash::FxHasher>,
    >,
) -> LoroResult<Vec<Change>> {
    let mut changes = Vec::with_capacity(encoded_changes.size_hint().0);
    for encoded_change in encoded_changes {
        let EncodedChange {
            peer_idx,
            mut len,
            timestamp,
            deps_len,
            dep_on_self,
            msg_len: _,
        } = encoded_change?;
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
            let dep = deps.next().ok_or(LoroError::DecodeDataCorruptionError)??;
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

#[allow(clippy::too_many_arguments)]
fn extract_ops(
    raw_values: &[u8],
    iter: impl Iterator<Item = Result<EncodedOp, ColumnarError>>,
    mut del_iter: impl Iterator<Item = Result<EncodedDeleteStartId, ColumnarError>>,
    shared_arena: &SharedArena,
    arenas: &DecodedArenas<'_>,
    should_extract_ops_with_ids: bool,
) -> LoroResult<ExtractedOps> {
    let mut value_reader = ValueReader::new(raw_values);
    let mut ops_map: FxHashMap<PeerID, Vec<Op>> = FxHashMap::default();
    let containers: Vec<_> = arenas
        .containers
        .iter()
        .map(|x| x.as_container_id(arenas))
        .try_collect()?;
    let mut ops = Vec::new();
    for op in iter {
        let EncodedOp {
            container_index,
            prop,
            peer_idx,
            value_type,
            counter,
        } = op?;
        if containers.len() <= container_index as usize
            || arenas.peer_ids.len() <= peer_idx as usize
        {
            return Err(LoroError::DecodeDataCorruptionError);
        }
        let peer = arenas.peer_ids[peer_idx as usize];
        let cid = &containers[container_index as usize];
        let kind = ValueKind::from_u8(value_type);
        let value = Value::decode(kind, &mut value_reader, arenas, ID::new(peer, counter))?;

        let content = decode_op(cid, value, &mut del_iter, shared_arena, arenas, prop)?;

        let container = shared_arena.register_container(cid);

        let op = Op {
            counter,
            container,
            content,
        };

        if should_extract_ops_with_ids {
            ops.push(OpWithId {
                peer,
                op: op.clone(),
                lamport: None,
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
    let key_register: ValueRegister<InternalString> = ValueRegister::new();
    let (start_counters, diff_changes) = init_encode(oplog, vv, &mut peer_register);
    let ExtractedContainer {
        containers,
        cid_idx_pairs: c_pairs,
        container_to_index: container_idx2index,
    } = extract_containers_in_order(
        &mut state.iter().map(|x| x.container_idx()).chain(
            diff_changes
                .iter()
                .flat_map(|x| x.ops.iter())
                .map(|x| x.container),
        ),
        &oplog.arena,
    );
    let cid_register: ValueRegister<ContainerID> = ValueRegister::from_existing(containers);
    let mut dep_arena = DepsArena::default();
    let mut value_writer = ValueWriter::new();
    let registers = Rc::new(RefCell::new(EncodedRegisters {
        peer: peer_register,
        container: cid_register,
        key: key_register,
    }));

    let registers_clone = registers.clone();

    // This stores the required op positions of each container state.
    // The states can be encoded in these positions in the next step.
    // This data structure stores that mapping from op id to the required total order.
    let mut origin_ops: Vec<TempOp<'_>> = Vec::new();
    let mut pos_mapping_heap: Vec<PosMappingItem> = Vec::new();
    let mut pos_target_value = 0;

    let mut states = Vec::new();
    let mut state_bytes = Vec::new();
    for (_, container) in c_pairs.iter() {
        let container_index = *container_idx2index.get(container).unwrap() as u32;

        let is_unknown = container.is_unknown();

        if is_unknown {
            states.push(EncodedStateInfo {
                container_index,
                op_len: 0,
                is_unknown,
                state_bytes_len: 0,
            });
            continue;
        }

        let state = match state.get_state(*container) {
            Some(state) if !state.is_state_empty() => state,
            _ => {
                states.push(EncodedStateInfo {
                    container_index,
                    op_len: 0,
                    is_unknown,
                    state_bytes_len: 0,
                });
                continue;
            }
        };

        let mut op_len = 0;
        let bytes = state.encode_snapshot(super::StateSnapshotEncoder {
            register_peer: &mut |peer| RefCell::borrow_mut(&registers).peer.register(&peer),
            check_idspan: &|_id_span| {
                // TODO: todo!("check intersection by vv that defined by idlp");
                // if let Some(counter) = vv.intersect_span(id_span) {
                //     Err(IdSpan {
                //         client_id: id_span.peer,
                //         counter,
                //     })
                // } else {
                Ok(())
                // }
            },
            encoder_by_op: &mut |op| {
                origin_ops.push(TempOp {
                    op: Cow::Owned(op.op),
                    peer_idx: RefCell::borrow_mut(&registers_clone)
                        .peer
                        .register(&op.peer) as u32,
                    peer_id: op.peer,
                    container_index,
                    prop_that_used_for_sort: -1,
                    lamport: op.lamport.unwrap(),
                });
            },
            record_idspan: &mut |id_span| {
                let len = id_span.atom_len();
                op_len += len;
                let start_id = oplog.idlp_to_id(IdLp::new(id_span.peer, id_span.lamport.start));
                pos_mapping_heap.push(PosMappingItem {
                    start_id: start_id.expect("convert idlp to id failed"),
                    len,
                    target_value: pos_target_value,
                });
                pos_target_value += len as i32;
            },
            mode: super::EncodeMode::Snapshot,
        });

        states.push(EncodedStateInfo {
            container_index,
            op_len: op_len as u32,
            is_unknown: false,
            state_bytes_len: bytes.len() as u32,
        });
        state_bytes.extend(bytes);
    }

    drop(registers_clone);
    let mut registers = match Rc::try_unwrap(registers) {
        Ok(r) => r.into_inner(),
        Err(_) => unreachable!(),
    };
    let changes = encode_changes(
        &diff_changes,
        &mut dep_arena,
        &mut |op| {
            origin_ops.push(op);
        },
        &container_idx2index,
        &mut registers,
    );

    let ops: Vec<TempOp> = calc_sorted_ops_for_snapshot(origin_ops, pos_mapping_heap);

    let (encoded_ops, del_starts) =
        encode_ops(ops, &oplog.arena, &mut value_writer, &mut registers);

    let EncodedRegisters {
        peer: mut peer_register,
        container: cid_register,
        key: mut key_register,
    } = registers;

    let container_arena = ContainerArena::from_containers(
        cid_register.unwrap_vec(),
        &mut peer_register,
        &mut key_register,
    );

    let doc = EncodedDoc {
        ops: encoded_ops,
        delete_starts: del_starts,
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
        start_frontiers: Vec::new(),
    };

    serde_columnar::to_vec(&doc).unwrap()
}

#[derive(Clone, Copy, PartialEq, Debug, Eq, PartialOrd, Ord)]
struct IdWithLamport {
    peer: PeerID,
    lamport: Lamport,
}

#[derive(Clone, Copy, PartialEq, Debug, Eq)]
struct PosMappingItem {
    start_id: ID,
    len: usize,
    target_value: i32,
}

impl Ord for PosMappingItem {
    fn cmp(&self, other: &Self) -> Ordering {
        // this is reversed so that the BinaryHeap will be a min-heap
        other.start_id.cmp(&self.start_id)
    }
}

impl PartialOrd for PosMappingItem {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PosMappingItem {
    fn split(&mut self, pos: usize) -> Self {
        let new_len = self.len - pos;
        self.len = pos;
        PosMappingItem {
            start_id: ID {
                peer: self.start_id.peer,
                counter: self.start_id.counter + pos as Counter,
            },
            len: new_len,
            target_value: self.target_value + pos as i32,
        }
    }
}

fn calc_sorted_ops_for_snapshot<'a>(
    mut origin_ops: Vec<TempOp<'a>>,
    mut pos_mapping_heap: Vec<PosMappingItem>,
) -> Vec<TempOp<'a>> {
    origin_ops.sort_unstable();
    pos_mapping_heap.sort_unstable();
    let mut ops: Vec<TempOp<'a>> = Vec::with_capacity(origin_ops.len());
    let ops_len: usize = origin_ops.iter().map(|x| x.atom_len()).sum();
    let mut origin_top = origin_ops.pop();
    let mut pos_top = pos_mapping_heap.pop();

    while origin_top.is_some() || pos_top.is_some() {
        let Some(mut inner_origin_top) = origin_top else {
            unreachable!()
        };

        let Some(mut inner_pos_top) = pos_top else {
            ops.push(inner_origin_top);
            origin_top = origin_ops.pop();
            continue;
        };
        match inner_origin_top.id_start().cmp(&inner_pos_top.start_id) {
            std::cmp::Ordering::Less => {
                if inner_origin_top.id_end() <= inner_pos_top.start_id {
                    ops.push(inner_origin_top);
                    origin_top = origin_ops.pop();
                } else {
                    let delta =
                        inner_pos_top.start_id.counter - inner_origin_top.id_start().counter;
                    let right = inner_origin_top.split(delta as usize);
                    ops.push(inner_origin_top);
                    origin_top = Some(right);
                }
            }
            std::cmp::Ordering::Equal => {
                match inner_origin_top.atom_len().cmp(&inner_pos_top.len) {
                    std::cmp::Ordering::Less => {
                        // origin top is shorter than pos mapping,
                        // need to split the pos mapping
                        let len = inner_origin_top.atom_len();
                        inner_origin_top.prop_that_used_for_sort =
                            i32::MIN + inner_pos_top.target_value;
                        ops.push(inner_origin_top);
                        let next = inner_pos_top.split(len);
                        origin_top = origin_ops.pop();
                        pos_top = Some(next);
                    }
                    std::cmp::Ordering::Equal => {
                        // origin op's length equal to pos mapping's length
                        inner_origin_top.prop_that_used_for_sort =
                            i32::MIN + inner_pos_top.target_value;
                        ops.push(inner_origin_top.clone());
                        origin_top = origin_ops.pop();
                        pos_top = pos_mapping_heap.pop();
                    }
                    std::cmp::Ordering::Greater => {
                        // origin top is longer than pos mapping,
                        // need to split the origin top
                        let right = inner_origin_top.split(inner_pos_top.len);
                        inner_origin_top.prop_that_used_for_sort =
                            i32::MIN + inner_pos_top.target_value;
                        ops.push(inner_origin_top);
                        origin_top = Some(right);
                        pos_top = pos_mapping_heap.pop();
                    }
                }
            }
            std::cmp::Ordering::Greater => unreachable!(),
        }
    }

    ops.sort_unstable_by(|a, b| {
        a.container_index.cmp(&b.container_index).then({
            a.prop_that_used_for_sort
                .cmp(&b.prop_that_used_for_sort)
                .then_with(|| a.peer_idx.cmp(&b.peer_idx))
                .then_with(|| a.lamport.cmp(&b.lamport))
        })
    });

    debug_assert_eq!(ops.iter().map(|x| x.atom_len()).sum::<usize>(), ops_len);
    ops
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
    let arenas = decode_arena(&iter.arenas)?;
    let ExtractedOps {
        ops_map,
        mut ops,
        containers,
    } = extract_ops(
        &iter.raw_values,
        iter.ops,
        iter.delete_starts,
        &oplog.arena,
        &arenas,
        true,
    )?;
    let DecodedArenas {
        peer_ids,
        deps,
        state_blob_arena,
        ..
    } = arenas;

    let changes = decode_changes(iter.changes, iter.start_counters, &peer_ids, deps, ops_map)?;
    let (new_ids, pending_changes) = import_changes_to_oplog(changes, &mut oplog)?;

    for op in ops.iter_mut() {
        // update op's lamport
        op.lamport = oplog.get_lamport_at(op.id());
    }

    decode_snapshot_states(
        &mut state,
        oplog.frontiers().clone(),
        iter.states,
        containers,
        state_blob_arena,
        ops,
        &oplog,
        &peer_ids,
    )
    .unwrap();

    assert!(pending_changes.is_empty());
    // we cannot assert this because frontiers of oplog is not updated yet when batch_importing
    // assert_eq!(&state.frontiers, oplog.frontiers());
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

#[allow(clippy::too_many_arguments)]
fn decode_snapshot_states(
    state: &mut DocState,
    frontiers: Frontiers,
    encoded_state_iter: IterableEncodedStateInfo<'_>,
    containers: Vec<ContainerID>,
    state_blob_arena: &[u8],
    ops: Vec<OpWithId>,
    oplog: &std::sync::MutexGuard<'_, OpLog>,
    peers: &PeerIdArena,
) -> LoroResult<()> {
    let mut state_blob_index: usize = 0;
    let mut ops_index: usize = 0;
    let mut unknown_containers = Vec::new();
    for encoded_state in encoded_state_iter {
        let EncodedStateInfo {
            container_index,
            mut op_len,
            is_unknown,
            state_bytes_len,
        } = encoded_state?;
        if is_unknown {
            let container_id = containers[container_index as usize].clone();
            let container = state.arena.register_container(&container_id);
            unknown_containers.push(container);
            if container.is_unknown() {
                state.init_unknown_container(container_id);
            }
            continue;
        }
        if op_len == 0 && state_bytes_len == 0 {
            continue;
        }

        if container_index >= containers.len() as u32 {
            return Err(LoroError::DecodeDataCorruptionError);
        }

        let container_id = &containers[container_index as usize];

        let container = state.arena.register_container(container_id);

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
            .skip_while(|x| x.op.container != container)
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
                peers: &peers.peer_ids,
            },
        );
    }

    let s = take(&mut state.states);
    state.init_with_states_and_version(s, frontiers, oplog, unknown_containers);
    Ok(())
}

mod encode {
    use fxhash::FxHashMap;
    use loro_common::{ContainerType, HasId, PeerID, ID};
    use rle::{HasLength, Sliceable};
    use std::borrow::Cow;

    use crate::{
        arena::SharedArena,
        change::{Change, Lamport},
        container::idx::ContainerIdx,
        encoding::value::{EncodedTreeMove, FutureValue, MarkStart, Value, ValueKind, ValueWriter},
        op::{FutureInnerContent, Op},
    };

    #[derive(Debug, Clone)]
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

    impl PartialEq for TempOp<'_> {
        fn eq(&self, other: &Self) -> bool {
            self.peer_id == other.peer_id && self.lamport == other.lamport
        }
    }

    impl Eq for TempOp<'_> {}
    impl Ord for TempOp<'_> {
        fn cmp(&self, other: &Self) -> std::cmp::Ordering {
            self.peer_id
                .cmp(&other.peer_id)
                .then(self.lamport.cmp(&other.lamport))
                // we need reverse because we'll need to use binary heap to get the smallest one
                .reverse()
        }
    }

    impl PartialOrd for TempOp<'_> {
        fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
            Some(self.cmp(other))
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
        arena: &SharedArena,
        value_writer: &mut ValueWriter,
        registers: &mut EncodedRegisters,
    ) -> (Vec<EncodedOp>, Vec<EncodedDeleteStartId>) {
        let mut encoded_ops = Vec::with_capacity(ops.len());
        let mut delete_start = Vec::new();
        for TempOp {
            op,
            peer_idx,
            container_index,
            ..
        } in ops
        {
            let value_type = encode_op(&op, arena, &mut delete_start, value_writer, registers);
            let prop = get_op_prop(&op, registers);
            encoded_ops.push(EncodedOp {
                container_index,
                peer_idx,
                counter: op.counter,
                prop,
                value_type: value_type.to_u8(),
            });
        }

        (encoded_ops, delete_start)
    }

    pub(super) fn encode_changes<'a>(
        diff_changes: &'a [Cow<'a, Change>],
        dep_arena: &mut super::DepsArena,
        push_op: &mut impl FnMut(TempOp<'a>),
        container_idx2index: &FxHashMap<ContainerIdx, usize>,
        registers: &mut EncodedRegisters,
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
                    dep_arena.push(registers.peer.register(&dep.peer), dep.counter);
                }
            }

            let peer_idx = registers.peer.register(&change.id.peer);
            changes.push(EncodedChange {
                dep_on_self,
                deps_len,
                peer_idx,
                len: change.atom_len(),
                timestamp: change.timestamp,
                msg_len: 0,
            });

            for op in change.ops().iter() {
                let lamport = (op.counter - change.id.counter) as Lamport + change.lamport();
                push_op(TempOp {
                    op: Cow::Borrowed(op),
                    lamport,
                    prop_that_used_for_sort: get_sorting_prop(op, registers),
                    peer_idx: peer_idx as u32,
                    peer_id: change.id.peer,
                    container_index: container_idx2index[&op.container] as u32,
                });
            }
        }
        changes
    }

    use crate::{OpLog, VersionVector};
    pub(crate) use value_register::ValueRegister;

    use super::{EncodedChange, EncodedDeleteStartId, EncodedOp, EncodedRegisters};
    mod value_register {
        use fxhash::FxHashMap;

        pub struct ValueRegister<T> {
            map_value_to_index: FxHashMap<T, usize>,
            vec: Vec<T>,
        }

        impl<T: std::hash::Hash + Clone + PartialEq + Eq> Default for ValueRegister<T> {
            fn default() -> Self {
                Self::new()
            }
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
        let start_vv = vv.trim(oplog.vv());
        let mut start_counters = Vec::new();

        let mut diff_changes: Vec<Cow<'a, Change>> = Vec::new();
        for change in oplog.iter_changes_peer_by_peer(&start_vv, self_vv) {
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

    fn get_future_op_prop(op: &FutureInnerContent) -> i32 {
        match &op {
            #[cfg(feature = "counter")]
            FutureInnerContent::Counter(c) => *c as i32,
            FutureInnerContent::Unknown { .. } => 0,
        }
    }

    fn get_op_prop(op: &Op, registers: &mut EncodedRegisters) -> i32 {
        match &op.content {
            crate::op::InnerContent::List(list) => match list {
                crate::container::list::list_op::InnerListOp::Move { to, .. } => *to as i32,
                crate::container::list::list_op::InnerListOp::Set { .. } => 0,
                crate::container::list::list_op::InnerListOp::Insert { pos, .. } => *pos as i32,
                crate::container::list::list_op::InnerListOp::InsertText { pos, .. } => *pos as i32,
                crate::container::list::list_op::InnerListOp::Delete(span) => span.span.pos as i32,
                crate::container::list::list_op::InnerListOp::StyleStart { start, .. } => {
                    *start as i32
                }
                crate::container::list::list_op::InnerListOp::StyleEnd => 0,
            },
            crate::op::InnerContent::Map(map) => {
                let key = registers.key.register(&map.key);
                key as i32
            }
            crate::op::InnerContent::Tree(_) => 0,
            // The future should not use register to encode prop
            crate::op::InnerContent::Future(f) => get_future_op_prop(f),
        }
    }

    fn get_sorting_prop(op: &Op, registers: &mut EncodedRegisters) -> i32 {
        match &op.content {
            crate::op::InnerContent::List(_) => 0,
            crate::op::InnerContent::Map(map) => {
                let key = registers.key.register(&map.key);
                key as i32
            }
            crate::op::InnerContent::Tree(..) => 0,
            crate::op::InnerContent::Future(f) => match f {
                #[cfg(feature = "counter")]
                FutureInnerContent::Counter(_) => 0,
                FutureInnerContent::Unknown { .. } => 0,
            },
        }
    }

    #[inline]
    fn encode_op(
        op: &Op,
        arena: &SharedArena,
        delete_start: &mut Vec<EncodedDeleteStartId>,
        value_writer: &mut ValueWriter,
        registers: &mut EncodedRegisters,
    ) -> ValueKind {
        let value = match &op.content {
            crate::op::InnerContent::List(list) => match list {
                crate::container::list::list_op::InnerListOp::Insert { slice, .. } => {
                    assert!(matches!(
                        op.container.get_type(),
                        ContainerType::List | ContainerType::MovableList
                    ));
                    let value = arena.get_values(slice.0.start as usize..slice.0.end as usize);
                    Value::LoroValueArray(value)
                }
                crate::container::list::list_op::InnerListOp::InsertText {
                    slice,
                    unicode_start: _,
                    unicode_len: _,
                    ..
                } => {
                    // TODO: refactor this from_utf8 can be done internally without checking
                    Value::Str(std::str::from_utf8(slice.as_bytes()).unwrap())
                }
                crate::container::list::list_op::InnerListOp::Delete(span) => {
                    delete_start.push(EncodedDeleteStartId {
                        peer_idx: registers.peer.register(&span.id_start.peer),
                        counter: span.id_start.counter,
                        len: span.span.signed_len,
                    });
                    Value::DeleteSeq
                }
                crate::container::list::list_op::InnerListOp::StyleStart {
                    start,
                    end,
                    key,
                    value,
                    info,
                } => Value::MarkStart(MarkStart {
                    len: *end - *start,
                    key: key.clone(),
                    value: value.clone(),
                    info: info.to_byte(),
                }),
                crate::container::list::list_op::InnerListOp::Set { elem_id, value } => {
                    Value::ListSet {
                        peer_idx: registers.peer.register(&elem_id.peer),
                        lamport: elem_id.lamport,
                        value: value.clone(),
                    }
                }
                crate::container::list::list_op::InnerListOp::StyleEnd => Value::Null,
                crate::container::list::list_op::InnerListOp::Move {
                    from,
                    from_id,
                    to: _,
                } => Value::ListMove {
                    from: *from as usize,
                    from_idx: registers.peer.register(&from_id.peer),
                    lamport: from_id.lamport as usize,
                },
            },
            crate::op::InnerContent::Map(map) => {
                assert_eq!(op.container.get_type(), ContainerType::Map);
                match &map.value {
                    Some(v) => Value::LoroValue(v.clone()),
                    None => Value::DeleteOnce,
                }
            }
            crate::op::InnerContent::Tree(t) => {
                assert_eq!(op.container.get_type(), ContainerType::Tree);
                Value::TreeMove(EncodedTreeMove::from_op(t))
            }
            crate::op::InnerContent::Future(f) => match f {
                #[cfg(feature = "counter")]
                FutureInnerContent::Counter(_) => Value::Future(FutureValue::Counter),
                FutureInnerContent::Unknown { prop: _, value } => Value::from_owned(value),
            },
        };
        let (k, _) = value.encode(value_writer, registers);
        k
    }
}

#[allow(clippy::too_many_arguments)]
#[inline]
fn decode_op(
    cid: &ContainerID,
    value: Value<'_>,
    del_iter: &mut impl Iterator<Item = Result<EncodedDeleteStartId, ColumnarError>>,
    shared_arena: &SharedArena,
    arenas: &DecodedArenas<'_>,
    prop: i32,
) -> LoroResult<crate::op::InnerContent> {
    let content = match cid.container_type() {
        ContainerType::Text => match value {
            Value::Str(s) => {
                let (slice, result) = shared_arena.alloc_str_with_slice(s);
                crate::op::InnerContent::List(
                    crate::container::list::list_op::InnerListOp::InsertText {
                        slice,
                        unicode_start: result.start as u32,
                        unicode_len: (result.end - result.start) as u32,
                        pos: prop as u32,
                    },
                )
            }
            Value::DeleteSeq => {
                let del_start = del_iter.next().unwrap()?;
                let peer_idx = del_start.peer_idx;
                let cnt = del_start.counter;
                let len = del_start.len;
                crate::op::InnerContent::List(crate::container::list::list_op::InnerListOp::Delete(
                    DeleteSpanWithId::new(
                        ID::new(arenas.peer_ids.peer_ids[peer_idx], cnt as Counter),
                        prop as isize,
                        len,
                    ),
                ))
            }
            Value::MarkStart(mark) => crate::op::InnerContent::List(
                crate::container::list::list_op::InnerListOp::StyleStart {
                    start: prop as u32,
                    end: prop as u32 + mark.len,
                    key: mark.key,
                    value: mark.value,
                    info: TextStyleInfoFlag::from_byte(mark.info),
                },
            ),
            Value::Null => crate::op::InnerContent::List(
                crate::container::list::list_op::InnerListOp::StyleEnd,
            ),
            _ => unreachable!(),
        },
        ContainerType::Map => {
            let key = arenas
                .keys
                .keys
                .get(prop as usize)
                .ok_or(LoroError::DecodeDataCorruptionError)?
                .clone();
            match value {
                Value::DeleteOnce => {
                    crate::op::InnerContent::Map(crate::container::map::MapSet { key, value: None })
                }
                Value::LoroValue(v) => {
                    crate::op::InnerContent::Map(crate::container::map::MapSet {
                        key,
                        value: Some(v.clone()),
                    })
                }
                _ => unreachable!(),
            }
        }
        ContainerType::List => {
            let pos = prop as usize;
            match value {
                Value::LoroValueArray(arr) => {
                    let range = shared_arena.alloc_values(arr.into_iter());
                    crate::op::InnerContent::List(
                        crate::container::list::list_op::InnerListOp::Insert {
                            slice: SliceRange::new(range.start as u32..range.end as u32),
                            pos,
                        },
                    )
                }
                Value::DeleteSeq => {
                    let del_start = del_iter.next().unwrap()?;
                    let peer_idx = del_start.peer_idx;
                    let cnt = del_start.counter;
                    let len = del_start.len;
                    crate::op::InnerContent::List(
                        crate::container::list::list_op::InnerListOp::Delete(
                            DeleteSpanWithId::new(
                                ID::new(arenas.peer_ids[peer_idx], cnt as Counter),
                                pos as isize,
                                len,
                            ),
                        ),
                    )
                }
                _ => unreachable!(),
            }
        }
        ContainerType::Tree => match value {
            Value::TreeMove(op) => crate::op::InnerContent::Tree(op.as_tree_op()),
            _ => {
                unreachable!()
            }
        },
        ContainerType::MovableList => {
            let pos = prop as usize;
            match value {
                Value::LoroValueArray(arr) => {
                    let range = shared_arena.alloc_values(arr.into_iter());
                    crate::op::InnerContent::List(
                        crate::container::list::list_op::InnerListOp::Insert {
                            slice: SliceRange::new(range.start as u32..range.end as u32),
                            pos,
                        },
                    )
                }
                Value::DeleteSeq => {
                    let del_start = del_iter.next().unwrap()?;
                    let peer_idx = del_start.peer_idx;
                    let cnt = del_start.counter;
                    let len = del_start.len;
                    crate::op::InnerContent::List(
                        crate::container::list::list_op::InnerListOp::Delete(
                            DeleteSpanWithId::new(
                                ID::new(arenas.peer_ids[peer_idx], cnt as Counter),
                                pos as isize,
                                len,
                            ),
                        ),
                    )
                }
                Value::ListMove {
                    from,
                    from_idx,
                    lamport,
                } => crate::op::InnerContent::List(
                    crate::container::list::list_op::InnerListOp::Move {
                        from: from as u32,
                        from_id: IdLp::new(arenas.peer_ids[from_idx], lamport as Lamport),
                        to: prop as u32,
                    },
                ),
                Value::ListSet {
                    peer_idx,
                    lamport,
                    value,
                } => crate::op::InnerContent::List(
                    crate::container::list::list_op::InnerListOp::Set {
                        elem_id: IdLp::new(arenas.peer_ids[peer_idx], lamport as Lamport),
                        value,
                    },
                ),
                _ => unreachable!(),
            }
        }
        #[cfg(feature = "counter")]
        ContainerType::Counter => {
            crate::op::InnerContent::Future(FutureInnerContent::Counter(prop as i64))
        }

        ContainerType::Unknown(_) => crate::op::InnerContent::Future(FutureInnerContent::Unknown {
            prop,
            value: value.into_owned(),
        }),
    };

    Ok(content)
}

pub type PeerIdx = usize;

struct ExtractedContainer {
    containers: Vec<ContainerID>,
    cid_idx_pairs: Vec<(ContainerID, ContainerIdx)>,
    container_to_index: FxHashMap<ContainerIdx, usize>,
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
        container_to_index: container_idx2index,
    }
}

#[columnar(ser, de)]
struct EncodedDoc<'a> {
    #[columnar(class = "vec", iter = "EncodedOp")]
    ops: Vec<EncodedOp>,
    #[columnar(class = "vec", iter = "EncodedChange")]
    changes: Vec<EncodedChange>,
    #[columnar(class = "vec", iter = "EncodedDeleteStartId")]
    delete_starts: Vec<EncodedDeleteStartId>,
    /// Container states snapshot.
    ///
    /// It's empty when the encoding mode is not snapshot.
    #[columnar(class = "vec", iter = "EncodedStateInfo")]
    states: Vec<EncodedStateInfo>,
    /// The first counter value for each change of each peer in `changes`
    start_counters: Vec<Counter>,
    /// The frontiers at the start of this encoded delta.
    ///
    /// It's empty when the encoding mode is snapshot.
    start_frontiers: Vec<(PeerIdx, Counter)>,
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
    #[columnar(strategy = "DeltaRle")]
    peer_idx: u32,
    #[columnar(strategy = "DeltaRle")]
    value_type: u8,
    #[columnar(strategy = "DeltaRle")]
    counter: i32,
}

#[columnar(vec, ser, de, iterable)]
#[derive(Debug, Clone)]
struct EncodedDeleteStartId {
    #[columnar(strategy = "DeltaRle")]
    peer_idx: usize,
    #[columnar(strategy = "DeltaRle")]
    counter: i32,
    #[columnar(strategy = "DeltaRle")]
    len: isize,
}

#[columnar(vec, ser, de, iterable)]
#[derive(Debug, Clone)]
struct EncodedChange {
    #[columnar(strategy = "DeltaRle")]
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
    #[columnar(strategy = "BoolRle")]
    is_unknown: bool,
}

#[cfg(test)]
mod test {
    use std::sync::Arc;

    use loro_common::LoroValue;

    use crate::fx_map;

    use super::*;

    fn test_loro_value_read_write(v: impl Into<LoroValue>, container_id: Option<ContainerID>) {
        let v = v.into();
        let id = match &container_id {
            Some(ContainerID::Root { .. }) => ID::new(u64::MAX, 0),
            Some(ContainerID::Normal { peer, counter, .. }) => ID::new(*peer, *counter),
            None => ID::new(u64::MAX, 0),
        };

        let mut registers = EncodedRegisters {
            key: ValueRegister::new(),
            container: ValueRegister::new(),
            peer: ValueRegister::new(),
        };

        let mut writer = ValueWriter::new();
        let (kind, _) = writer.write_value_content(&v, &mut registers);

        let binding = writer.finish();
        let mut reader = ValueReader::new(binding.as_slice());

        let ans = reader
            .read_value_content(kind, &registers.key.unwrap_vec(), id)
            .unwrap();
        assert_eq!(v, ans)
    }

    #[test]
    fn test_value_read_write() {
        test_loro_value_read_write(true, None);
        test_loro_value_read_write(false, None);
        test_loro_value_read_write(123, None);
        test_loro_value_read_write(1.23, None);
        test_loro_value_read_write(LoroValue::Null, None);
        test_loro_value_read_write(
            LoroValue::Binary(Arc::new(vec![123, 223, 255, 0, 1, 2, 3])),
            None,
        );
        test_loro_value_read_write("sldk;ajfas;dlkfas测试", None);
        // we won't encode root container by `value content`
        // test_loro_value_read_write(
        //     LoroValue::Container(ContainerID::new_root("name", ContainerType::Text)),
        //     Some(ContainerID::new_root("name", ContainerType::Text)),
        // );
        test_loro_value_read_write(
            LoroValue::Container(ContainerID::new_normal(
                ID::new(u64::MAX, 123),
                ContainerType::Tree,
            )),
            Some(ContainerID::new_normal(
                ID::new(u64::MAX, 123),
                ContainerType::Tree,
            )),
        );
        test_loro_value_read_write(vec![1i32, 2, 3], None);
        test_loro_value_read_write(
            LoroValue::Map(Arc::new(fx_map![
                "1".into() => 123.into(),
                "2".into() => "123".into(),
                "3".into() => vec![true].into()
            ])),
            None,
        );
    }
}

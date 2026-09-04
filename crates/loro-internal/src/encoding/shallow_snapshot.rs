use bytes::Bytes;
use rle::HasLength;
use rustc_hash::FxHashSet;
use std::collections::BTreeSet;

use loro_common::{ContainerID, ContainerType, IdSpan, LoroEncodeError, LoroError, ID};

use crate::{
    container::{idx::ContainerIdx, list::list_op::InnerListOp},
    dag::DagUtils,
    encoding::export_fast_updates_in_range,
    encoding::fast_snapshot::{_encode_snapshot, Snapshot},
    state::{
        container_store::{ContainerWrapper, FRONTIERS_KEY},
        redact_dead_style_values, DocState,
    },
    utils::kv_wrapper::KvWrapper,
    version::{Frontiers, VersionVector},
    LoroDoc,
};

#[cfg(test)]
const MAX_OPS_NUM_TO_ENCODE_WITHOUT_LATEST_STATE: usize = 16;
#[cfg(not(test))]
const MAX_OPS_NUM_TO_ENCODE_WITHOUT_LATEST_STATE: usize = 256;

/// Minimum number of retained ops (root..latest) for the forward-replay fast
/// path to be worth it. Below this, the old checkout path is used: at F ==
/// latest it is trivial (no history to walk back), and on the 66k-container /
/// 720k-op fixture the paths tie at ~8k retained ops while the checkout path
/// peaks at ~4x less memory; forward replay only wins decisively past ~64k
/// (1.25-3.6x at 79k, 10-14x at 393k). Note this fixture has tiny
/// per-container histories; docs with long text histories penalize the
/// checkout path more, shifting the real crossover lower.
#[cfg(test)]
const MIN_RETAINED_OPS_FOR_FORWARD_ROOT_STATE: usize = 16;
#[cfg(not(test))]
const MIN_RETAINED_OPS_FOR_FORWARD_ROOT_STATE: usize = 65536;

/// The fast path re-encodes and replays ALL pre-root history into a temporary
/// doc, so its cost scales with the prefix, not the tail. Cap the prefix/tail
/// ratio: on the fixture the fast path wins at ratio 9 and loses at ratio 19,
/// and an unrelated huge prefix (e.g. millions of same-key Map overwrites
/// before the root) must not be replayed just because the tail is large.
const MAX_PRE_ROOT_TO_RETAINED_OPS_RATIO: usize = 16;

/// Absolute cap on pre-root ops for the forward-replay path. Replaying ~1M ops
/// costs roughly 0.5-1s and several hundred MiB of peak memory; beyond that
/// the checkout path is safer (its work is bounded by the tail), especially
/// on wasm32.
const MAX_PRE_ROOT_OPS_FOR_FORWARD_REPLAY: usize = 1_000_000;

/// Cap on the pre-root prefix's decoded byte size, estimated by walking op
/// payloads by reference BEFORE any value is copied (see
/// `estimate_ops_content_bytes`). Op counts miss value sizes: a Map write is
/// one atom regardless of how large its Binary/String value is, so a
/// byte-heavy but low-op prefix would bypass the op-count gates. Checking only
/// the encoded blob afterwards would be too late —
/// `export_fast_updates_in_range` slice-copies values into a fresh store while
/// building it, which is exactly the allocation this cap exists to prevent.
#[cfg(test)]
const MAX_PRE_ROOT_BYTES_FOR_FORWARD_REPLAY: usize = 1 << 20;
#[cfg(not(test))]
const MAX_PRE_ROOT_BYTES_FOR_FORWARD_REPLAY: usize = 32 << 20;

/// Estimate the decoded byte size of everything in `spans` — op payloads
/// (recursing into nested `LoroValue::List`/`Map`), style values, and commit
/// messages — following arena slices by reference and never copying values.
/// The walk is cap-aware: every counting step takes a remaining budget and
/// bails as soon as it is exceeded, so a rejected prefix costs one bounded
/// walk instead of a full copy.
fn estimate_ops_content_bytes(oplog: &crate::OpLog, spans: &[IdSpan], cap: usize) -> usize {
    let mut total = 0usize;
    'outer: for span in spans {
        let mut span = *span;
        span.normalize_();
        if span.counter.end <= 0 {
            continue;
        }

        span.counter.start = span.counter.start.max(0);
        span.counter.end = span.counter.end.max(0);
        if span.counter.start >= span.counter.end {
            continue;
        }

        for change in oplog.iter_changes(span) {
            let start =
                ((span.counter.start - change.id.counter).max(0) as usize).min(change.atom_len());
            let end =
                ((span.counter.end - change.id.counter).max(0) as usize).min(change.atom_len());
            if start == end {
                continue;
            }

            if let Some(msg) = change.commit_msg.as_ref() {
                total = total.saturating_add(msg.len());
                if total > cap {
                    break 'outer;
                }
            }

            for op in crate::op::RichOp::new_iter_by_cnt_range(change, span.counter) {
                total = total.saturating_add(rich_op_content_bytes(oplog, &op, cap - total));
                if total > cap {
                    break 'outer;
                }
            }
        }
    }
    total
}

/// Byte size of an op's variable-length fields plus a small flat per-op
/// overhead: everything the block encoder copies (payload values, map and
/// style keys, fractional indexes, root container names, unknown-future
/// bytes). Arena slices are measured by reference; nothing is copied.
/// `remaining` is the budget left before the cap; the count may stop early
/// once it is exceeded.
fn rich_op_content_bytes(oplog: &crate::OpLog, op: &crate::op::RichOp, remaining: usize) -> usize {
    const PER_OP_OVERHEAD: usize = 16;
    if remaining < PER_OP_OVERHEAD {
        return remaining + 1;
    }

    // Root container names are variable-length and copied into the encoded
    // block's container arena.
    let container_name_len = match oplog.arena.get_container_id(op.raw_op().container) {
        Some(ContainerID::Root { name, .. }) => name.len(),
        _ => 0,
    };
    let after_fixed = remaining.saturating_sub(PER_OP_OVERHEAD + container_name_len);

    PER_OP_OVERHEAD
        + container_name_len
        + match &op.raw_op().content {
            crate::op::InnerContent::Map(map) => {
                let mut used = map.key.len();
                if let Some(value) = map.value.as_ref() {
                    used = used.saturating_add(value_bytes_capped(
                        value,
                        after_fixed.saturating_sub(used),
                    ));
                }
                used
            }
            crate::op::InnerContent::List(list) => match list {
                InnerListOp::Insert { slice, .. } => oplog.arena.with_values(
                    slice.0.start as usize..slice.0.end as usize,
                    |values| {
                        let mut used = 0usize;
                        for value in values {
                            used = used.saturating_add(value_bytes_capped(
                                value,
                                after_fixed.saturating_sub(used),
                            ));
                            if used > after_fixed {
                                break;
                            }
                        }
                        used
                    },
                ),
                InnerListOp::InsertText { slice, .. } => slice.len(),
                InnerListOp::Set { value, .. } => value_bytes_capped(value, after_fixed),
                InnerListOp::StyleStart { key, value, .. } => {
                    let mut used = key.len();
                    used = used.saturating_add(value_bytes_capped(
                        value,
                        after_fixed.saturating_sub(used),
                    ));
                    used
                }
                _ => 0,
            },
            // Fractional indexes are variable-length and copied by the encoder.
            crate::op::InnerContent::Tree(tree_op) => match tree_op.as_ref() {
                crate::container::tree::tree_op::TreeOp::Create { position, .. }
                | crate::container::tree::tree_op::TreeOp::Move { position, .. } => {
                    position.as_bytes().len()
                }
                crate::container::tree::tree_op::TreeOp::Delete { .. } => 0,
            },
            crate::op::InnerContent::Future(future) => match future {
                crate::op::FutureInnerContent::Unknown { value, .. } => {
                    owned_value_bytes_capped(value, after_fixed)
                }
                #[cfg(feature = "counter")]
                crate::op::FutureInnerContent::Counter(_) => 0,
            },
        }
}

/// Byte size of an owned future/unknown value, recursing where the payload is
/// a `LoroValue`. `remaining` is the budget left before the cap.
fn owned_value_bytes_capped(value: &crate::encoding::value::OwnedValue, remaining: usize) -> usize {
    use crate::encoding::value::OwnedValue;
    const OVERHEAD: usize = 16;
    if remaining < OVERHEAD {
        return remaining + 1;
    }

    match value {
        OwnedValue::Str(s) => OVERHEAD.saturating_add(s.len()),
        OwnedValue::Binary(b) => OVERHEAD.saturating_add(b.len()),
        OwnedValue::LoroValue(v) => OVERHEAD.saturating_add(value_bytes_capped(v, remaining)),
        OwnedValue::Future(owned) => match owned {
            crate::encoding::value::OwnedFutureValue::Unknown { data, .. } => {
                OVERHEAD.saturating_add(data.len())
            }
        },
        _ => OVERHEAD,
    }
}

/// Byte size of a value, recursing into nested lists and maps (the encoder
/// writes them recursively, so the estimate must too). `remaining` is the
/// budget left before the cap; the count may stop early once it is exceeded.
fn value_bytes_capped(value: &loro_common::LoroValue, remaining: usize) -> usize {
    const OVERHEAD: usize = 16;
    if remaining < OVERHEAD {
        return remaining + 1;
    }

    let mut used = OVERHEAD;
    match value {
        loro_common::LoroValue::String(s) => used = used.saturating_add(s.len()),
        loro_common::LoroValue::Binary(b) => used = used.saturating_add(b.len()),
        loro_common::LoroValue::List(list) => {
            for item in list.iter() {
                used =
                    used.saturating_add(value_bytes_capped(item, remaining.saturating_sub(used)));
                if used > remaining {
                    break;
                }
            }
        }
        loro_common::LoroValue::Map(map) => {
            for (key, item) in map.iter() {
                used = used.saturating_add(key.len());
                if used > remaining {
                    break;
                }
                used =
                    used.saturating_add(value_bytes_capped(item, remaining.saturating_sub(used)));
                if used > remaining {
                    break;
                }
            }
        }
        _ => {}
    }
    used
}

/// Whether the forward-replay path may be used for the given prefix/tail
/// shape. The caller still has to require a non-shallow doc whose state is at
/// the latest version. Extracted as a pure predicate so the gate can be
/// tested directly.
fn forward_replay_gate(ops_num: usize, pre_root_ops: usize, pre_root_bytes: usize) -> bool {
    ops_num >= MIN_RETAINED_OPS_FOR_FORWARD_ROOT_STATE
        && pre_root_ops <= MAX_PRE_ROOT_OPS_FOR_FORWARD_REPLAY
        && pre_root_ops <= MAX_PRE_ROOT_TO_RETAINED_OPS_RATIO * ops_num
        && pre_root_bytes <= MAX_PRE_ROOT_BYTES_FOR_FORWARD_REPLAY
}

#[tracing::instrument(skip_all)]
pub(crate) fn export_shallow_snapshot<W: std::io::Write>(
    doc: &LoroDoc,
    start_from: &Frontiers,
    w: &mut W,
) -> Result<Frontiers, LoroEncodeError> {
    let (snapshot, start_from) = export_shallow_snapshot_inner(doc, start_from)?;
    _encode_snapshot(&snapshot, w);
    Ok(start_from)
}

pub(crate) fn export_shallow_snapshot_inner(
    doc: &LoroDoc,
    start_from: &Frontiers,
) -> Result<(Snapshot, Frontiers), LoroEncodeError> {
    let oplog = doc.oplog().lock();
    let start_from = calc_shallow_doc_start(&oplog, start_from);
    // `root_vv` is the version of the state at the shallow root; `start_vv`
    // additionally excludes the frontier ops themselves because the retained
    // history must include them.
    let root_vv = frontiers_to_vv_for_export(&oplog, &start_from, "export_shallow_snapshot")?;
    let mut start_vv = root_vv.clone();
    for id in start_from.iter() {
        // we need to include the ops in start_from, this can make things easier
        start_vv.insert(id.peer, id.counter);
    }

    #[cfg(debug_assertions)]
    {
        use crate::dag::Dag;
        if !start_from.is_empty() {
            assert!(start_from.len() == 1);
            let id = start_from.as_single().unwrap();
            let node = oplog.dag.get(id).unwrap();
            if id.counter == node.cnt {
                let vv = oplog.dag().frontiers_to_vv(&node.deps).unwrap();
                assert_eq!(vv, start_vv);
            } else {
                let vv = oplog
                    .dag()
                    .frontiers_to_vv(&Frontiers::from(id.inc(-1)))
                    .unwrap();
                assert_eq!(vv, start_vv);
            }
        }
    }

    loro_common::debug!(
        "start version vv={:?} frontiers={:?}",
        &start_vv,
        &start_from,
    );

    let latest_frontiers = oplog.frontiers().clone();
    let state_frontiers = doc.state_frontiers();
    let is_attached = !doc.is_detached();
    let oplog_bytes = oplog.export_change_store_from(&start_vv, &start_from);
    let latest_vv = oplog.vv();
    let ops_num: usize = latest_vv.sub_iter(&start_vv).map(|x| x.atom_len()).sum();
    // Pre-encode the pre-root history for the forward-replay path below while
    // the oplog lock is held. Calling `LoroDoc::export` there instead would
    // re-enter `with_barrier` and violate the txn lock order. Shallow docs are
    // excluded: their pre-root history is trimmed, so forward replay from
    // empty cannot reconstruct the root state. Small retained ranges are
    // excluded too: the checkout path is then cheap and peaks at far less
    // memory (see MIN_RETAINED_OPS_FOR_FORWARD_ROOT_STATE). The prefix itself
    // is bounded absolutely and relative to the tail, because this path pays
    // for replaying all of it while the checkout path only walks the tail.
    let pre_root_ops: usize = root_vv
        .iter()
        .map(|(_, counter)| (*counter).max(0) as usize)
        .sum();
    // Cheap op-count legs first (even the estimate walks the prefix's ops, so
    // it must not run for the small-tail cases the checkout path handles
    // best). The byte leg runs BEFORE encoding: the estimate follows arena
    // slices by reference, while export_fast_updates_in_range would
    // slice-copy every value into a fresh store — the very allocation the cap
    // exists to prevent.
    let pre_root_updates = (state_frontiers == latest_frontiers
        && oplog.shallow_since_vv().is_empty()
        && forward_replay_gate(ops_num, pre_root_ops, 0))
    .then(|| {
        let spans: Vec<IdSpan> = root_vv
            .iter()
            .filter(|(_, counter)| **counter > 0)
            .map(|(peer, counter)| IdSpan::new(*peer, 0, *counter))
            .collect();
        if estimate_ops_content_bytes(&oplog, &spans, MAX_PRE_ROOT_BYTES_FOR_FORWARD_REPLAY)
            > MAX_PRE_ROOT_BYTES_FOR_FORWARD_REPLAY
        {
            return None;
        }

        Some(export_fast_updates_in_range(&oplog, &spans))
    })
    .flatten();
    if &start_from == oplog.shallow_since_frontiers() && state_frontiers == latest_frontiers {
        let mut state = doc.app_state().lock();
        if let Some((shallow_root_state_bytes, shallow_root_kv)) =
            state.store.shallow_root_state_for_export()
        {
            // Ops since the root are few enough to replay on import; otherwise
            // also ship the encoded latest state as an overlay.
            let overlay_kv = if ops_num > MAX_OPS_NUM_TO_ENCODE_WITHOUT_LATEST_STATE {
                let mut alive_c_bytes = shallow_root_kv.keys();
                if has_unknown_container_key(alive_c_bytes.iter()) {
                    return Err(LoroEncodeError::UnknownContainer);
                }

                state.ensure_all_alive_containers()?;
                state.store.flush();

                // All the containers that are created after start_from need to be encoded.
                for cid in state.store.iter_all_container_ids() {
                    if let ContainerID::Normal { peer, counter, .. } = cid {
                        let temp_id = ID::new(peer, counter);
                        if !start_from.contains(&temp_id) {
                            alive_c_bytes.insert(cid.to_bytes());
                        }
                    } else {
                        alive_c_bytes.insert(cid.to_bytes());
                    }
                }

                let new_kv = state.store.get_kv_clone();
                new_kv.remove_same(&shallow_root_kv);
                new_kv.retain_keys(&alive_c_bytes);
                Some(new_kv)
            } else {
                None
            };

            // The stored shallow-root bytes may predate dead-style redaction
            // (e.g. imported from an older export), so re-run it before reuse.
            let shallow_root_state_bytes =
                if redact_export_states(&shallow_root_kv, overlay_kv.as_ref())? {
                    // The cloned root kv has no FRONTIERS_KEY (InnerStore::decode
                    // strips it on import); restore it before export.
                    shallow_root_kv.insert(FRONTIERS_KEY, start_from.encode().into());
                    shallow_root_kv.export()
                } else {
                    shallow_root_state_bytes
                };

            return Ok((
                Snapshot {
                    oplog_bytes,
                    state_bytes: overlay_kv.map(|kv| kv.export()),
                    shallow_root_state_bytes,
                },
                start_from,
            ));
        }
    }
    drop(oplog);
    let result = (|| -> Result<Snapshot, LoroEncodeError> {
        if let Some(pre_root_updates) = pre_root_updates {
            // The live state is already at the latest version: build the root
            // state by replaying pre-root history forward into a temporary doc
            // instead of checking the live doc out backwards. A reverse
            // checkout (latest -> root) makes the diff calculators rebuild a
            // full CRDT tracker from empty for every list-like container
            // touched in the range (the `should_rebuild` path in
            // `RichtextDiffCalculator::calculate_diff`), which costs orders of
            // magnitude more than forward replay when the doc has many small
            // containers. Forward replay also leaves the live doc untouched,
            // so no state restore is needed.
            let root_doc = LoroDoc::new();
            root_doc
                .import(&pre_root_updates)
                .map_err(LoroEncodeError::from)?;
            let mut root_state = root_doc.app_state().lock();
            // The replay doc does not share the live doc's deleted-root set;
            // without it a root container deleted before the root would be
            // re-encoded as an empty entry instead of being dropped at flush.
            // (`InnerStore::flush` only drops the entry when the value is
            // still empty, so roots deleted *after* the root keep their
            // at-root content even with the set mirrored.)
            *root_state.config.deleted_root_containers.lock() =
                doc.config().deleted_root_containers.lock().clone();
            // Root containers exist on the live doc once they are accessed,
            // even when they have no ops; the replay doc cannot know about
            // those. Mirror the live store's root entries so the exported root
            // state ships the same empty root containers the checkout path
            // would. `existing_retention_roots` is a root-only key scan — a
            // full `load_all` here would defeat lazily imported docs.
            {
                let mut live_state = doc.app_state().lock();
                for idx in live_state.existing_retention_roots() {
                    let cid = live_state.arena.get_container_id(idx).unwrap();
                    root_state.store.ensure_container(&cid);
                }
            }
            let alive_containers = root_state.ensure_all_alive_containers()?;
            if has_unknown_container(alive_containers.iter().copied()) {
                return Err(LoroEncodeError::UnknownContainer);
            }
            let mut alive_c_bytes = alive_indices_to_bytes(&root_state, &alive_containers);
            root_state.store.flush();
            let shallow_root_state_kv = root_state.store.get_kv_clone();
            drop(root_state);

            let latest_state_kv = {
                let mut state = doc.app_state().lock();
                latest_state_overlay_kv(
                    &mut state,
                    ops_num,
                    &start_from,
                    &shallow_root_state_kv,
                    &mut alive_c_bytes,
                )?
            };
            return encode_shallow_sections(
                oplog_bytes,
                &start_from,
                shallow_root_state_kv,
                latest_state_kv,
                alive_c_bytes,
            );
        }

        doc._checkout_without_emitting(&start_from, false, false)
            .map_err(LoroEncodeError::from)?;
        let mut state = doc.app_state().lock();
        let alive_containers = state.ensure_all_alive_containers()?;
        if has_unknown_container(alive_containers.iter().copied()) {
            return Err(LoroEncodeError::UnknownContainer);
        }
        let mut alive_c_bytes = alive_indices_to_bytes(&state, &alive_containers);
        state.store.flush();
        let shallow_root_state_kv = state.store.get_kv_clone();
        drop(state);
        doc._checkout_without_emitting(&latest_frontiers, false, false)
            .map_err(LoroEncodeError::from)?;
        let latest_state_kv = {
            let mut state = doc.app_state().lock();
            latest_state_overlay_kv(
                &mut state,
                ops_num,
                &start_from,
                &shallow_root_state_kv,
                &mut alive_c_bytes,
            )?
        };
        encode_shallow_sections(
            oplog_bytes,
            &start_from,
            shallow_root_state_kv,
            latest_state_kv,
            alive_c_bytes,
        )
    })();

    restore_export_doc_state(doc, &state_frontiers, is_attached)?;
    doc.drop_pending_events();
    Ok((result?, start_from))
}

/// Compute the encoded latest-state overlay shipped alongside the shallow root
/// when the retained history is too large to replay on import. Containers
/// created after `start_from` are added to `alive_c_bytes` so both the root
/// state and the overlay keep them.
fn latest_state_overlay_kv(
    state: &mut DocState,
    ops_num: usize,
    start_from: &Frontiers,
    shallow_root_state_kv: &KvWrapper,
    alive_c_bytes: &mut BTreeSet<Vec<u8>>,
) -> Result<Option<KvWrapper>, LoroEncodeError> {
    if ops_num <= MAX_OPS_NUM_TO_ENCODE_WITHOUT_LATEST_STATE {
        return Ok(None);
    }

    state.ensure_all_alive_containers()?;
    state.store.encode();
    // All the containers that are created after start_from need to be encoded
    for cid in state.store.iter_all_container_ids() {
        if let ContainerID::Normal { peer, counter, .. } = cid {
            let temp_id = ID::new(peer, counter);
            if !start_from.contains(&temp_id) {
                alive_c_bytes.insert(cid.to_bytes());
            }
        } else {
            alive_c_bytes.insert(cid.to_bytes());
        }
    }

    let new_kv = state.store.get_kv_clone();
    new_kv.remove_same(shallow_root_state_kv);
    new_kv.retain_keys(alive_c_bytes);
    Ok(Some(new_kv))
}

fn encode_shallow_sections(
    oplog_bytes: Bytes,
    start_from: &Frontiers,
    shallow_root_state_kv: KvWrapper,
    latest_state_kv: Option<KvWrapper>,
    alive_c_bytes: BTreeSet<Vec<u8>>,
) -> Result<Snapshot, LoroEncodeError> {
    shallow_root_state_kv.retain_keys(&alive_c_bytes);
    redact_export_states(&shallow_root_state_kv, latest_state_kv.as_ref())?;
    let state_bytes = latest_state_kv.map(|kv| kv.export());
    shallow_root_state_kv.insert(FRONTIERS_KEY, start_from.encode().into());
    let shallow_root_state_bytes = shallow_root_state_kv.export();

    Ok(Snapshot {
        oplog_bytes,
        state_bytes,
        shallow_root_state_bytes,
    })
}

fn has_unknown_container(mut idxs: impl Iterator<Item = ContainerIdx>) -> bool {
    idxs.any(|idx| matches!(idx.get_type(), ContainerType::Unknown(_)))
}

fn has_unknown_container_key<'a>(mut keys: impl Iterator<Item = &'a Vec<u8>>) -> bool {
    keys.any(|key| ContainerID::from_bytes(key).is_unknown())
}

/// Nulls the values of rich-text style pairs that are dead in this state (no
/// text between the anchors). In a document whose history is trimmed at this
/// state such a pair can never style anything again, but its value would
/// otherwise ship in the export even though no read API can reach it. See
/// `context/shallow-snapshot-style-redaction.md`.
///
/// Containers are keyed by `ContainerID` bytes whose first byte is the
/// container type, so the scan stays within the two Text key ranges and blocks
/// holding other containers are passed through in their compressed form.
///
/// Returns the ids of the redacted `StyleStart` ops, or `None` when the KV was
/// left untouched.
/// Applies dead-style redaction to the states being exported.
///
/// The root state is fully redacted; the optional overlay (the encoded
/// latest/target state shipped alongside it) only redacts the pairs the root
/// redacted. Pairs that die *after* the root must keep their values so
/// checkouts into the retained range still render them — this function is the
/// single place that protocol lives.
///
/// Returns whether the root KV changed. Call before the KVs are exported and
/// after `remove_same`, so byte-identical entries still dedup (a deduped entry
/// resolves to the redacted root version on import).
fn redact_export_states(
    root: &KvWrapper,
    overlay: Option<&KvWrapper>,
) -> Result<bool, LoroEncodeError> {
    match redact_dead_text_styles(root, None)? {
        None => Ok(false),
        Some(redacted) => {
            if let Some(overlay) = overlay {
                redact_dead_text_styles(overlay, Some(&redacted))?;
            }
            Ok(true)
        }
    }
}

fn redact_dead_text_styles(
    kv: &KvWrapper,
    only_pairs: Option<&FxHashSet<ID>>,
) -> Result<Option<FxHashSet<ID>>, LoroEncodeError> {
    const ROOT_MARK: u8 = 0b1000_0000;
    let text_kind = ContainerType::Text.to_u8();
    debug_assert_eq!(text_kind & ROOT_MARK, 0);
    let mut redacted: FxHashSet<ID> = FxHashSet::default();
    let mut changed = false;
    for first_key_byte in [text_kind, text_kind | ROOT_MARK] {
        for (key, value) in kv.scan_range_entries(&[first_key_byte], &[first_key_byte + 1]) {
            if value.is_empty() {
                continue;
            }
            let offset = ContainerWrapper::payload_offset(&value).map_err(LoroEncodeError::from)?;
            if let Some((payload, ids)) = redact_dead_style_values(&value[offset..], only_pairs)
                .map_err(LoroEncodeError::from)?
            {
                let mut new_value = Vec::with_capacity(offset + payload.len());
                new_value.extend_from_slice(&value[..offset]);
                new_value.extend_from_slice(&payload);
                kv.insert(&key, new_value.into());
                redacted.extend(ids);
                changed = true;
            }
        }
    }
    Ok(changed.then_some(redacted))
}

pub(crate) fn export_state_only_snapshot<W: std::io::Write>(
    doc: &LoroDoc,
    target_frontiers: &Frontiers,
    w: &mut W,
) -> Result<Frontiers, LoroEncodeError> {
    let oplog = doc.oplog().lock();
    let start_from = calc_shallow_doc_start(&oplog, target_frontiers);
    let mut start_vv =
        frontiers_to_vv_for_export(&oplog, &start_from, "export_state_only_snapshot")?;
    for id in start_from.iter() {
        // we need to include the ops in start_from, this can make things easier
        start_vv.insert(id.peer, id.counter);
    }

    loro_common::debug!(
        "start version vv={:?} frontiers={:?}",
        &start_vv,
        &start_from,
    );

    let to_vv = frontiers_to_vv_for_export(&oplog, target_frontiers, "export_state_only_snapshot")?;
    let oplog_bytes =
        oplog.export_change_store_in_range(&start_vv, &start_from, &to_vv, target_frontiers);
    let state_frontiers = doc.state_frontiers();
    let is_attached = !doc.is_detached();
    drop(oplog);
    let result = (|| -> Result<(), LoroEncodeError> {
        doc._checkout_without_emitting(&start_from, false, false)
            .map_err(LoroEncodeError::from)?;
        let mut state = doc.app_state().lock();
        let alive_containers = state.ensure_all_alive_containers()?;
        if has_unknown_container(alive_containers.iter().copied()) {
            return Err(LoroEncodeError::UnknownContainer);
        }
        let mut alive_c_bytes = alive_indices_to_bytes(&state, &alive_containers);
        state.store.flush();
        let shallow_state_kv = state.store.get_kv_clone();
        drop(state);

        doc._checkout_without_emitting(target_frontiers, false, false)
            .map_err(LoroEncodeError::from)?;
        let mut state = doc.app_state().lock();
        state.ensure_all_alive_containers()?;
        state.store.encode();
        for cid in state.store.iter_all_container_ids() {
            if let ContainerID::Normal { peer, counter, .. } = cid {
                let temp_id = ID::new(peer, counter);
                if !start_from.contains(&temp_id) {
                    alive_c_bytes.insert(cid.to_bytes());
                }
            } else {
                alive_c_bytes.insert(cid.to_bytes());
            }
        }

        let target_state_kv = state.store.get_kv_clone();
        drop(state);
        target_state_kv.remove_same(&shallow_state_kv);
        target_state_kv.retain_keys(&alive_c_bytes);

        shallow_state_kv.retain_keys(&alive_c_bytes);
        redact_export_states(&shallow_state_kv, Some(&target_state_kv))?;
        shallow_state_kv.insert(FRONTIERS_KEY, start_from.encode().into());
        let shallow_state_bytes = shallow_state_kv.export();
        let snapshot = Snapshot {
            oplog_bytes,
            state_bytes: Some(target_state_kv.export()),
            shallow_root_state_bytes: shallow_state_bytes,
        };
        _encode_snapshot(&snapshot, w);
        Ok(())
    })();

    restore_export_doc_state(doc, &state_frontiers, is_attached)?;
    doc.drop_pending_events();
    result?;
    Ok(start_from)
}

fn alive_indices_to_bytes(
    state: &DocState,
    alive_containers: &rustc_hash::FxHashSet<ContainerIdx>,
) -> BTreeSet<Vec<u8>> {
    alive_containers
        .iter()
        .map(|idx| state.arena.get_container_id(*idx).unwrap().to_bytes())
        .collect()
}

fn frontiers_to_vv_for_export(
    oplog: &crate::OpLog,
    frontiers: &Frontiers,
    context: &str,
) -> Result<VersionVector, LoroEncodeError> {
    oplog.dag().frontiers_to_vv(frontiers).ok_or_else(|| {
        LoroEncodeError::FrontiersNotFound(format!(
            "{context}: unreachable frontiers {frontiers:?}"
        ))
    })
}

fn restore_export_doc_state(
    doc: &LoroDoc,
    state_frontiers: &Frontiers,
    was_attached: bool,
) -> Result<(), LoroEncodeError> {
    if &doc.state_frontiers() != state_frontiers {
        doc._checkout_without_emitting(state_frontiers, false, false)
            .map_err(LoroEncodeError::from)?;
    }

    if was_attached {
        doc.set_detached(false);
    }

    Ok(())
}

/// Calculates optimal starting version for the shallow doc
///
/// It should be a common ancestor version of the user-given version and the latest version.
/// Otherwise, users cannot replay the history from the initial version till the latest version.
fn calc_shallow_doc_start(oplog: &crate::OpLog, frontiers: &Frontiers) -> Frontiers {
    // Find a common ancestor version of the given frontiers by iterative pairwise reduction.
    // This converges to a single frontier or empty if there is no common ancestor.
    let mut current = frontiers.clone();
    while current.len() > 1 {
        let ids: Vec<ID> = current.iter().collect();
        let mut next = Frontiers::new();
        let mut i = 0;
        while i < ids.len() {
            if i + 1 < ids.len() {
                let (gca, _) = oplog
                    .dag()
                    .find_common_ancestor(&Frontiers::from(ids[i]), &Frontiers::from(ids[i + 1]));
                for id in gca.iter() {
                    next.push(id);
                }
            } else {
                next.push(ids[i]);
            }
            i += 2;
        }
        if next == current {
            // Cannot converge further (pairwise GCAs are the nodes themselves).
            // Fall back to empty frontiers, meaning export full history.
            return clamp_to_shallow_root(oplog, Frontiers::default());
        }
        current = next;
    }

    let mut ans = Frontiers::new();
    for id in current.iter() {
        let mut processed = false;
        if let Some(op) = oplog.get_op_that_includes(id) {
            if let crate::op::InnerContent::List(InnerListOp::StyleStart { .. }) = &op.content {
                // StyleStart and StyleEnd operations must be kept together in the GC snapshot.
                // Splitting them could lead to an weird document state that cannot be
                // properly encoded. To ensure they stay together, we advance the frontier by
                // one step to include both operations.

                // > Id.counter + 1 is guaranteed to be the StyleEnd Op
                ans.push(id.inc(1));
                processed = true;
            }
        }

        if !processed {
            ans.push(id);
        }
    }

    clamp_to_shallow_root(oplog, ans)
}

fn clamp_to_shallow_root(oplog: &crate::OpLog, frontiers: Frontiers) -> Frontiers {
    if oplog.shallow_since_vv().is_empty() {
        return frontiers;
    }

    let Some(vv) = oplog.dag().frontiers_to_vv(&frontiers) else {
        return oplog.shallow_since_frontiers().clone();
    };

    if vv.includes_vv(&oplog.shallow_since_vv().to_vv()) {
        frontiers
    } else {
        oplog.shallow_since_frontiers().clone()
    }
}

pub(crate) fn encode_snapshot_at<W: std::io::Write>(
    doc: &LoroDoc,
    frontiers: &Frontiers,
    w: &mut W,
) -> Result<(), LoroEncodeError> {
    let was_detached = doc.is_detached();
    let version_before_start = doc.state_frontiers().clone();
    doc._checkout_without_emitting(frontiers, true, false)
        .map_err(LoroEncodeError::from)?;
    let result = 'block: {
        let oplog = doc.oplog().lock();
        let mut state = doc.app_state().lock();
        let is_shallow = state.store.shallow_root_store().is_some();
        if is_shallow {
            break 'block Err(LoroEncodeError::from(LoroError::NotImplemented(
                "fork_at on shallow docs",
            )));
        }

        if state.is_in_txn() {
            break 'block Err(LoroEncodeError::internal(
                "encode_snapshot_at: state is unexpectedly still in a transaction",
            ));
        }
        let Some(oplog_bytes) = oplog.fork_changes_up_to(frontiers) else {
            break 'block Err(LoroEncodeError::FrontiersNotFound(format!(
                "frontiers: {:?} when export in SnapshotAt mode",
                frontiers
            )));
        };

        if oplog.is_shallow() {
            let Some(shallow_root_frontiers) = state.store.shallow_root_frontiers() else {
                break 'block Err(LoroEncodeError::internal(
                    "encode_snapshot_at: shallow oplog is missing shallow root frontiers",
                ));
            };
            if oplog.shallow_since_frontiers() != shallow_root_frontiers {
                break 'block Err(LoroEncodeError::internal(
                    "encode_snapshot_at: shallow root frontiers are inconsistent",
                ));
            }
        }

        let alive_containers = state.ensure_all_alive_containers()?;
        if has_unknown_container(alive_containers.iter().copied()) {
            break 'block Err(LoroEncodeError::UnknownContainer);
        }

        let alive_c_bytes = alive_indices_to_bytes(&state, &alive_containers);
        state.store.flush();
        let state_kv = state.store.get_kv_clone();
        state_kv.retain_keys(&alive_c_bytes);
        let bytes = state_kv.export();
        _encode_snapshot(
            &Snapshot {
                oplog_bytes,
                state_bytes: Some(bytes),
                shallow_root_state_bytes: Bytes::new(),
            },
            w,
        );

        Ok(())
    };
    let restore_result = doc
        ._checkout_without_emitting(&version_before_start, false, false)
        .map_err(LoroEncodeError::from);
    if !was_detached {
        doc.set_detached(false);
    }
    doc.app_state().lock().take_events();

    match result {
        Err(err) => Err(err),
        Ok(()) => restore_result,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::configure::Configure;
    use crate::container::idx::ContainerIdx;
    use crate::container::richtext::richtext_state::{PosType, RichtextStateChunk};
    use crate::container::richtext::AnchorType;
    use crate::encoding::fast_snapshot::_decode_snapshot_bytes;
    use crate::encoding::EncodeMode;
    use crate::encoding::ExportMode;
    use crate::handler::MapHandler;
    use crate::handler::TextHandler;
    use crate::state::{ContainerCreationContext, FastStateSnapshot, RichtextState};
    use crate::HandlerTrait;
    use crate::LoroDoc;
    use loro_common::LoroValue;

    fn shallow_sections(blob: &[u8]) -> Snapshot {
        let parsed = crate::encoding::parse_header_and_body(blob, true).unwrap();
        _decode_snapshot_bytes(Bytes::copy_from_slice(parsed.body)).unwrap()
    }

    /// Reassemble decomposed sections into a complete importable blob
    /// (magic + checksum + mode header, matching `encode_with`).
    fn assemble_snapshot_blob(sections: &Snapshot) -> Vec<u8> {
        let mut blob = Vec::new();
        blob.extend(crate::encoding::MAGIC_BYTES);
        blob.extend([0u8; 16]);
        blob.extend(EncodeMode::FastSnapshot.to_bytes());
        _encode_snapshot(sections, &mut blob);
        let checksum = xxhash_rust::xxh32::xxh32(&blob[20..], crate::encoding::XXH_SEED);
        blob[16..20].copy_from_slice(&checksum.to_le_bytes());
        blob
    }

    /// Decode the style values of a text container straight out of exported
    /// state KV bytes, so assertions don't depend on whether the secret is
    /// visible through LZ4 compression.
    fn text_style_values(kv_bytes: &Bytes, cid: &ContainerID) -> Vec<(ID, LoroValue)> {
        let kv = KvWrapper::new_mem();
        kv.import(kv_bytes.clone()).unwrap();
        let value = kv
            .get(&cid.to_bytes())
            .expect("text container should be present in the exported state");
        let offset = ContainerWrapper::payload_offset(&value).unwrap();
        let (text, rest) = RichtextState::decode_value(&value[offset..]).unwrap();
        let idx = ContainerIdx::from_index_and_type(0, ContainerType::Text);
        let configure = Configure::default();
        let ctx = ContainerCreationContext {
            configure: &configure,
            peer: 0,
        };
        let state = RichtextState::decode_snapshot_fast(idx, (text, rest), ctx).unwrap();
        let mut styles = Vec::new();
        state.iter_raw(&mut |chunk| {
            if let RichtextStateChunk::Style {
                style,
                anchor_type: AnchorType::Start,
            } = chunk
            {
                styles.push((ID::new(style.peer, style.cnt), style.value.clone()));
            }
        });
        styles
    }

    fn root_text_cid() -> ContainerID {
        ContainerID::new_root("text", ContainerType::Text)
    }

    #[test]
    fn shallow_export_nulls_dead_style_values_for_root_and_nested_text() {
        let doc = LoroDoc::new_auto_commit();
        doc.set_peer_id(1).unwrap();
        let root_text = doc.get_text("text");
        root_text.insert(0, "abc", PosType::Unicode).unwrap();
        root_text
            .mark(0, 3, "comment", "root-secret".into(), PosType::Unicode)
            .unwrap();
        let nested_text = doc
            .get_map("meta")
            .insert_container("note", TextHandler::new_detached())
            .unwrap();
        nested_text.insert(0, "xyz", PosType::Unicode).unwrap();
        nested_text
            .mark(0, 3, "comment", "nested-secret".into(), PosType::Unicode)
            .unwrap();
        doc.commit_then_renew();
        root_text.delete(0, 3, PosType::Unicode).unwrap();
        nested_text.delete(0, 3, PosType::Unicode).unwrap();
        doc.commit_then_renew();

        let blob = doc
            .export(ExportMode::shallow_snapshot(&doc.oplog_frontiers()))
            .unwrap();
        let sections = shallow_sections(&blob);

        for cid in [root_text_cid(), nested_text.id()] {
            let styles = text_style_values(&sections.shallow_root_state_bytes, &cid);
            assert_eq!(styles.len(), 1, "container {cid}");
            assert_eq!(styles[0].1, LoroValue::Null, "container {cid}");
        }
    }

    #[test]
    fn overlay_keeps_style_values_that_die_after_the_root() {
        let doc = LoroDoc::new_auto_commit();
        doc.set_peer_id(1).unwrap();
        let text = doc.get_text("text");
        text.insert(0, "hello", PosType::Unicode).unwrap();
        text.mark(0, 5, "comment", "keep-me".into(), PosType::Unicode)
            .unwrap();
        doc.commit_then_renew();
        let start = doc.oplog_frontiers();

        // The style dies after the shallow root...
        text.delete(0, 5, PosType::Unicode).unwrap();
        // ...and enough tail ops force the export to carry the latest state.
        let filler = doc.get_map("filler");
        for i in 0..(MAX_OPS_NUM_TO_ENCODE_WITHOUT_LATEST_STATE + 1) {
            filler.insert(&i.to_string(), i as i64).unwrap();
        }
        doc.commit_then_renew();

        let blob = doc.export(ExportMode::shallow_snapshot(&start)).unwrap();
        let sections = shallow_sections(&blob);
        let state_bytes = sections
            .state_bytes
            .expect("overlay state should be encoded");

        let cid = root_text_cid();
        let keep = LoroValue::String("keep-me".into());
        assert_eq!(
            text_style_values(&sections.shallow_root_state_bytes, &cid)[0].1,
            keep,
            "style alive at the root must keep its value"
        );
        // The pair is dead in the latest state but was alive at the root, so a
        // checkout back into the retained range must still render it.
        assert_eq!(text_style_values(&state_bytes, &cid)[0].1, keep);
    }

    #[test]
    fn overlay_redacts_pairs_dead_at_the_root() {
        let doc = LoroDoc::new_auto_commit();
        doc.set_peer_id(1).unwrap();
        let text = doc.get_text("text");
        text.insert(0, "abc", PosType::Unicode).unwrap();
        text.mark(0, 3, "comment", "dead-secret".into(), PosType::Unicode)
            .unwrap();
        doc.commit_then_renew();
        text.delete(0, 3, PosType::Unicode).unwrap();
        doc.commit_then_renew();
        let start = doc.oplog_frontiers();

        // Make the latest text entry differ from the root entry so it survives
        // `remove_same` and must be redacted via the root's pair whitelist.
        text.insert(0, "later", PosType::Unicode).unwrap();
        let filler = doc.get_map("filler");
        for i in 0..(MAX_OPS_NUM_TO_ENCODE_WITHOUT_LATEST_STATE + 1) {
            filler.insert(&i.to_string(), i as i64).unwrap();
        }
        doc.commit_then_renew();

        let blob = doc.export(ExportMode::shallow_snapshot(&start)).unwrap();
        let sections = shallow_sections(&blob);
        let cid = root_text_cid();
        assert_eq!(
            text_style_values(&sections.shallow_root_state_bytes, &cid)[0].1,
            LoroValue::Null
        );
        let state_bytes = sections
            .state_bytes
            .expect("overlay state should be encoded");
        assert_eq!(text_style_values(&state_bytes, &cid)[0].1, LoroValue::Null);
    }

    #[test]
    fn legacy_unredacted_shallow_blob_is_cleaned_on_reexport() {
        let doc = LoroDoc::new_auto_commit();
        doc.set_peer_id(1).unwrap();
        let text = doc.get_text("text");
        text.insert(0, "abc", PosType::Unicode).unwrap();
        text.mark(0, 3, "comment", "legacy-secret".into(), PosType::Unicode)
            .unwrap();
        doc.commit_then_renew();
        text.delete(0, 3, PosType::Unicode).unwrap();
        doc.commit_then_renew();

        let blob = doc
            .export(ExportMode::shallow_snapshot(&doc.oplog_frontiers()))
            .unwrap();
        let mut sections = shallow_sections(&blob);

        // Rebuild the root state the way pre-redaction exports encoded it: the
        // doc's own state at the tip still holds the secret value.
        let unredacted_root = {
            let mut state = doc.app_state().lock();
            state.store.flush();
            let kv = state.store.get_kv_clone();
            drop(state);
            kv.insert(FRONTIERS_KEY, doc.oplog_frontiers().encode().into());
            kv.export()
        };
        let cid = root_text_cid();
        assert_eq!(
            text_style_values(&unredacted_root, &cid)[0].1,
            LoroValue::String("legacy-secret".into()),
            "test setup must produce an unredacted legacy root"
        );
        sections.shallow_root_state_bytes = unredacted_root;

        // Import the legacy-shaped blob; re-exporting must clean the stored
        // root bytes on the reuse branch.
        let legacy_doc = LoroDoc::new();
        legacy_doc
            .import(&assemble_snapshot_blob(&sections))
            .unwrap();
        let reexported = legacy_doc
            .export(ExportMode::shallow_snapshot(&legacy_doc.oplog_frontiers()))
            .unwrap();
        let resections = shallow_sections(&reexported);
        assert_eq!(
            text_style_values(&resections.shallow_root_state_bytes, &cid)[0].1,
            LoroValue::Null
        );
    }

    /// Regression test for the P1 found in review of the bounded container
    /// value cache (loro-dev/loro#1092): after a shallow-snapshot import the
    /// store is `AllLoaded`, and a container walk evicts most decoded values.
    /// Re-exporting the same shallow root must still enumerate every container
    /// created after the root — otherwise the latest-state overlay silently
    /// drops the evicted ones and the next import loses them.
    #[test]
    fn reexport_same_shallow_root_after_walk_eviction_keeps_overlay_containers() {
        // Doc A: base content behind the shallow root.
        let a = LoroDoc::new_auto_commit();
        a.set_peer_id(1).unwrap();
        a.get_text("text")
            .insert(0, "base", PosType::Unicode)
            .unwrap();
        a.commit_then_renew();
        let start = a.oplog_frontiers();

        // Doc B: import the shallow snapshot, then create containers that live
        // only in the latest-state overlay (they are not in the root KV).
        let b = LoroDoc::new_auto_commit();
        b.set_peer_id(2).unwrap();
        b.import(&a.export(ExportMode::shallow_snapshot(&start)).unwrap())
            .unwrap();
        let n = 64;
        let list = b.get_list("list");
        for i in 0..n {
            let map = list
                .insert_container(i, MapHandler::new_detached())
                .unwrap();
            map.insert("key", i as i64).unwrap();
        }
        b.commit_then_renew();
        // 2 ops per container > MAX_OPS_NUM_TO_ENCODE_WITHOUT_LATEST_STATE
        // (16 in test builds), so this export ships the latest-state overlay.
        let blob = b.export(ExportMode::shallow_snapshot(&start)).unwrap();

        // Doc C imports the blob (store is AllLoaded with lazy wrappers) and
        // walks every overlay container, evicting most of them from the
        // bounded value cache.
        let c = LoroDoc::new_auto_commit();
        c.import(&blob).unwrap();
        let list = c.get_list("list");
        for i in 0..n {
            let child_id = list.get(i).unwrap().as_container().unwrap().clone();
            assert_eq!(c.get_map(child_id).get("key"), Some((i as i64).into()));
        }

        // Re-exporting the same shallow root reuses the stored root bytes and
        // rebuilds the overlay by enumerating all containers. With the broken
        // `load_all` short-circuit this dropped every evicted container.
        let reexported = c.export(ExportMode::shallow_snapshot(&start)).unwrap();
        assert!(
            shallow_sections(&reexported).state_bytes.is_some(),
            "test setup must take the overlay export path"
        );

        let d = LoroDoc::new_auto_commit();
        d.import(&reexported).unwrap();
        let list = d.get_list("list");
        assert_eq!(list.len(), n);
        for i in 0..n {
            let child_id = list
                .get(i)
                .unwrap_or_else(|| panic!("container {i} lost after shallow re-export"))
                .as_container()
                .unwrap()
                .clone();
            assert_eq!(d.get_map(child_id).get("key"), Some((i as i64).into()));
        }
    }

    #[test]
    fn forward_replay_gate_bounds_prefix_by_ops_ratio_and_bytes() {
        let min = MIN_RETAINED_OPS_FOR_FORWARD_ROOT_STATE;
        let max_ops = MAX_PRE_ROOT_OPS_FOR_FORWARD_REPLAY;
        let max_bytes = MAX_PRE_ROOT_BYTES_FOR_FORWARD_REPLAY;

        // Tail below the minimum: never.
        assert!(!forward_replay_gate(min - 1, 0, 0));
        // Balanced prefix/tail: allowed.
        assert!(forward_replay_gate(min, min, 1));
        // Prefix larger than 16x the tail: blocked even though both op counts
        // are small.
        assert!(!forward_replay_gate(min, 17 * min, 1));
        // Prefix above the absolute op cap: blocked.
        assert!(!forward_replay_gate(max_ops, max_ops + 1, 1));
        // A byte-heavy but low-op prefix (a Map write is one atom regardless
        // of value size): blocked only by the byte leg.
        assert!(!forward_replay_gate(min, 1, max_bytes + 1));
        assert!(forward_replay_gate(min, 1, max_bytes));
    }

    /// A byte-heavy, low-op prefix (a huge payload nested inside a Map value,
    /// later overwritten) must not be replayed into a temporary doc just
    /// because the tail clears the retained-ops threshold. The export must
    /// still be correct.
    #[test]
    fn byte_heavy_low_op_prefix_exports_correctly() {
        let doc = LoroDoc::new_auto_commit();
        doc.set_peer_id(1).unwrap();
        let map = doc.get_map("m");
        let big = "v".repeat(MAX_PRE_ROOT_BYTES_FOR_FORWARD_REPLAY + 1024);
        // Nested one level down: the estimator must recurse into it.
        let mut payload = crate::FxHashMap::default();
        payload.insert("payload".to_string(), LoroValue::String(big.into()));
        map.insert("k", LoroValue::Map(payload.into())).unwrap();
        map.insert("k", "small").unwrap();
        doc.commit_then_renew();
        let f = doc.oplog_frontiers();
        // Tail just past the (test-scale) retained-ops threshold.
        let text = doc.get_text("t");
        text.insert(
            0,
            &"x".repeat(MIN_RETAINED_OPS_FOR_FORWARD_ROOT_STATE + 1),
            PosType::Unicode,
        )
        .unwrap();
        doc.commit_then_renew();

        let blob = doc.export(ExportMode::shallow_snapshot(&f)).unwrap();
        let imported = LoroDoc::new();
        imported.import(&blob).unwrap();
        assert_eq!(imported.get_deep_value(), doc.get_deep_value());
        assert_eq!(imported.shallow_since_frontiers(), f);
    }

    #[test]
    fn prefix_content_bytes_estimate_counts_value_bytes() {
        let doc = LoroDoc::new_auto_commit();
        doc.set_peer_id(1).unwrap();
        // One atom carrying 2 MiB of payload.
        let big = "v".repeat(2 << 20);
        doc.get_map("m").insert("k", big.as_str()).unwrap();
        doc.get_text("t")
            .insert(0, "abc", PosType::Unicode)
            .unwrap();
        doc.commit_then_renew();

        let oplog = doc.oplog().lock();
        let spans = vec![IdSpan::new(1, 0, 4)];
        let est = estimate_ops_content_bytes(&oplog, &spans, usize::MAX);
        assert!(
            est >= (2 << 20) + 3,
            "estimate must include the value bytes, got {est}"
        );
        // Early exit: a smaller cap stops the walk as soon as it is exceeded.
        let capped = estimate_ops_content_bytes(&oplog, &spans, 1024);
        assert!(capped > 1024 && capped <= 1024 + (2 << 20) + 64);
    }

    #[test]
    fn prefix_content_bytes_estimate_counts_nested_style_and_commit_msg_bytes() {
        let doc = LoroDoc::new_auto_commit();
        doc.set_peer_id(1).unwrap();
        let big = "v".repeat(1 << 20);

        // One atom whose payload is a nested map/list holding the big string.
        let mut inner = crate::FxHashMap::default();
        inner.insert("payload".to_string(), LoroValue::String(big.clone().into()));
        let nested = LoroValue::List(vec![LoroValue::Map(inner.into())].into());
        doc.get_map("m").insert("k", nested).unwrap();

        // A style mark value.
        let text = doc.get_text("t");
        text.insert(0, "ab", PosType::Unicode).unwrap();
        text.mark(
            0,
            1,
            "comment",
            LoroValue::String(big.clone().into()),
            PosType::Unicode,
        )
        .unwrap();

        // A commit message.
        doc.set_next_commit_message(&big);
        doc.commit_then_renew();

        let oplog = doc.oplog().lock();
        let end = *oplog.vv().get(&1).unwrap();
        let spans = vec![IdSpan::new(1, 0, end)];
        let est = estimate_ops_content_bytes(&oplog, &spans, usize::MAX);
        assert!(
            est >= 3 * big.len(),
            "estimate must count nested values, style values and the commit message, got {est}"
        );
    }

    #[test]
    fn prefix_content_bytes_estimate_counts_style_keys_and_root_names() {
        let doc = LoroDoc::new_auto_commit();
        doc.set_peer_id(1).unwrap();
        let big = "k".repeat(1 << 20);

        // A huge style key: the block encoder copies keys into the block's
        // key register.
        let mut styles = crate::container::richtext::config::StyleConfigMap::new();
        styles.insert(
            big.as_str().into(),
            crate::container::richtext::config::StyleConfig {
                expand: crate::container::richtext::ExpandType::After,
            },
        );
        doc.config_text_style(styles);
        let text = doc.get_text("t");
        text.insert(0, "ab", PosType::Unicode).unwrap();
        text.mark(0, 1, big.as_str(), LoroValue::Bool(true), PosType::Unicode)
            .unwrap();

        // A huge root container name: copied into the block's container arena.
        doc.get_text(big.as_str())
            .insert(0, "x", PosType::Unicode)
            .unwrap();
        doc.commit_then_renew();

        let oplog = doc.oplog().lock();
        let end = *oplog.vv().get(&1).unwrap();
        let spans = vec![IdSpan::new(1, 0, end)];
        let est = estimate_ops_content_bytes(&oplog, &spans, usize::MAX);
        assert!(
            est >= 2 * big.len(),
            "estimate must count style keys and root container names, got {est}"
        );
    }
}

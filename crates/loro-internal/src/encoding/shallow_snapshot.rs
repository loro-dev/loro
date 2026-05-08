use bytes::Bytes;
use rle::HasLength;
use std::collections::BTreeSet;

use loro_common::{ContainerID, ContainerType, LoroEncodeError, LoroError, ID};

use crate::{
    container::list::list_op::InnerListOp,
    dag::DagUtils,
    encoding::fast_snapshot::{_encode_snapshot, Snapshot},
    state::container_store::FRONTIERS_KEY,
    version::{shrink_frontiers, Frontiers, VersionVector},
    LoroDoc,
};

#[cfg(test)]
const MAX_OPS_NUM_TO_ENCODE_WITHOUT_LATEST_STATE: usize = 16;
#[cfg(not(test))]
const MAX_OPS_NUM_TO_ENCODE_WITHOUT_LATEST_STATE: usize = 256;

#[tracing::instrument(skip_all)]
pub(crate) fn export_shallow_snapshot<W: std::io::Write>(
    doc: &LoroDoc,
    start_from: &Frontiers,
    w: &mut W,
) -> Result<Frontiers, LoroEncodeError> {
    let (snapshot, start_from) = export_shallow_snapshot_inner(doc, start_from)?;
    _encode_snapshot(snapshot, w);
    Ok(start_from)
}

pub(crate) fn export_shallow_snapshot_inner(
    doc: &LoroDoc,
    start_from: &Frontiers,
) -> Result<(Snapshot, Frontiers), LoroEncodeError> {
    let oplog = doc.oplog().lock();
    let start_from = calc_shallow_doc_start(&oplog, start_from);
    let mut start_vv = frontiers_to_vv_for_export(&oplog, &start_from, "export_shallow_snapshot")?;
    for id in start_from.iter() {
        // we need to include the ops in start_from, this can make things easier
        start_vv.insert(id.peer, id.counter);
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
    if &start_from == oplog.shallow_since_frontiers()
        && state_frontiers == latest_frontiers
        && ops_num <= MAX_OPS_NUM_TO_ENCODE_WITHOUT_LATEST_STATE
    {
        let state = doc.app_state().lock();
        if let Some(shallow_root_state_bytes) = state.store.encode_shallow_root_state() {
            return Ok((
                Snapshot {
                    oplog_bytes,
                    state_bytes: None,
                    shallow_root_state_bytes,
                },
                start_from,
            ));
        }
    }
    drop(oplog);
    let result = (|| -> Result<Snapshot, LoroEncodeError> {
        doc._checkout_without_emitting(&start_from, false, false)
            .map_err(LoroEncodeError::from)?;
        let mut state = doc.app_state().lock();
        let alive_containers = state.ensure_all_alive_containers();
        if has_unknown_container(alive_containers.iter()) {
            return Err(LoroEncodeError::UnknownContainer);
        }
        let mut alive_c_bytes: BTreeSet<Vec<u8>> =
            alive_containers.iter().map(|x| x.to_bytes()).collect();
        state.store.flush();
        let shallow_root_state_kv = state.store.get_kv_clone();
        drop(state);
        doc._checkout_without_emitting(&latest_frontiers, false, false)
            .map_err(LoroEncodeError::from)?;
        let state_bytes = if ops_num > MAX_OPS_NUM_TO_ENCODE_WITHOUT_LATEST_STATE {
            let mut state = doc.app_state().lock();
            state.ensure_all_alive_containers();
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
            new_kv.remove_same(&shallow_root_state_kv);
            new_kv.retain_keys(&alive_c_bytes);
            Some(new_kv.export())
        } else {
            None
        };

        shallow_root_state_kv.retain_keys(&alive_c_bytes);
        shallow_root_state_kv.insert(FRONTIERS_KEY, start_from.encode().into());
        let shallow_root_state_bytes = shallow_root_state_kv.export();

        Ok(Snapshot {
            oplog_bytes,
            state_bytes,
            shallow_root_state_bytes,
        })
    })();

    restore_export_doc_state(doc, &state_frontiers, is_attached)?;
    doc.drop_pending_events();
    Ok((result?, start_from))
}

fn has_unknown_container<'a>(mut cids: impl Iterator<Item = &'a ContainerID>) -> bool {
    cids.any(|cid| matches!(cid.container_type(), ContainerType::Unknown(_)))
}

pub(crate) fn export_state_only_snapshot<W: std::io::Write>(
    doc: &LoroDoc,
    target_frontiers: &Frontiers,
    w: &mut W,
) -> Result<Frontiers, LoroEncodeError> {
    let oplog = doc.oplog().lock();
    let target_frontiers = normalize_state_only_target_frontiers(&oplog, target_frontiers);
    let start_from = calc_state_only_doc_start(&oplog, &target_frontiers);
    let start_inclusive_vv =
        frontiers_to_vv_for_export(&oplog, &start_from, "export_state_only_snapshot")?;
    let mut start_vv = start_inclusive_vv.clone();
    for id in start_from.iter() {
        // we need to include the ops in start_from, this can make things easier
        start_vv.insert(id.peer, id.counter);
    }

    loro_common::debug!(
        "start version vv={:?} frontiers={:?}",
        &start_vv,
        &start_from,
    );

    let mut to_vv =
        frontiers_to_vv_for_export(&oplog, &target_frontiers, "export_state_only_snapshot")?;
    to_vv.merge(&start_inclusive_vv);
    let to_frontiers = oplog.dag().vv_to_frontiers(&to_vv);

    let oplog_bytes =
        oplog.export_change_store_in_range(&start_vv, &start_from, &to_vv, &to_frontiers);
    let state_frontiers = doc.state_frontiers();
    let is_attached = !doc.is_detached();
    drop(oplog);
    let result = (|| -> Result<(), LoroEncodeError> {
        doc._checkout_without_emitting(&start_from, false, false)
            .map_err(LoroEncodeError::from)?;
        let mut state = doc.app_state().lock();
        let alive_containers = state.ensure_all_alive_containers();
        let alive_c_bytes = cids_to_bytes(alive_containers);
        state.store.flush();
        let shallow_state_kv = state.store.get_kv_clone();
        drop(state);
        shallow_state_kv.retain_keys(&alive_c_bytes);
        shallow_state_kv.insert(FRONTIERS_KEY, start_from.encode().into());
        let shallow_state_bytes = shallow_state_kv.export();
        let snapshot = Snapshot {
            oplog_bytes,
            state_bytes: None,
            shallow_root_state_bytes: shallow_state_bytes,
        };
        _encode_snapshot(snapshot, w);
        Ok(())
    })();

    restore_export_doc_state(doc, &state_frontiers, is_attached)?;
    doc.drop_pending_events();
    result?;
    Ok(start_from)
}

fn cids_to_bytes(
    alive_containers: std::collections::HashSet<ContainerID, rustc_hash::FxBuildHasher>,
) -> BTreeSet<Vec<u8>> {
    let alive_c_bytes: BTreeSet<Vec<u8>> = alive_containers.iter().map(|x| x.to_bytes()).collect();
    alive_c_bytes
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
/// It should be the LCA of the user given version and the latest version.
/// Otherwise, users cannot replay the history from the initial version till the latest version.
fn calc_shallow_doc_start(oplog: &crate::OpLog, frontiers: &Frontiers) -> Frontiers {
    let frontiers = shrink_frontiers(frontiers, oplog.dag()).unwrap_or_else(|_| frontiers.clone());
    calc_shallow_doc_start_from(oplog, frontiers)
}

fn calc_state_only_doc_start(oplog: &crate::OpLog, frontiers: &Frontiers) -> Frontiers {
    calc_shallow_doc_start_from(oplog, frontiers.clone())
}

fn normalize_state_only_target_frontiers(oplog: &crate::OpLog, frontiers: &Frontiers) -> Frontiers {
    if oplog.is_shallow() {
        shrink_frontiers(frontiers, oplog.dag()).unwrap_or_else(|_| frontiers.clone())
    } else {
        frontiers.clone()
    }
}

fn calc_shallow_doc_start_from(oplog: &crate::OpLog, frontiers: Frontiers) -> Frontiers {
    if !oplog.shallow_since_vv().is_empty() {
        // The target frontiers have already been checked by the caller. On a
        // shallow doc, searching for a lower GCA can walk into trimmed history.
        // Keep the requested boundary.
        return advance_style_start_frontiers(oplog, frontiers);
    }

    // Find the LCA of the given frontiers by iteratively pairwise GCA.
    // This converges to a single frontier or empty if there is no common ancestor.
    let mut current = frontiers;
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
        if next.is_empty() || next == current {
            // Cannot converge further (no non-empty GCA, or pairwise GCAs are
            // the nodes themselves).
            // Keep the multi-frontier start so the shallow root still represents
            // the complete boundary instead of falling back to full history.
            break;
        }
        current = next;
    }

    advance_style_start_frontiers(oplog, current)
}

fn advance_style_start_frontiers(oplog: &crate::OpLog, frontiers: Frontiers) -> Frontiers {
    let mut ans = Frontiers::new();
    for id in frontiers.iter() {
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

    ans
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
        let target_frontiers = state.frontiers.clone();
        let Some(oplog_bytes) = oplog.fork_changes_up_to(&target_frontiers) else {
            break 'block Err(LoroEncodeError::FrontiersNotFound(format!(
                "frontiers: {:?} when export in SnapshotAt mode",
                target_frontiers
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

        let alive_containers = state.ensure_all_alive_containers();
        if has_unknown_container(alive_containers.iter()) {
            break 'block Err(LoroEncodeError::UnknownContainer);
        }

        let alive_c_bytes = cids_to_bytes(alive_containers);
        state.store.flush();
        let state_kv = state.store.get_kv_clone();
        state_kv.retain_keys(&alive_c_bytes);
        let bytes = state_kv.export();
        _encode_snapshot(
            Snapshot {
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

    let final_result = match result {
        Err(err) => Err(err),
        Ok(()) => restore_result,
    };

    final_result
}

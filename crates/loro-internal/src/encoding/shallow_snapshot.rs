use bytes::Bytes;
use rle::HasLength;

use loro_common::{ContainerID, ContainerType, LoroEncodeError, ID};

use crate::{
    container::list::list_op::InnerListOp,
    dag::DagUtils,
    encoding::fast_snapshot::{Snapshot, _encode_snapshot},
    state::container_store::FRONTIERS_KEY,
    version::Frontiers,
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
    let oplog = doc.oplog().lock().unwrap();
    let start_from = calc_shallow_doc_start(&oplog, start_from);
    let mut start_vv = oplog.dag().frontiers_to_vv(&start_from).unwrap();
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
    drop(oplog);
    doc._checkout_without_emitting(&start_from, false, false)
        .unwrap();
    let mut state = doc.app_state().lock().unwrap();
    let alive_containers = state.ensure_all_alive_containers();
    if has_unknown_container(alive_containers.ids.iter()) {
        return Err(LoroEncodeError::UnknownContainer);
    }
    let mut alive_c_bytes = alive_containers.all_bytes();
    state.store.flush();
    let shallow_root_state_kv = state.store.get_kv_clone();
    state.merge_gc_into_kv(&alive_containers.gc_keys, &shallow_root_state_kv);
    drop(state);
    doc._checkout_without_emitting(&latest_frontiers, false, false)
        .unwrap();
    let state_bytes = if ops_num > MAX_OPS_NUM_TO_ENCODE_WITHOUT_LATEST_STATE {
        let mut state = doc.app_state().lock().unwrap();
        let post_alive = state.ensure_all_alive_containers();
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
        alive_c_bytes.extend(post_alive.gc_keys.iter().cloned());

        let new_kv = state.store.get_kv_clone();
        state.merge_gc_into_kv(&post_alive.gc_keys, &new_kv);
        new_kv.remove_same(&shallow_root_state_kv);
        new_kv.retain_keys(&alive_c_bytes);
        Some(new_kv.export())
    } else {
        None
    };

    shallow_root_state_kv.retain_keys(&alive_c_bytes);
    shallow_root_state_kv.insert(FRONTIERS_KEY, start_from.encode().into());
    let shallow_root_state_bytes = shallow_root_state_kv.export();

    let snapshot = Snapshot {
        oplog_bytes,
        state_bytes,
        shallow_root_state_bytes,
    };

    if state_frontiers != latest_frontiers {
        doc._checkout_without_emitting(&state_frontiers, false, false)
            .unwrap();
    }

    if is_attached {
        doc.set_detached(false);
    }

    doc.drop_pending_events();
    Ok((snapshot, start_from))
}

fn has_unknown_container<'a>(mut cids: impl Iterator<Item = &'a ContainerID>) -> bool {
    cids.any(|cid| matches!(cid.container_type(), ContainerType::Unknown(_)))
}

pub(crate) fn export_state_only_snapshot<W: std::io::Write>(
    doc: &LoroDoc,
    start_from: &Frontiers,
    w: &mut W,
) -> Result<Frontiers, LoroEncodeError> {
    let oplog = doc.oplog().lock().unwrap();
    let start_from = calc_shallow_doc_start(&oplog, start_from);
    let mut start_vv = oplog.dag().frontiers_to_vv(&start_from).unwrap();
    for id in start_from.iter() {
        // we need to include the ops in start_from, this can make things easier
        start_vv.insert(id.peer, id.counter);
    }

    loro_common::debug!(
        "start version vv={:?} frontiers={:?}",
        &start_vv,
        &start_from,
    );

    let mut to_vv = start_vv.clone();
    for id in start_from.iter() {
        to_vv.insert(id.peer, id.counter + 1);
    }

    let oplog_bytes =
        oplog.export_change_store_in_range(&start_vv, &start_from, &to_vv, &start_from);
    let state_frontiers = doc.state_frontiers();
    let is_attached = !doc.is_detached();
    drop(oplog);
    doc._checkout_without_emitting(&start_from, false, false)
        .unwrap();
    let mut state = doc.app_state().lock().unwrap();
    let alive_containers = state.ensure_all_alive_containers();
    let alive_c_bytes = alive_containers.all_bytes();
    state.store.flush();
    let shallow_state_kv = state.store.get_kv_clone();
    state.merge_gc_into_kv(&alive_containers.gc_keys, &shallow_state_kv);
    drop(state);
    shallow_state_kv.retain_keys(&alive_c_bytes);
    shallow_state_kv.insert(FRONTIERS_KEY, start_from.encode().into());
    let shallow_state_bytes = shallow_state_kv.export();
    // println!("shallow_state_bytes.len = {:?}", shallow_state_bytes.len());
    // println!("oplog_bytes.len = {:?}", oplog_bytes.len());
    let snapshot = Snapshot {
        oplog_bytes,
        state_bytes: None,
        shallow_root_state_bytes: shallow_state_bytes,
    };
    _encode_snapshot(snapshot, w);

    if state_frontiers != start_from {
        doc._checkout_without_emitting(&state_frontiers, false, false)
            .unwrap();
    }

    if is_attached {
        doc.set_detached(false);
    }

    doc.drop_pending_events();
    Ok(start_from)
}

/// Calculates optimal starting version for the shallow doc
///
/// It should be the LCA of the user given version and the latest version.
/// Otherwise, users cannot replay the history from the initial version till the latest version.
fn calc_shallow_doc_start(oplog: &crate::OpLog, frontiers: &Frontiers) -> Frontiers {
    // start is the real start frontiers
    let (mut start, _) = oplog
        .dag()
        .find_common_ancestor(frontiers, oplog.frontiers());

    while start.len() > 1 {
        start.keep_one();
        let (new_start, _) = oplog.dag().find_common_ancestor(&start, oplog.frontiers());
        start = new_start;
    }

    let mut ans = Frontiers::new();
    for id in start.iter() {
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
    let version_before_start = doc.oplog_frontiers();
    doc._checkout_without_emitting(frontiers, true, false)
        .unwrap();
    let result = 'block: {
        let oplog = doc.oplog().lock().unwrap();
        let mut state = doc.app_state().lock().unwrap();
        let is_shallow = state.store.shallow_root_store().is_some();
        if is_shallow {
            unimplemented!()
        }

        assert!(!state.is_in_txn());
        let Some(oplog_bytes) = oplog.fork_changes_up_to(frontiers) else {
            break 'block Err(LoroEncodeError::FrontiersNotFound(format!(
                "frontiers: {:?} when export in SnapshotAt mode",
                frontiers
            )));
        };

        if oplog.is_shallow() {
            assert_eq!(
                oplog.shallow_since_frontiers(),
                state.store.shallow_root_frontiers().unwrap()
            );
        }

        let alive_containers = state.ensure_all_alive_containers();
        if has_unknown_container(alive_containers.ids.iter()) {
            break 'block Err(LoroEncodeError::UnknownContainer);
        }

        let alive_c_bytes = alive_containers.all_bytes();
        state.store.flush();
        let state_kv = state.store.get_kv_clone();
        state.merge_gc_into_kv(&alive_containers.gc_keys, &state_kv);
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
    doc._checkout_without_emitting(&version_before_start, false, false)
        .unwrap();
    if !was_detached {
        doc.set_detached(false);
    }
    doc.drop_pending_events();
    result
}

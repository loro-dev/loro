use rle::HasLength;
use std::collections::BTreeSet;

use loro_common::LoroResult;
use tracing::{debug, trace};

use crate::{
    container::list::list_op::InnerListOp,
    dag::{Dag, DagUtils},
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
pub(crate) fn export_gc_snapshot<W: std::io::Write>(
    doc: &LoroDoc,
    start_from: &Frontiers,
    w: &mut W,
) -> LoroResult<Frontiers> {
    let oplog = doc.oplog().lock().unwrap();
    let start_from = calc_gc_doc_start(&oplog, start_from);
    let mut start_vv = oplog.dag().frontiers_to_vv(&start_from).unwrap();
    for id in start_from.iter() {
        // we need to include the ops in start_from, this can make things easier
        start_vv.insert(id.peer, id.counter);
    }

    #[cfg(debug_assertions)]
    {
        if !start_from.is_empty() {
            assert!(start_from.len() == 1);
            let node = oplog.dag.get(start_from[0]).unwrap();
            if start_from[0].counter == node.cnt {
                let vv = oplog.dag().frontiers_to_vv(&node.deps).unwrap();
                assert_eq!(vv, start_vv);
            } else {
                let vv = oplog
                    .dag()
                    .frontiers_to_vv(&Frontiers::from(start_from[0].inc(-1)))
                    .unwrap();
                assert_eq!(vv, start_vv);
            }
        }
    }

    debug!(
        "start version vv={:?} frontiers={:?}",
        &start_vv, &start_from,
    );

    let oplog_bytes = oplog.export_change_store_from(&start_vv, &start_from);
    let latest_vv = oplog.vv();
    let ops_num: usize = latest_vv.sub_iter(&start_vv).map(|x| x.atom_len()).sum();
    drop(oplog);
    doc.checkout(&start_from)?;
    let mut state = doc.app_state().lock().unwrap();
    let alive_containers = state.ensure_all_alive_containers();
    let alive_c_bytes: BTreeSet<Vec<u8>> = alive_containers.iter().map(|x| x.to_bytes()).collect();
    state.store.flush();
    let gc_state_kv = state.store.get_kv().clone();
    drop(state);
    doc.checkout_to_latest();
    let state_bytes = if ops_num > MAX_OPS_NUM_TO_ENCODE_WITHOUT_LATEST_STATE {
        let mut state = doc.app_state().lock().unwrap();
        state.ensure_all_alive_containers();
        state.store.encode();
        let new_kv = state.store.get_kv().clone();
        new_kv.remove_same(&gc_state_kv);
        new_kv.retain_keys(&alive_c_bytes);
        Some(new_kv.export())
    } else {
        None
    };

    gc_state_kv.retain_keys(&alive_c_bytes);
    gc_state_kv.insert(FRONTIERS_KEY, start_from.encode().into());
    let gc_state_bytes = gc_state_kv.export();

    let snapshot = Snapshot {
        oplog_bytes,
        state_bytes,
        gc_bytes: gc_state_bytes,
    };

    _encode_snapshot(snapshot, w);
    Ok(start_from)
}

pub(crate) fn export_state_only_snapshot<W: std::io::Write>(
    doc: &LoroDoc,
    start_from: &Frontiers,
    w: &mut W,
) -> LoroResult<Frontiers> {
    let oplog = doc.oplog().lock().unwrap();
    let start_from = calc_gc_doc_start(&oplog, start_from);
    trace!("gc_start_from {:?}", &start_from);
    let mut start_vv = oplog.dag().frontiers_to_vv(&start_from).unwrap();
    for id in start_from.iter() {
        // we need to include the ops in start_from, this can make things easier
        start_vv.insert(id.peer, id.counter);
    }

    debug!(
        "start version vv={:?} frontiers={:?}",
        &start_vv, &start_from,
    );

    let mut to_vv = start_vv.clone();
    for id in start_from.iter() {
        to_vv.insert(id.peer, id.counter + 1);
    }

    let oplog_bytes =
        oplog.export_change_store_in_range(&start_vv, &start_from, &to_vv, &start_from);
    drop(oplog);
    doc.checkout(&start_from)?;
    let mut state = doc.app_state().lock().unwrap();
    let alive_containers = state.ensure_all_alive_containers();
    let alive_c_bytes: BTreeSet<Vec<u8>> = alive_containers.iter().map(|x| x.to_bytes()).collect();
    state.store.flush();
    let gc_state_kv = state.store.get_kv().clone();
    drop(state);
    doc.checkout_to_latest();
    let state_bytes = None;
    gc_state_kv.retain_keys(&alive_c_bytes);
    gc_state_kv.insert(FRONTIERS_KEY, start_from.encode().into());
    let gc_state_bytes = gc_state_kv.export();
    let snapshot = Snapshot {
        oplog_bytes,
        state_bytes,
        gc_bytes: gc_state_bytes,
    };

    _encode_snapshot(snapshot, w);
    Ok(start_from)
}

/// Calculates optimal starting version for the trimmed doc
///
/// It should be the LCA of the user given version and the latest version.
/// Otherwise, users cannot replay the history from the initial version till the latest version.
fn calc_gc_doc_start(oplog: &crate::OpLog, frontiers: &Frontiers) -> Frontiers {
    // start is the real start frontiers
    let (mut start, _) = oplog
        .dag()
        .find_common_ancestor(frontiers, oplog.frontiers());
    while start.len() > 1 {
        start.drain(1..);
        let (new_start, _) = oplog.dag().find_common_ancestor(&start, oplog.frontiers());
        start = new_start;
    }

    for id in start.iter_mut() {
        if let Some(op) = oplog.get_op_that_includes(*id) {
            if let crate::op::InnerContent::List(InnerListOp::StyleStart { .. }) = &op.content {
                // StyleStart and StyleEnd operations must be kept together in the GC snapshot.
                // Splitting them could lead to an weird document state that cannot be
                // properly encoded. To ensure they stay together, we advance the frontier by
                // one step to include both operations.

                // > Id.counter + 1 is guaranteed to be the StyleEnd Op
                id.counter += 1;
            }
        }
    }

    start
}

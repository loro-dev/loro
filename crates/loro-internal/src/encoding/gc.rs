use bytes::Bytes;
use loro_common::LoroResult;
use tracing::debug;

use crate::{
    dag::DagUtils,
    encoding::fast_snapshot::{Snapshot, _encode_snapshot},
    version::Frontiers,
    LoroDoc,
};

use super::fast_snapshot::_decode_snapshot_bytes;

#[tracing::instrument(skip_all)]
pub(crate) fn export_gc_snapshot<W: std::io::Write>(
    doc: &LoroDoc,
    start_from: &Frontiers,
    w: &mut W,
) -> LoroResult<Frontiers> {
    assert!(!doc.is_detached());
    let oplog = doc.oplog().lock().unwrap();
    let start_from = calc_actual_start(&oplog, start_from);
    let start_vv = oplog.dag().frontiers_to_vv(&start_from).unwrap();
    debug!(
        "start version vv={:?} frontiers={:?}",
        &start_vv, &start_from,
    );

    let oplog_bytes = oplog.export_from_fast(&start_vv);
    drop(oplog);
    doc.checkout(&start_from)?;
    let mut state = doc.app_state().lock().unwrap();
    let gc_state_bytes = state.store.encode();
    let old_kv = state.store.get_kv().clone();
    drop(state);
    doc.checkout_to_latest();
    let mut state = doc.app_state().lock().unwrap();
    state.store.encode();
    let new_kv = state.store.get_kv().clone();
    new_kv.remove_same(&old_kv);
    let state_bytes = new_kv.export();
    let snapshot = Snapshot {
        oplog_bytes,
        state_bytes,
        gc_bytes: gc_state_bytes,
    };

    _encode_snapshot(snapshot, w);
    Ok(start_from)
}

/// The real start version should be the lca of the given one and the latest frontiers
fn calc_actual_start(oplog: &crate::OpLog, frontiers: &Frontiers) -> Frontiers {
    let mut frontiers = frontiers;
    let f;
    if frontiers == oplog.frontiers() && !frontiers.is_empty() {
        // This is not allowed.
        // We need to at least export one op
        f = Some(oplog.get_deps_of(frontiers[0]).unwrap());
        frontiers = f.as_ref().unwrap();
    }

    // start is the real start frontiers
    let (start, _) = oplog
        .dag()
        .find_common_ancestor(frontiers, oplog.frontiers());

    let cur_f = oplog.frontiers();
    oplog.dag.find_common_ancestor(&start, cur_f).0
}

pub(crate) fn import_gc_snapshot(doc: &LoroDoc, bytes: Bytes) -> LoroResult<()> {
    let mut oplog = doc.oplog().lock().unwrap();
    let mut state = doc.app_state().lock().unwrap();
    if !oplog.is_empty() || !state.is_empty() {
        panic!()
    }

    let Snapshot {
        oplog_bytes,
        state_bytes,
        gc_bytes,
    } = _decode_snapshot_bytes(bytes)?;
    oplog.decode_change_store(oplog_bytes)?;
    if !gc_bytes.is_empty() {
        state
            .store
            .decode_gc(gc_bytes, state_bytes, oplog.dag().start_frontiers().clone())?;
    } else {
        state.store.decode(state_bytes)?;
    }
    // FIXME: we may need to extract the unknown containers here?
    // Or we should lazy load it when the time comes?
    state.init_with_states_and_version(oplog.frontiers().clone(), &oplog, vec![], false);
    Ok(())
}

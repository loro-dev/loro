use rle::HasLength;
use std::collections::BTreeSet;

use loro_common::LoroResult;
use tracing::{debug, trace};

use crate::{
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
pub(crate) fn export_gc_snapshot<W: std::io::Write>(
    doc: &LoroDoc,
    start_from: &Frontiers,
    w: &mut W,
) -> LoroResult<Frontiers> {
    assert!(!doc.is_detached());
    let oplog = doc.oplog().lock().unwrap();
    trace!("start_from: {:?}", &start_from);
    let start_from = calc_actual_start(&oplog, start_from);
    let mut start_vv = oplog.dag().frontiers_to_vv(&start_from).unwrap();
    trace!("start_from: {:?}", &start_from);
    for id in start_from.iter() {
        // we need to include the ops in start_from, this can make things easier
        start_vv.insert(id.peer, id.counter);
    }
    debug!(
        "start version vv={:?} frontiers={:?}",
        &start_vv, &start_from,
    );

    let oplog_bytes = oplog.export_from_fast(&start_vv);
    let latest_vv = oplog.vv();
    let ops_num: usize = latest_vv.sub_iter(&start_vv).map(|x| x.atom_len()).sum();
    drop(oplog);
    doc.checkout(&start_from)?;
    let mut state = doc.app_state().lock().unwrap();
    let alive_containers = state.get_all_alive_containers();
    let alive_c_bytes: BTreeSet<Vec<u8>> = alive_containers.iter().map(|x| x.to_bytes()).collect();
    state.store.flush();
    let gc_state_kv = state.store.get_kv().clone();
    drop(state);
    doc.checkout_to_latest();
    let state_bytes = if ops_num > MAX_OPS_NUM_TO_ENCODE_WITHOUT_LATEST_STATE {
        let mut state = doc.app_state().lock().unwrap();
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

/// The real start version should be the lca of the given one and the latest frontiers
fn calc_actual_start(oplog: &crate::OpLog, frontiers: &Frontiers) -> Frontiers {
    // start is the real start frontiers
    let (start, _) = oplog
        .dag()
        .find_common_ancestor(frontiers, oplog.frontiers());

    let cur_f = oplog.frontiers();
    oplog.dag.find_common_ancestor(&start, cur_f).0
}

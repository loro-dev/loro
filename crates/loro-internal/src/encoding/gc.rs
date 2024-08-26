use crate::{dag::DagUtils, version::Frontiers, LoroDoc};

pub(crate) fn export_gc_snapshot(doc: &LoroDoc, frontiers: &Frontiers) -> (Vec<u8>, Frontiers) {
    let oplog = doc.oplog().lock().unwrap();
    // start is the real start frontiers
    let (start, _) = oplog
        .dag()
        .find_common_ancestor(&frontiers, &oplog.frontiers());

    todo!()
}

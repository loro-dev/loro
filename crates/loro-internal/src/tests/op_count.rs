use crate::{loro::ExportMode, LoroDoc};
use std::sync::atomic::Ordering::Release;

#[test]
fn len_ops_is_available_while_state_is_locked_with_local_ops() {
    let doc = LoroDoc::new_auto_commit();
    doc.set_peer_id(1).unwrap();
    doc.get_map("map").insert("k", 1).unwrap();

    let _state = doc.app_state().lock();
    assert_eq!(doc.len_ops(), 1);
}

#[test]
fn len_ops_tracks_snapshot_import_while_state_is_locked() {
    let src = LoroDoc::new_auto_commit();
    src.set_peer_id(2).unwrap();
    src.get_map("map").insert("k", 1).unwrap();
    src.commit_then_renew();
    let expected = src.len_ops();
    let snapshot = src.export(ExportMode::Snapshot).unwrap();

    let dst = LoroDoc::new();
    dst.import(&snapshot).unwrap();

    let _state = dst.app_state().lock();
    assert_eq!(dst.len_ops(), expected);
}

#[test]
fn len_ops_prefers_exact_oplog_value_when_lock_order_allows() {
    let doc = LoroDoc::new_auto_commit();
    doc.set_peer_id(3).unwrap();
    doc.get_map("map").insert("k", 1).unwrap();

    doc.visible_op_count.store(0, Release);
    assert_eq!(doc.len_ops(), 1);
}

use crate::{
    loro::ExportMode, state::fail_next_import_state_apply_for_test, version::VersionVector, LoroDoc,
};
use std::sync::atomic::Ordering::{Acquire, Release};

fn assert_visible_op_count_cache_matches_exact(doc: &LoroDoc) {
    let exact = doc.oplog().lock().visible_op_count_exact();
    assert_eq!(doc.visible_op_count.load(Acquire), exact);

    let _state = doc.app_state().lock();
    assert_eq!(doc.len_ops(), exact);
}

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

#[test]
fn visible_op_count_cache_tracks_pending_activation() {
    let src = LoroDoc::new_auto_commit();
    src.set_peer_id(4).unwrap();
    let map = src.get_map("map");
    map.insert("base", "base").unwrap();
    let update_base = src
        .export(ExportMode::updates(&VersionVector::default()))
        .unwrap();
    let version_base = src.oplog_vv();

    map.insert("later", "later").unwrap();
    let update_later = src.export(ExportMode::updates(&version_base)).unwrap();

    let dst = LoroDoc::new();
    let status = dst.import(&update_later).unwrap();
    assert!(status.pending.is_some());
    assert_visible_op_count_cache_matches_exact(&dst);
    assert_eq!(dst.len_ops(), 0);

    let status = dst.import(&update_base).unwrap();
    assert!(status.pending.is_none());
    assert_eq!(dst.len_ops(), src.len_ops());
    assert_visible_op_count_cache_matches_exact(&dst);
}

#[test]
fn visible_op_count_cache_rolls_back_after_failed_pending_activation() {
    let src = LoroDoc::new_auto_commit();
    src.set_peer_id(5).unwrap();
    let map = src.get_map("map");
    map.insert("base", "base").unwrap();
    let update_base = src
        .export(ExportMode::updates(&VersionVector::default()))
        .unwrap();
    let version_base = src.oplog_vv();

    map.insert("later", "later").unwrap();
    let update_later = src.export(ExportMode::updates(&version_base)).unwrap();

    let dst = LoroDoc::new();
    dst.import(&update_later).unwrap();
    assert_eq!(dst.len_ops(), 0);
    assert_visible_op_count_cache_matches_exact(&dst);

    fail_next_import_state_apply_for_test();
    let err = dst.import(&update_base).unwrap_err();
    assert!(
        err.to_string().contains("state apply failpoint"),
        "unexpected error: {err:?}"
    );
    assert_eq!(dst.len_ops(), 0);
    assert_visible_op_count_cache_matches_exact(&dst);

    dst.import(&update_base).unwrap();
    assert_eq!(dst.len_ops(), src.len_ops());
    assert_visible_op_count_cache_matches_exact(&dst);
}

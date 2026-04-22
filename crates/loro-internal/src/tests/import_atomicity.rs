use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

use crate::{
    cursor::PosType,
    encoding::json_schema::json::{JsonOpContent, JsonSchema, ListOp},
    loro::ExportMode,
    state::fail_next_import_state_apply_for_test,
    version::{Frontiers, VersionVector},
    LoroDoc, LoroError, TreeParentId,
};

fn pending_len(doc: &LoroDoc) -> usize {
    doc.oplog().lock().pending_changes_len()
}

fn corrupt_snapshot_state_bytes(snapshot: &mut [u8]) {
    let body_start = 22;
    let oplog_len =
        u32::from_le_bytes(snapshot[body_start..body_start + 4].try_into().unwrap()) as usize;
    let state_len_pos = body_start + 4 + oplog_len;
    let state_len = u32::from_le_bytes(
        snapshot[state_len_pos..state_len_pos + 4]
            .try_into()
            .unwrap(),
    ) as usize;
    assert!(state_len > 0);
    let state_start = state_len_pos + 4;
    snapshot[state_start] ^= 0xff;

    let checksum = xxhash_rust::xxh32::xxh32(&snapshot[20..], u32::from_le_bytes(*b"LORO"));
    snapshot[16..20].copy_from_slice(&checksum.to_le_bytes());
}

fn make_json_list_update_with_four_ops(peer: u64) -> (LoroDoc, JsonSchema) {
    let doc = LoroDoc::new();
    doc.set_peer_id(peer).unwrap();
    let map = doc.get_map("map");
    let list = doc.get_list("list");
    let text = doc.get_text("text");

    let mut txn = doc.txn().unwrap();
    map.insert_with_txn(&mut txn, "prefix", "map-value".into())
        .unwrap();
    list.insert_with_txn(&mut txn, 0, "seed".into()).unwrap();
    text.insert_with_txn(&mut txn, 0, "text-value", PosType::Unicode)
        .unwrap();
    list.insert_with_txn(&mut txn, 1, "tail".into()).unwrap();
    txn.commit().unwrap();

    let json = doc.export_json_updates(&Default::default(), &doc.oplog_vv(), false);
    assert_eq!(json.changes.len(), 1);
    assert_eq!(json.changes[0].ops.len(), 4);
    (doc, json)
}

fn move_last_list_insert_far_out_of_bounds(json: &mut JsonSchema) {
    let last_change = json.changes.last_mut().unwrap();
    let last_op = last_change.ops.last_mut().unwrap();
    match &mut last_op.content {
        JsonOpContent::List(ListOp::Insert { pos, .. }) => {
            *pos = 1_000;
        }
        other => panic!("expected list insert op, got {other:?}"),
    }
}

fn make_multi_peer_frontier_doc() -> LoroDoc {
    let base = LoroDoc::new_auto_commit();
    base.set_peer_id(1).unwrap();
    base.get_map("map").insert("base", 0).unwrap();
    base.get_text("text").insert_unicode(0, "base").unwrap();
    let tree = base.get_tree("tree");
    let root = tree.create(TreeParentId::Root).unwrap();
    tree.get_meta(root).unwrap().insert("base", 0).unwrap();

    let base_updates = base.export(ExportMode::all_updates()).unwrap();

    let peer2 = base.fork();
    peer2.set_peer_id(2).unwrap();
    peer2.get_map("map").insert("p2", 2).unwrap();
    peer2.get_text("text").insert_unicode(0, "p2").unwrap();
    peer2.commit_then_renew();
    let peer2_updates = peer2.export(ExportMode::updates(&base.oplog_vv())).unwrap();

    let peer3 = base.fork();
    peer3.set_peer_id(3).unwrap();
    peer3.get_map("map").insert("p3", 3).unwrap();
    let peer3_tree = peer3.get_tree("tree");
    let node = peer3_tree.create(TreeParentId::Root).unwrap();
    peer3_tree.get_meta(node).unwrap().insert("p3", 3).unwrap();
    peer3.commit_then_renew();
    let peer3_updates = peer3.export(ExportMode::updates(&base.oplog_vv())).unwrap();

    let target = LoroDoc::new();
    target.import(&base_updates).unwrap();
    target.import(&peer2_updates).unwrap();
    target.import(&peer3_updates).unwrap();
    target
}

fn assert_doc_unchanged(
    doc: &LoroDoc,
    vv: &VersionVector,
    frontiers: &Frontiers,
    state: &crate::LoroValue,
) {
    assert_eq!(&doc.oplog_vv(), vv);
    assert_eq!(&doc.oplog_frontiers(), frontiers);
    assert_eq!(&doc.get_deep_value(), state);
}

#[test]
fn failed_dependency_import_rolls_back_single_pending_change() {
    let src = LoroDoc::new_auto_commit();
    src.set_peer_id(1).unwrap();
    let map = src.get_map("map");
    map.insert("seed", "base").unwrap();
    let update_base = src
        .export(ExportMode::updates(&VersionVector::default()))
        .unwrap();
    let version_base = src.oplog_vv();

    map.insert("later", "value").unwrap();
    let update_later = src.export(ExportMode::updates(&version_base)).unwrap();

    let dst = LoroDoc::new();
    dst.import(&update_later).unwrap();
    assert_eq!(pending_len(&dst), 1);
    let vv_before_import = dst.oplog_vv();
    let frontiers_before_import = dst.oplog_frontiers();
    let state_before_import = dst.get_deep_value();

    fail_next_import_state_apply_for_test();
    let err = dst.import(&update_base).unwrap_err();
    assert!(
        err.to_string().contains("state apply failpoint"),
        "unexpected error: {err:?}"
    );
    assert_eq!(pending_len(&dst), 1);
    assert_eq!(dst.oplog_vv(), vv_before_import);
    assert_eq!(dst.oplog_frontiers(), frontiers_before_import);
    assert_eq!(dst.get_deep_value(), state_before_import);

    dst.import(&update_base).unwrap();
    assert_eq!(pending_len(&dst), 0);
    assert_eq!(dst.oplog_vv(), src.oplog_vv());
    assert_eq!(dst.oplog_frontiers(), src.oplog_frontiers());
    assert_eq!(dst.get_deep_value(), src.get_deep_value());
}

#[test]
fn failed_dependency_import_rolls_back_multiple_pending_changes() {
    let base = LoroDoc::new_auto_commit();
    base.set_peer_id(1).unwrap();
    let map = base.get_map("map");
    map.insert("seed", "base").unwrap();
    let update_base = base
        .export(ExportMode::updates(&VersionVector::default()))
        .unwrap();
    let version_base = base.oplog_vv();

    let peer2 = LoroDoc::new_auto_commit();
    peer2.set_peer_id(2).unwrap();
    peer2.import(&update_base).unwrap();
    peer2.get_map("map").insert("p2", "B").unwrap();
    let update_peer2 = peer2.export(ExportMode::updates(&version_base)).unwrap();

    let peer3 = LoroDoc::new_auto_commit();
    peer3.set_peer_id(3).unwrap();
    peer3.import(&update_base).unwrap();
    peer3.get_map("map").insert("p3", "C").unwrap();
    let update_peer3 = peer3.export(ExportMode::updates(&version_base)).unwrap();

    let expected = LoroDoc::new();
    expected.import(&update_base).unwrap();
    expected.import(&update_peer2).unwrap();
    expected.import(&update_peer3).unwrap();

    let dst = LoroDoc::new();
    dst.import(&update_peer2).unwrap();
    dst.import(&update_peer3).unwrap();
    assert_eq!(pending_len(&dst), 2);

    let vv_before_import = dst.oplog_vv();
    let frontiers_before_import: Frontiers = dst.oplog_frontiers();
    let state_before_import = dst.get_deep_value();

    fail_next_import_state_apply_for_test();
    let err = dst.import(&update_base).unwrap_err();
    assert!(
        err.to_string().contains("state apply failpoint"),
        "unexpected error: {err:?}"
    );
    assert_eq!(pending_len(&dst), 2);
    assert_eq!(dst.oplog_vv(), vv_before_import);
    assert_eq!(dst.oplog_frontiers(), frontiers_before_import);
    assert_eq!(dst.get_deep_value(), state_before_import);

    dst.import(&update_base).unwrap();
    assert_eq!(pending_len(&dst), 0);
    assert_eq!(dst.oplog_vv(), expected.oplog_vv());
    assert_eq!(dst.oplog_frontiers(), expected.oplog_frontiers());
    assert_eq!(dst.get_deep_value(), expected.get_deep_value());
}

#[test]
fn failed_import_keeps_multi_peer_frontiers_intact() {
    let target = make_multi_peer_frontier_doc();
    let vv_before_import = target.oplog_vv();
    assert!(vv_before_import.iter().count() >= 3);
    let frontiers_before_import = target.oplog_frontiers();
    let state_before_import = target.get_deep_value();

    let (_, mut bad_json) = make_json_list_update_with_four_ops(4);
    move_last_list_insert_far_out_of_bounds(&mut bad_json);
    let bad_json = serde_json::to_string(&bad_json).unwrap();

    let err = target.import_json_updates(&bad_json).unwrap_err();
    assert!(
        err.to_string().contains("list diff"),
        "expected state list bounds validation, got {err:?}"
    );
    assert_eq!(target.oplog_vv(), vv_before_import);
    assert_eq!(target.oplog_frontiers(), frontiers_before_import);
    assert_eq!(target.get_deep_value(), state_before_import);
}

#[test]
fn malformed_json_import_returns_error_without_mutating_doc() {
    let doc = make_multi_peer_frontier_doc();
    let vv_before_import = doc.oplog_vv();
    let frontiers_before_import = doc.oplog_frontiers();
    let state_before_import = doc.get_deep_value();

    let err = doc
        .import_json_updates("[3,{ \"('  k\" :\n\n42222 }]")
        .unwrap_err();
    assert_eq!(err, LoroError::InvalidJsonSchema);
    assert_doc_unchanged(
        &doc,
        &vv_before_import,
        &frontiers_before_import,
        &state_before_import,
    );
}

#[test]
fn failed_import_does_not_emit_events() {
    let doc = LoroDoc::new();
    let hit = Arc::new(AtomicUsize::new(0));
    let hit_cloned = hit.clone();
    let _sub = doc.subscribe_root(Arc::new(move |_event| {
        hit_cloned.fetch_add(1, Ordering::SeqCst);
    }));

    let (_, mut bad_json) = make_json_list_update_with_four_ops(7);
    move_last_list_insert_far_out_of_bounds(&mut bad_json);
    let bad_json = serde_json::to_string(&bad_json).unwrap();
    let err = doc.import_json_updates(&bad_json).unwrap_err();
    assert!(
        err.to_string().contains("list diff"),
        "expected state list bounds validation, got {err:?}"
    );
    assert_eq!(hit.load(Ordering::SeqCst), 0);
    assert!(doc.drop_pending_events().is_empty());

    let (_, good_json) = make_json_list_update_with_four_ops(8);
    let good_json = serde_json::to_string(&good_json).unwrap();
    doc.import_json_updates(&good_json).unwrap();
    assert!(hit.load(Ordering::SeqCst) > 0);
}

#[test]
fn corrupt_snapshot_import_rolls_back_empty_doc() {
    let src = LoroDoc::new_auto_commit();
    src.set_peer_id(9).unwrap();
    src.get_text("text").insert_unicode(0, "snapshot").unwrap();
    src.get_list("list").push("value").unwrap();
    let snapshot = src.export(ExportMode::Snapshot).unwrap();
    let mut corrupt_snapshot = snapshot.clone();
    corrupt_snapshot_state_bytes(&mut corrupt_snapshot);

    let dst = LoroDoc::new();
    let vv_before_import = dst.oplog_vv();
    let frontiers_before_import = dst.oplog_frontiers();
    let state_before_import = dst.get_deep_value();
    let err = dst.import(&corrupt_snapshot).unwrap_err();
    assert!(
        err.to_string().contains("decode_snapshot")
            || err.to_string().contains("Decode")
            || err.to_string().contains("snapshot"),
        "unexpected error: {err:?}"
    );
    assert_eq!(dst.oplog_vv(), vv_before_import);
    assert_eq!(dst.oplog_frontiers(), frontiers_before_import);
    assert_eq!(dst.get_deep_value(), state_before_import);
    assert!(dst.oplog().lock().is_empty());

    dst.import(&snapshot).unwrap();
    assert_eq!(dst.get_deep_value(), src.get_deep_value());
}

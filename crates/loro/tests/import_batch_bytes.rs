use std::sync::{Arc, Mutex};

use loro::{bytes::Bytes, ExportMode, ImportBlobOutcome, LoroDoc, LoroError, ToJson};
use pretty_assertions::assert_eq;

fn assert_meta_eq(a: &loro::ImportBlobMetadata, b: &loro::ImportBlobMetadata) {
    assert_eq!(a.mode, b.mode);
    assert_eq!(a.partial_start_vv, b.partial_start_vv);
    assert_eq!(a.partial_end_vv, b.partial_end_vv);
    assert_eq!(a.start_timestamp, b.start_timestamp);
    assert_eq!(a.start_frontiers, b.start_frontiers);
    assert_eq!(a.end_timestamp, b.end_timestamp);
    assert_eq!(a.change_num, b.change_num);
}

#[test]
fn import_bytes_and_meta_bytes_match_slice_apis() {
    let a = LoroDoc::new();
    a.set_peer_id(1).unwrap();
    a.get_text("t").insert(0, "hello").unwrap();
    a.commit();

    let snapshot = Bytes::from(a.export(ExportMode::Snapshot).unwrap());
    let vv = a.oplog_vv();

    a.get_text("t").insert(5, " world").unwrap();
    a.commit();
    let updates = Bytes::from(a.export(ExportMode::updates(&vv)).unwrap());

    let snap_meta_slice = LoroDoc::decode_import_blob_meta(&snapshot, true).unwrap();
    let snap_meta_bytes = LoroDoc::decode_import_blob_meta_bytes(snapshot.clone(), true).unwrap();
    assert_meta_eq(&snap_meta_slice, &snap_meta_bytes);

    let upd_meta_slice = LoroDoc::decode_import_blob_meta(&updates, true).unwrap();
    let upd_meta_bytes = LoroDoc::decode_import_blob_meta_bytes(updates.clone(), true).unwrap();
    assert_meta_eq(&upd_meta_slice, &upd_meta_bytes);

    let b = LoroDoc::new();
    let origins: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let origins_clone = origins.clone();
    let _sub = b.subscribe_root(Arc::new(move |ev| {
        origins_clone.lock().unwrap().push(ev.origin.to_string());
    }));

    b.import_bytes(snapshot).unwrap();
    b.import_with_bytes(updates, "custom-bytes-origin").unwrap();

    assert_eq!(b.get_deep_value(), a.get_deep_value());
    assert_eq!(
        origins.lock().unwrap().as_slice(),
        &["", "custom-bytes-origin"]
    );
}

#[test]
fn import_batch_bytes_empty_single_and_out_of_order_multi() {
    let empty_doc = LoroDoc::new();
    let empty_status = empty_doc.import_batch_bytes(Vec::new()).unwrap();
    assert!(empty_status.success.is_empty());
    assert!(empty_status.pending.is_none());

    let src = LoroDoc::new();
    src.set_peer_id(1).unwrap();
    src.get_text("t").insert(0, "base").unwrap();
    src.commit();
    let snapshot = Bytes::from(src.export(ExportMode::Snapshot).unwrap());
    let v1 = src.oplog_vv();

    src.get_text("t").insert(4, " one").unwrap();
    src.commit();
    let u1 = Bytes::from(src.export(ExportMode::updates(&v1)).unwrap());
    let v2 = src.oplog_vv();

    src.get_text("t").insert(8, " two").unwrap();
    src.commit();
    let u2 = Bytes::from(src.export(ExportMode::updates(&v2)).unwrap());

    let single = LoroDoc::new();
    single.import_batch_bytes(vec![snapshot.clone()]).unwrap();
    assert_eq!(single.get_text("t").to_string(), "base");

    let dst = LoroDoc::new();
    let status = dst.import_batch_bytes(vec![u2, u1, snapshot]).unwrap();
    assert!(status.pending.is_none());
    assert_eq!(
        dst.get_deep_value().to_json_value(),
        src.get_deep_value().to_json_value()
    );
}

#[test]
fn import_batch_detailed_bytes_reports_outcomes_and_refuses_in_txn() {
    let src = LoroDoc::new();
    src.set_peer_id(2).unwrap();
    src.get_map("m").insert("k", "v1").unwrap();
    src.commit();
    let v1 = src.oplog_vv();
    let u1 = Bytes::from(src.export(ExportMode::all_updates()).unwrap());
    src.get_map("m").insert("k2", "v2").unwrap();
    src.commit();
    let u2 = Bytes::from(src.export(ExportMode::updates(&v1)).unwrap());

    // All-empty input
    let empty = LoroDoc::new();
    let empty_detail = empty.import_batch_detailed_bytes(Vec::new()).unwrap();
    assert!(empty_detail.blobs.is_empty());
    assert!(empty_detail.rollback_error.is_none());

    // Single invalid blob
    let single_bad = LoroDoc::new();
    let detail = single_bad
        .import_batch_detailed_bytes(vec![Bytes::from_static(b"invalid")])
        .unwrap();
    assert!(matches!(detail.blobs[0], ImportBlobOutcome::Failed(_)));

    // Mixed valid + invalid out-of-order batch
    let dst = LoroDoc::new();
    let bad = Bytes::from_static(b"corrupted blob");
    let detail = dst
        .import_batch_detailed_bytes(vec![u2.clone(), bad, u1.clone()])
        .unwrap();
    assert!(detail.rollback_error.is_none());
    assert_eq!(detail.blobs.len(), 3);
    assert!(matches!(detail.blobs[0], ImportBlobOutcome::Imported(_)));
    assert!(matches!(detail.blobs[1], ImportBlobOutcome::Failed(_)));
    assert!(matches!(detail.blobs[2], ImportBlobOutcome::Imported(_)));
    assert_eq!(
        dst.get_deep_value().to_json_value(),
        src.get_deep_value().to_json_value()
    );

    // Calling import_batch_bytes / import_batch_detailed_bytes inside a pre-commit callback returns ImportWhenInTxn.
    let nested_doc = LoroDoc::new();
    let err_slot: Arc<Mutex<Option<LoroError>>> = Arc::new(Mutex::new(None));
    let detailed_err_slot: Arc<Mutex<Option<LoroError>>> = Arc::new(Mutex::new(None));
    let err_slot_clone = err_slot.clone();
    let detailed_err_slot_clone = detailed_err_slot.clone();
    let doc_clone = nested_doc.clone();
    let _sub = nested_doc.subscribe_pre_commit(Box::new(move |_| {
        let res = doc_clone.import_batch_bytes(vec![u1.clone(), u2.clone()]);
        *err_slot_clone.lock().unwrap() = res.err();
        let detailed = doc_clone.import_batch_detailed_bytes(vec![u1.clone(), u2.clone()]);
        *detailed_err_slot_clone.lock().unwrap() = detailed.err();
        true
    }));
    nested_doc.get_text("t").insert(0, "local").unwrap();
    nested_doc.commit();
    assert!(matches!(
        *err_slot.lock().unwrap(),
        Some(LoroError::ImportWhenInTxn)
    ));
    assert!(matches!(
        *detailed_err_slot.lock().unwrap(),
        Some(LoroError::ImportWhenInTxn)
    ));
}

#[test]
fn snapshot_at_and_fork_at_match_deep_value_across_containers() {
    let doc = LoroDoc::new();
    doc.set_peer_id(10).unwrap();
    doc.get_text("text").insert(0, "alpha").unwrap();
    doc.get_map("map").insert("a", 1).unwrap();
    let list = doc.get_list("list");
    list.push("x").unwrap();
    let mlist = doc.get_movable_list("mlist");
    mlist.push(10).unwrap();
    let tree = doc.get_tree("tree");
    let root = tree.create(None).unwrap();
    tree.get_meta(root).unwrap().insert("name", "r").unwrap();
    doc.commit();

    let mid_frontiers = doc.oplog_frontiers();
    let expected_mid = doc.get_deep_value().to_json_value();

    // Advance past mid_frontiers with edits on a second peer.
    let peer2 = LoroDoc::new();
    peer2.set_peer_id(20).unwrap();
    peer2
        .import(&doc.export(ExportMode::Snapshot).unwrap())
        .unwrap();
    peer2.get_text("text").insert(5, " beta").unwrap();
    peer2.get_map("map").insert("b", 2).unwrap();
    peer2.get_list("list").push("y").unwrap();
    peer2.get_movable_list("mlist").mov(0, 0).unwrap();
    let child = peer2.get_tree("tree").create(root).unwrap();
    peer2
        .get_tree("tree")
        .get_meta(child)
        .unwrap()
        .insert("name", "c")
        .unwrap();
    peer2.commit();

    doc.import(&peer2.export(ExportMode::all_updates()).unwrap())
        .unwrap();

    let snap_at = doc.export(ExportMode::snapshot_at(&mid_frontiers)).unwrap();
    let restored = LoroDoc::from_snapshot_bytes(Bytes::from(snap_at)).unwrap();
    assert_eq!(restored.get_deep_value().to_json_value(), expected_mid);

    let forked = doc.fork_at(&mid_frontiers).unwrap();
    assert_eq!(forked.get_deep_value().to_json_value(), expected_mid);
}

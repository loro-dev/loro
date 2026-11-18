use std::sync::{Arc, Mutex};

use loro_internal::{
    configure::{StyleConfig, StyleConfigMap},
    cursor::PosType,
    encoding::ExportMode,
    loro::CommitOptions,
    IdSpan, LoroDoc, LoroValue, Subscription, TreeParentId, VersionVector,
};

fn check_update(
    source: &LoroDoc,
    replica: &LoroDoc,
    before: VersionVector,
    updates: &Mutex<Vec<Vec<u8>>>,
) -> Vec<u8> {
    let peer = source.peer_id();
    let span = IdSpan::new(
        peer,
        before.get(&peer).copied().unwrap_or(0),
        source.oplog_vv()[&peer],
    );
    let update = {
        let mut updates = updates.lock().unwrap();
        assert_eq!(updates.len(), 1);
        updates.pop().unwrap()
    };
    let range = source
        .export(ExportMode::updates_in_range([span].as_slice()))
        .unwrap();
    assert_eq!(update, range);
    assert!(replica.import(&update).unwrap().pending.is_none());
    assert_eq!(replica.get_deep_value(), source.get_deep_value());
    assert_eq!(replica.oplog_frontiers(), source.oplog_frontiers());
    assert_eq!(replica.oplog_vv(), source.oplog_vv());
    // The replica may represent the same history using different Change
    // boundaries. Check the public wire metadata against the exact range.
    let metadata = LoroDoc::decode_import_blob_meta(&update, true).unwrap();
    let range_metadata = LoroDoc::decode_import_blob_meta(&range, true).unwrap();
    assert_eq!(metadata.start_timestamp, range_metadata.start_timestamp);
    assert_eq!(metadata.end_timestamp, range_metadata.end_timestamp);
    assert_eq!(metadata.start_frontiers, range_metadata.start_frontiers);
    update
}

#[test]
fn local_update_matches_range_export_for_small_mixed_changes() {
    let source = LoroDoc::new_auto_commit();
    let replica = LoroDoc::new_auto_commit();
    source.set_peer_id(100).unwrap();
    let mut styles = StyleConfigMap::new();
    styles.insert("bold".into(), StyleConfig::new());
    source.config_text_style(styles);

    for peer in 1..=12 {
        let remote = LoroDoc::new_auto_commit();
        remote.set_peer_id(peer).unwrap();
        remote
            .get_map("peers")
            .insert(&format!("p{peer}"), true)
            .unwrap();
        let update = remote.export(ExportMode::all_updates()).unwrap();
        source.import(&update).unwrap();
        replica.import(&update).unwrap();
    }

    let updates = Arc::new(Mutex::new(Vec::new()));
    let inbox = updates.clone();
    let _sub = source.subscribe_local_update(Box::new(move |bytes| {
        inbox.lock().unwrap().push(bytes.clone());
        true
    }));
    let text = source.get_text("text");
    let list = source.get_list("list");
    let movable = source.get_movable_list("movable");
    let tree = source.get_tree("tree");
    replica.get_list("list");
    replica.get_movable_list("movable");
    replica.get_tree("tree");
    tree.enable_fractional_index(0);

    let before = source.oplog_vv();
    text.insert_utf16(0, "a中😀").unwrap();
    let commit_options = |timestamp| {
        CommitOptions::new()
            .timestamp(timestamp)
            .commit_msg("typed")
    };
    source.commit_with(commit_options(100));
    let update = check_update(&source, &replica, before, &updates);
    let metadata = LoroDoc::decode_import_blob_meta(&update, true).unwrap();
    assert_eq!(metadata.start_frontiers.len(), 12);
    assert_eq!(metadata.start_timestamp, 100);

    let before = source.oplog_vv();
    text.insert_utf16(4, "b").unwrap();
    source.commit_with(commit_options(101));
    let update = check_update(&source, &replica, before, &updates);
    // A merged change retains its previous timestamp.
    assert_eq!(
        LoroDoc::decode_import_blob_meta(&update, true)
            .unwrap()
            .start_timestamp,
        100
    );

    let before = source.oplog_vv();
    text.mark(0, 4, "bold", LoroValue::Bool(true), PosType::Utf16)
        .unwrap();
    list.insert(0, "left").unwrap();
    movable.insert(0, "x").unwrap();
    movable.insert(1, "y").unwrap();
    let a = tree.create(TreeParentId::Root).unwrap();
    let b = tree.create(TreeParentId::Root).unwrap();
    source.commit_then_renew();
    check_update(&source, &replica, before, &updates);

    let _pre_commit = source.subscribe_pre_commit(Box::new(|event| {
        event
            .modifier
            .set_message("changed by hook")
            .set_timestamp(300);
        true
    }));
    let before = source.oplog_vv();
    text.unmark(1, 2, "bold", PosType::Utf16).unwrap();
    list.delete(0, 1).unwrap();
    movable.set(0, "z").unwrap();
    movable.mov(0, 1).unwrap();
    movable.delete(0, 1).unwrap();
    tree.mov(b, TreeParentId::Node(a)).unwrap();
    source.get_map("map").insert("n", 42).unwrap();
    source.commit_with(
        CommitOptions::new()
            .timestamp(200)
            .commit_msg("from caller"),
    );
    let start = before[&source.peer_id()];
    let span = IdSpan::new(
        source.peer_id(),
        start,
        source.oplog_vv()[&source.peer_id()],
    );
    let update = check_update(&source, &replica, before, &updates);
    assert_eq!(
        LoroDoc::decode_import_blob_meta(&update, true)
            .unwrap()
            .start_timestamp,
        300
    );
    assert_eq!(replica.get_text("text").get_delta(), text.get_delta());
    let changes = replica.export_json_in_id_span(span);
    assert_eq!(changes[0].msg.as_deref(), Some("changed by hook"));
    assert_eq!(
        serde_json::to_value(replica.export_json_in_id_span(span)).unwrap(),
        serde_json::to_value(source.export_json_in_id_span(span)).unwrap()
    );

    #[cfg(feature = "counter")]
    {
        let before = source.oplog_vv();
        source.get_counter("counter").increment(3.0).unwrap();
        source.commit_then_renew();
        check_update(&source, &replica, before, &updates);
    }
}

#[test]
fn manual_commit_and_late_local_update_subscription_match_range_export() {
    let doc = LoroDoc::new();
    let replica = LoroDoc::new();
    let subscription = Arc::new(Mutex::new(None::<Subscription>));
    let updates = Arc::new(Mutex::new(Vec::new()));
    let doc_in_callback = doc.clone();
    let subscription_in_callback = subscription.clone();
    let inbox = updates.clone();
    let _observer = doc.subscribe_root(Arc::new(move |_| {
        let mut sub = subscription_in_callback.lock().unwrap();
        sub.get_or_insert_with(|| {
            let inbox = inbox.clone();
            doc_in_callback.subscribe_local_update(Box::new(move |bytes| {
                inbox.lock().unwrap().push(bytes.clone());
                true
            }))
        });
    }));

    for (index, text) in ["a", "b"].into_iter().enumerate() {
        let before = doc.oplog_vv();
        let mut txn = doc.txn().unwrap();
        txn.get_text("text")
            .insert_with_txn(&mut txn, index, text, PosType::Utf16)
            .unwrap();
        txn.commit().unwrap();
        check_update(&doc, &replica, before, &updates);
    }
}

#[test]
fn backward_deletes_keep_the_stored_direction_when_commits_merge() {
    let doc = LoroDoc::new_auto_commit();
    let replica = LoroDoc::new_auto_commit();
    let text = doc.get_text("text");
    let updates = Arc::new(Mutex::new(Vec::new()));
    let inbox = updates.clone();
    let _subscription = doc.subscribe_local_update(Box::new(move |bytes| {
        inbox.lock().unwrap().push(bytes.clone());
        true
    }));

    let before = doc.oplog_vv();
    text.insert_utf16(0, "abc😀").unwrap();
    doc.commit_then_renew();
    check_update(&doc, &replica, before, &updates);
    for pos in [2, 1, 0] {
        let before = doc.oplog_vv();
        text.delete(pos, 1, PosType::Utf16).unwrap();
        doc.commit_then_renew();
        check_update(&doc, &replica, before, &updates);
    }
}

#[test]
fn singleton_columns_match_range_export_across_container_ops() {
    let doc = LoroDoc::new_auto_commit();
    let replica = LoroDoc::new_auto_commit();
    doc.set_peer_id(u64::MAX - 1).unwrap();
    let updates = Arc::new(Mutex::new(Vec::new()));
    let inbox = updates.clone();
    let _subscription = doc.subscribe_local_update(Box::new(move |bytes| {
        inbox.lock().unwrap().push(bytes.clone());
        true
    }));
    let text = doc.get_text("text");
    let list = doc.get_list("list");
    let movable = doc.get_movable_list("movable");
    let map = doc.get_map("map");
    let tree = doc.get_tree("tree");
    replica.get_text("text");
    replica.get_list("list");
    replica.get_movable_list("movable");
    replica.get_map("map");
    replica.get_tree("tree");
    #[cfg(feature = "counter")]
    for doc in [&doc, &replica] {
        doc.get_counter("counter");
    }

    let checkpoint = |edit: &mut dyn FnMut(), timestamp| {
        let before = doc.oplog_vv();
        edit();
        doc.commit_with(CommitOptions::new().timestamp(timestamp).commit_msg("备注"));
        check_update(&doc, &replica, before, &updates);
    };
    checkpoint(&mut || text.insert_utf16(0, "中😀a").unwrap(), 1);
    checkpoint(&mut || text.delete(0, 1, PosType::Utf16).unwrap(), 2);
    checkpoint(&mut || list.insert(0, "单").unwrap(), 3);
    checkpoint(&mut || list.delete(0, 1).unwrap(), 4);
    checkpoint(&mut || movable.insert(0, "a").unwrap(), 5);
    checkpoint(&mut || movable.insert(1, "b").unwrap(), 6);
    checkpoint(&mut || movable.set(1, "c").unwrap(), 7);
    checkpoint(&mut || movable.mov(0, 1).unwrap(), 8);
    checkpoint(&mut || movable.delete(0, 1).unwrap(), 9);
    checkpoint(&mut || map.insert("k", "v".repeat(150)).unwrap(), 10);
    checkpoint(&mut || map.delete("k").unwrap(), 11);
    checkpoint(
        &mut || {
            tree.create(TreeParentId::Root).unwrap();
        },
        12,
    );
    #[cfg(feature = "counter")]
    checkpoint(
        &mut || doc.get_counter("counter").increment(-3.5).unwrap(),
        13,
    );
}

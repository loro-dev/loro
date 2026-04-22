use std::sync::{Arc, Mutex};

use loro::{
    CommitOptions, ContainerTrait, ContainerType, EncodedBlobMode, EventTriggerKind, ExportMode,
    Frontiers, IdSpan, Index, LoroDoc, LoroList, LoroMovableList, LoroResult, LoroText, ToJson,
    TreeParentId, VersionVector, ID,
};
use pretty_assertions::assert_eq;

fn deep_json(doc: &LoroDoc) -> serde_json::Value {
    doc.get_deep_value().to_json_value()
}

fn make_lifecycle_doc(peer_id: u64) -> LoroResult<(LoroDoc, Frontiers)> {
    let doc = LoroDoc::new();
    doc.set_peer_id(peer_id)?;

    let root = doc.get_map("root");
    root.insert("title", "alpha")?;
    let body = root.insert_container("body", LoroText::new())?;
    body.insert(0, "hello")?;

    let items = root.insert_container("items", LoroList::new())?;
    items.push("one")?;
    items.push("two")?;

    let order = root.insert_container("order", LoroMovableList::new())?;
    order.push("draft")?;
    order.push("review")?;

    let tree = doc.get_tree("tree");
    tree.enable_fractional_index(0);
    let root_node = tree.create(TreeParentId::Root)?;
    tree.get_meta(root_node)?.insert("name", "root")?;

    doc.commit();
    let frontiers = doc.state_frontiers();
    Ok((doc, frontiers))
}

#[test]
fn peer_commit_options_and_event_origin_follow_contract() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    let peer_changes = Arc::new(Mutex::new(Vec::<ID>::new()));
    let peer_changes_clone = Arc::clone(&peer_changes);
    let _peer_sub = doc.subscribe_peer_id_change(Box::new(move |next_id| {
        peer_changes_clone.lock().unwrap().push(*next_id);
        true
    }));

    let events = Arc::new(Mutex::new(Vec::<(EventTriggerKind, String)>::new()));
    let events_clone = Arc::clone(&events);
    let _root_sub = doc.subscribe_root(Arc::new(move |event| {
        events_clone
            .lock()
            .unwrap()
            .push((event.triggered_by, event.origin.to_string()));
    }));

    doc.set_peer_id(17)?;
    doc.set_peer_id(18)?;
    assert_eq!(
        peer_changes
            .lock()
            .unwrap()
            .iter()
            .map(|id| id.peer)
            .collect::<Vec<_>>(),
        vec![17, 18]
    );

    let text = doc.get_text("text");

    doc.set_next_commit_options(
        CommitOptions::new()
            .origin("ui")
            .timestamp(123)
            .commit_msg("insert-a"),
    );
    text.insert(0, "a")?;
    doc.commit();

    let first_frontiers = doc.state_frontiers();
    let first_change = doc
        .get_change(first_frontiers.as_single().expect("single frontier"))
        .expect("first change should exist");
    assert_eq!(first_change.message(), "insert-a");
    assert_eq!(first_change.timestamp(), 123);

    doc.set_next_commit_options(
        CommitOptions::new()
            .origin("discard-me")
            .timestamp(456)
            .commit_msg("discard-me"),
    );
    doc.commit();

    text.insert(1, "b")?;
    doc.commit();

    let latest_frontiers = doc.state_frontiers();
    let latest_change = doc
        .get_change(latest_frontiers.as_single().expect("single frontier"))
        .expect("latest change should exist");
    assert_eq!(latest_change.message(), "");
    assert_ne!(latest_change.timestamp(), 456);

    assert_eq!(
        events.lock().unwrap().as_slice(),
        &[
            (EventTriggerKind::Local, "ui".to_string()),
            (EventTriggerKind::Local, "".to_string()),
        ]
    );
    assert_eq!(doc.get_pending_txn_len(), 0);

    Ok(())
}

#[test]
fn checkout_detach_attach_fork_paths_and_frontiers_follow_contract() -> anyhow::Result<()> {
    let (doc, v1) = make_lifecycle_doc(31)?;
    let expected_v1_doc = doc.fork_at(&v1)?;
    let expected_v1_json = deep_json(&expected_v1_doc);

    let body_id = doc
        .get_map("root")
        .get("body")
        .unwrap()
        .into_container()
        .unwrap()
        .id();
    let body_container = doc
        .get_container(body_id.clone())
        .expect("body should exist");
    assert_eq!(body_container.get_type(), ContainerType::Text);
    assert_eq!(
        doc.get_by_path(&[Index::Key("root".into()), Index::Key("body".into())])
            .expect("path should resolve")
            .into_container()
            .expect("path should resolve to container")
            .get_type(),
        ContainerType::Text
    );
    assert_eq!(
        doc.get_by_str_path("root/body")
            .expect("string path should resolve")
            .into_container()
            .expect("string path should resolve to container")
            .get_type(),
        ContainerType::Text
    );

    let body_path = doc
        .get_path_to_container(&body_id)
        .expect("nested body should have a path");
    assert_eq!(
        body_path.last().map(|(_, index)| index),
        Some(&Index::Key("body".into()))
    );

    let foreign = LoroDoc::new();
    foreign.set_peer_id(32)?;
    foreign.get_text("foreign").insert(0, "x")?;
    foreign.commit();
    assert!(doc.frontiers_to_vv(&foreign.state_frontiers()).is_none());
    assert_eq!(
        doc.minimize_frontiers(&foreign.state_frontiers())
            .expect("foreign frontiers should remain unchanged"),
        foreign.state_frontiers()
    );
    assert!(doc.checkout(&foreign.state_frontiers()).is_err());

    let vv = doc
        .frontiers_to_vv(&v1)
        .expect("frontiers should be in doc");
    assert_eq!(doc.vv_to_frontiers(&vv), v1);
    assert_eq!(doc.minimize_frontiers(&v1).expect("minimizable"), v1);

    let forked = doc.fork();
    assert_eq!(deep_json(&forked), deep_json(&doc));

    let fork_at = doc.fork_at(&v1)?;
    assert_eq!(deep_json(&fork_at), expected_v1_json);
    assert_eq!(fork_at.state_frontiers(), v1);

    doc.detach();
    assert!(doc.is_detached());
    doc.attach();
    assert!(!doc.is_detached());

    let body_text =
        LoroText::try_from_container(body_container.clone()).expect("body should be text");
    body_text.insert(5, "!")?;
    doc.commit();
    let v2 = doc.state_frontiers();
    assert_eq!(doc.frontiers_to_vv(&v2).unwrap(), doc.oplog_vv());

    doc.checkout(&v1)?;
    assert!(doc.is_detached());
    doc.set_detached_editing(true);
    assert!(doc.is_detached_editing_enabled());
    assert_ne!(doc.peer_id(), 31);

    body_text.insert(0, "detached-")?;
    doc.attach();
    assert!(!doc.is_detached());
    assert_eq!(body_text.to_string(), "detached-hello!");

    let after_attach = doc.state_frontiers();
    assert!(doc.frontiers_to_vv(&after_attach).is_some());
    assert_eq!(
        doc.vv_to_frontiers(&doc.frontiers_to_vv(&after_attach).unwrap()),
        after_attach
    );

    Ok(())
}

#[test]
fn storage_blobs_json_updates_and_batch_import_follow_contract() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(41)?;
    doc.set_change_merge_interval(0);

    let root = doc.get_map("root");
    root.insert("title", "alpha")?;
    let body = root.insert_container("body", LoroText::new())?;
    body.insert(0, "Hello")?;
    let items = root.insert_container("items", LoroList::new())?;
    items.push("seed")?;
    let tree = doc.get_tree("tree");
    tree.enable_fractional_index(0);
    let tree_root = tree.create(TreeParentId::Root)?;
    tree.get_meta(tree_root)?.insert("kind", "root")?;
    doc.commit_with(CommitOptions::default().timestamp(11));
    let v1 = doc.state_frontiers();

    body.insert(body.len_unicode(), " world")?;
    items.push("branch")?;
    tree.get_meta(tree_root)?.insert("note", "updated")?;
    doc.commit_with(CommitOptions::default().timestamp(22));
    let v2 = doc.state_frontiers();
    assert_eq!(doc.frontiers_to_vv(&v2).unwrap(), doc.oplog_vv());
    let expected_v1_doc = doc.fork_at(&v1)?;
    let expected_v1_json = deep_json(&expected_v1_doc);

    let snapshot = doc.export(ExportMode::Snapshot)?;
    let snapshot_meta = LoroDoc::decode_import_blob_meta(&snapshot, true)?;
    assert_eq!(snapshot_meta.mode, EncodedBlobMode::Snapshot);
    assert!(snapshot_meta.start_frontiers.is_empty());

    let shallow = doc.export(ExportMode::shallow_snapshot(&v1))?;
    let shallow_meta = LoroDoc::decode_import_blob_meta(&shallow, true)?;
    assert_eq!(shallow_meta.mode, EncodedBlobMode::ShallowSnapshot);
    assert_eq!(shallow_meta.start_frontiers, v1);
    assert_eq!(shallow_meta.start_timestamp, 11);

    let state_only = doc.export(ExportMode::state_only(Some(&v1)))?;
    let state_only_meta = LoroDoc::decode_import_blob_meta(&state_only, true)?;
    assert_eq!(state_only_meta.mode, EncodedBlobMode::ShallowSnapshot);
    assert_eq!(state_only_meta.start_frontiers, v1);

    let snapshot_at = doc.export(ExportMode::snapshot_at(&v1))?;
    let snapshot_at_meta = LoroDoc::decode_import_blob_meta(&snapshot_at, true)?;
    assert_eq!(snapshot_at_meta.mode, EncodedBlobMode::Snapshot);
    assert!(snapshot_at_meta.start_frontiers.is_empty());

    let updates = doc.export(ExportMode::updates(&VersionVector::default()))?;
    let updates_meta = LoroDoc::decode_import_blob_meta(&updates, true)?;
    assert_eq!(updates_meta.mode, EncodedBlobMode::Updates);
    assert_eq!(updates_meta.partial_end_vv, doc.oplog_vv());

    let peer = doc.peer_id();
    let end_counter = *doc.oplog_vv().get(&peer).expect("peer should exist");
    let updates_range = doc.export(ExportMode::updates_in_range(vec![IdSpan::new(
        peer,
        0,
        end_counter,
    )]))?;
    let updates_range_meta = LoroDoc::decode_import_blob_meta(&updates_range, true)?;
    assert_eq!(updates_range_meta.mode, EncodedBlobMode::Updates);
    assert_eq!(updates_range_meta.partial_end_vv, doc.oplog_vv());

    let snapshot_doc = LoroDoc::from_snapshot(&snapshot)?;
    assert_eq!(deep_json(&snapshot_doc), deep_json(&doc));

    let shallow_doc = LoroDoc::new();
    shallow_doc.import(&shallow)?;
    assert!(shallow_doc.is_shallow());
    assert_eq!(shallow_doc.shallow_since_frontiers(), v1);

    let state_only_doc = LoroDoc::new();
    state_only_doc.import(&state_only)?;
    assert_eq!(deep_json(&state_only_doc), expected_v1_json);

    let snapshot_at_doc = LoroDoc::new();
    snapshot_at_doc.import(&snapshot_at)?;
    assert_eq!(deep_json(&snapshot_at_doc), expected_v1_json);

    let updates_doc = LoroDoc::new();
    updates_doc.import(&updates)?;
    assert_eq!(deep_json(&updates_doc), deep_json(&doc));

    let updates_range_doc = LoroDoc::new();
    updates_range_doc.import(&updates_range)?;
    assert_eq!(deep_json(&updates_range_doc), deep_json(&doc));

    let start = VersionVector::default();
    let end = doc.oplog_vv();
    let compressed = doc.export_json_updates(&start, &end);
    let uncompressed = doc.export_json_updates_without_peer_compression(&start, &end);
    assert!(compressed.peers.is_some());
    assert!(uncompressed.peers.is_none());

    let compressed_json = serde_json::to_string(&compressed)?;
    let parsed_from_str: loro::JsonSchema = compressed_json.as_str().try_into()?;
    assert_eq!(parsed_from_str.changes.len(), compressed.changes.len());

    let compressed_doc = LoroDoc::new();
    compressed_doc.import_json_updates(compressed.clone())?;
    assert_eq!(deep_json(&compressed_doc), deep_json(&doc));

    let compressed_doc_from_string = LoroDoc::new();
    compressed_doc_from_string.import_json_updates(compressed_json.clone())?;
    assert_eq!(deep_json(&compressed_doc_from_string), deep_json(&doc));

    let uncompressed_json = serde_json::to_string(&uncompressed)?;
    let uncompressed_doc = LoroDoc::new();
    uncompressed_doc.import_json_updates(uncompressed.clone())?;
    assert_eq!(deep_json(&uncompressed_doc), deep_json(&doc));

    let uncompressed_doc_from_string = LoroDoc::new();
    uncompressed_doc_from_string.import_json_updates(uncompressed_json.clone())?;
    assert_eq!(deep_json(&uncompressed_doc_from_string), deep_json(&doc));

    let doc_1 = LoroDoc::new();
    doc_1.set_peer_id(1)?;
    doc_1.set_change_merge_interval(0);
    doc_1.get_text("text").insert(0, "Hello world!")?;
    doc_1.commit();

    let doc_2 = LoroDoc::new();
    doc_2.set_peer_id(2)?;
    doc_2.set_change_merge_interval(0);
    doc_2.get_text("text").insert(0, "Hello world!")?;
    doc_2.commit();

    let blob11 = doc_1.export(ExportMode::updates_in_range(vec![IdSpan::new(1, 0, 5)]))?;
    let blob12 = doc_1.export(ExportMode::updates_in_range(vec![IdSpan::new(1, 5, 7)]))?;
    let blob13 = doc_1.export(ExportMode::updates_in_range(vec![IdSpan::new(1, 6, 12)]))?;

    let blob21 = doc_2.export(ExportMode::updates_in_range(vec![IdSpan::new(2, 0, 5)]))?;
    let blob22 = doc_2.export(ExportMode::updates_in_range(vec![IdSpan::new(2, 5, 6)]))?;
    let blob23 = doc_2.export(ExportMode::updates_in_range(vec![IdSpan::new(2, 6, 12)]))?;

    let batch_doc = LoroDoc::new();
    let status = batch_doc.import_batch(&[
        blob11.clone(),
        blob13.clone(),
        blob21.clone(),
        blob23.clone(),
    ])?;
    assert_eq!(status.success.get(&1), Some(&(0, 5)));
    assert_eq!(status.success.get(&2), Some(&(0, 5)));
    let pending = status.pending.expect("expected pending ranges");
    assert_eq!(pending.get(&1), Some(&(6, 12)));
    assert_eq!(pending.get(&2), Some(&(6, 12)));

    let status = batch_doc.import_batch(&[blob12.clone(), blob22.clone()])?;
    assert!(status.pending.is_none());
    assert_eq!(
        batch_doc.get_text("text").to_string(),
        "Hello world!Hello world!"
    );

    Ok(())
}

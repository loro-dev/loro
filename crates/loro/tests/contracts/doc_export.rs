use std::sync::{Arc, Mutex};

use loro::{
    CommitOptions, ExportMode, IdSpan, Index, LoroDoc, LoroList, LoroText, Timestamp, ToJson,
    TreeParentId, VersionVector, ID,
};
use pretty_assertions::assert_eq;
use serde_json::Value;

fn deep_json(doc: &LoroDoc) -> Value {
    doc.get_deep_value().to_json_value()
}

#[test]
fn commit_metadata_empty_commit_and_json_updates_roundtrip_follow_contract() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(41)?;
    doc.set_change_merge_interval(0);

    let pre_commit_payloads = Arc::new(Mutex::new(Vec::<(String, String, Timestamp)>::new()));
    let pre_commit_payloads_clone = Arc::clone(&pre_commit_payloads);
    let _pre_commit = doc.subscribe_pre_commit(Box::new(move |payload| {
        pre_commit_payloads_clone.lock().unwrap().push((
            payload.origin.clone(),
            payload.change_meta.message().to_owned(),
            payload.change_meta.timestamp(),
        ));
        true
    }));

    let root_origins = Arc::new(Mutex::new(Vec::<String>::new()));
    let root_origins_clone = Arc::clone(&root_origins);
    let _root_sub = doc.subscribe_root(Arc::new(move |event| {
        root_origins_clone
            .lock()
            .unwrap()
            .push(event.origin.to_owned());
    }));

    doc.set_next_commit_options(
        CommitOptions::new()
            .origin("discard")
            .commit_msg("discard")
            .timestamp(11),
    );
    doc.clear_next_commit_options();
    doc.set_next_commit_message("insert-a");
    doc.set_next_commit_timestamp(123);
    doc.set_next_commit_origin("ui");
    doc.get_map("root").insert("title", "Spec")?;
    doc.commit();

    let first_change = doc.get_change(ID::new(41, 0)).unwrap();
    assert_eq!(first_change.message(), "insert-a");
    assert_eq!(first_change.timestamp(), 123);
    assert_eq!(
        pre_commit_payloads.lock().unwrap().as_slice(),
        &[("ui".to_string(), "insert-a".to_string(), 123)]
    );
    assert_eq!(root_origins.lock().unwrap().as_slice(), &["ui".to_string()]);
    doc.set_next_commit_message("to-be-cleared");
    doc.set_next_commit_message("");
    doc.set_next_commit_timestamp(456);
    doc.set_next_commit_origin("discard");
    doc.commit();

    doc.get_map("root").insert("body", "Hello")?;
    doc.commit();

    let second_change = doc.get_change(ID::new(41, 1)).unwrap();
    assert_eq!(second_change.message(), "");
    assert!(second_change.timestamp() >= 123);
    assert_eq!(
        pre_commit_payloads.lock().unwrap().as_slice(),
        &[
            ("ui".to_string(), "insert-a".to_string(), 123),
            ("".to_string(), "".to_string(), 123)
        ]
    );
    assert_eq!(
        root_origins.lock().unwrap().as_slice(),
        &["ui".to_string(), "".to_string()]
    );

    doc.set_next_commit_options(
        CommitOptions::new()
            .origin("barrier")
            .commit_msg("carry")
            .timestamp(789),
    );
    let _snapshot = doc.export(ExportMode::Snapshot)?;

    doc.get_map("root").insert("footer", "World")?;
    doc.commit();

    let third_change = doc.get_change(ID::new(41, 2)).unwrap();
    assert_eq!(third_change.message(), "carry");
    assert_eq!(third_change.timestamp(), 789);
    assert_eq!(
        pre_commit_payloads.lock().unwrap().as_slice(),
        &[
            ("ui".to_string(), "insert-a".to_string(), 123),
            ("".to_string(), "".to_string(), 123),
            ("barrier".to_string(), "carry".to_string(), 789),
        ]
    );
    assert_eq!(
        root_origins.lock().unwrap().as_slice(),
        &["ui".to_string(), "".to_string(), "barrier".to_string()]
    );

    doc.set_record_timestamp(true);
    doc.get_map("root").insert("recorded", true)?;
    let _ = doc.commit_with(
        CommitOptions::new()
            .origin("manual")
            .commit_msg("recorded")
            .timestamp(1000),
    );

    let fourth_change = doc.get_change(ID::new(41, 3)).unwrap();
    assert_eq!(fourth_change.message(), "recorded");
    assert_eq!(fourth_change.timestamp(), 1000);
    assert_eq!(
        root_origins.lock().unwrap().as_slice(),
        &[
            "ui".to_string(),
            "".to_string(),
            "barrier".to_string(),
            "manual".to_string()
        ]
    );

    let start = VersionVector::default();
    let end = doc.oplog_vv();
    let compressed = doc.export_json_updates(&start, &end);
    let uncompressed = doc.export_json_updates_without_peer_compression(&start, &end);
    assert!(compressed.peers.is_some());
    assert!(uncompressed.peers.is_none());

    let compressed_json = serde_json::to_string(&compressed)?;
    let uncompressed_json = serde_json::to_string(&uncompressed)?;
    let compressed_schema: loro::JsonSchema = compressed_json.as_str().try_into()?;
    let uncompressed_schema: loro::JsonSchema = uncompressed_json.as_str().try_into()?;
    assert_eq!(compressed_schema.changes.len(), compressed.changes.len());
    assert_eq!(
        uncompressed_schema.changes.len(),
        uncompressed.changes.len()
    );

    let compressed_doc = LoroDoc::new();
    compressed_doc.import_json_updates(compressed.clone())?;
    assert_eq!(deep_json(&compressed_doc), deep_json(&doc));
    assert_eq!(
        compressed_doc.get_change(ID::new(41, 0)).unwrap().message(),
        "insert-a"
    );
    assert_eq!(
        compressed_doc
            .get_change(ID::new(41, 3))
            .unwrap()
            .timestamp(),
        1000
    );

    let compressed_doc_from_str = LoroDoc::new();
    compressed_doc_from_str.import_json_updates(compressed_json.as_str())?;
    assert_eq!(deep_json(&compressed_doc_from_str), deep_json(&doc));
    assert_eq!(
        compressed_doc_from_str
            .get_change(ID::new(41, 2))
            .unwrap()
            .message(),
        "carry"
    );

    let uncompressed_doc = LoroDoc::new();
    uncompressed_doc.import_json_updates(uncompressed.clone())?;
    assert_eq!(deep_json(&uncompressed_doc), deep_json(&doc));
    assert_eq!(
        uncompressed_doc
            .get_change(ID::new(41, 1))
            .unwrap()
            .message(),
        ""
    );

    let uncompressed_doc_from_str = LoroDoc::new();
    uncompressed_doc_from_str.import_json_updates(uncompressed_json.as_str())?;
    assert_eq!(deep_json(&uncompressed_doc_from_str), deep_json(&doc));
    assert_eq!(
        uncompressed_doc_from_str
            .get_change(ID::new(41, 0))
            .unwrap()
            .timestamp(),
        123
    );

    Ok(())
}

#[test]
fn checkout_diff_apply_diff_and_path_queries_follow_contract() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(52)?;
    doc.set_change_merge_interval(0);

    let project = doc.get_map("project");
    project.insert("title", "Spec")?;

    let body = project.insert_container("body", LoroText::new())?;
    body.insert(0, "Hello")?;

    let tags = project.insert_container("tags", LoroList::new())?;
    tags.push("alpha")?;

    let tree = doc.get_tree("tree");
    tree.enable_fractional_index(0);
    let root = tree.create(TreeParentId::Root)?;
    let child = tree.create_at(root, 0)?;
    tree.get_meta(root)?.insert("label", "root")?;
    tree.get_meta(child)?.insert("label", "child")?;

    doc.commit();
    let v1 = doc.state_frontiers();
    let expected_v1 = deep_json(&doc);

    assert_eq!(
        doc.get_by_str_path("project/title")
            .unwrap()
            .get_deep_value(),
        "Spec".into()
    );
    assert_eq!(
        doc.get_by_path(&[Index::Key("project".into()), Index::Key("body".into())])
            .unwrap()
            .get_deep_value(),
        "Hello".into()
    );
    assert_eq!(
        doc.get_by_str_path(&format!("tree/{root}/label"))
            .unwrap()
            .get_deep_value(),
        "root".into()
    );
    assert_eq!(
        doc.get_by_str_path(&format!("tree/{child}/label"))
            .unwrap()
            .get_deep_value(),
        "child".into()
    );

    body.insert(body.len_unicode(), " world")?;
    project.insert("phase", "draft")?;
    doc.commit();
    let v2 = doc.state_frontiers();

    let diff = doc.diff(&v1, &v2)?;
    let snapshot_at_v1 = doc.export(ExportMode::snapshot_at(&v1))?;
    let patched = LoroDoc::new();
    patched.import(&snapshot_at_v1)?;
    patched.apply_diff(diff.clone())?;
    assert_eq!(deep_json(&patched), deep_json(&doc));

    let reverted = LoroDoc::new();
    reverted.import(&doc.export(ExportMode::Snapshot)?)?;
    reverted.revert_to(&v1)?;
    assert_eq!(deep_json(&reverted), expected_v1);

    let traveler = LoroDoc::new();
    traveler.import(&doc.export(ExportMode::Snapshot)?)?;
    traveler.checkout(&v1)?;
    assert!(traveler.is_detached());
    assert_eq!(deep_json(&traveler), expected_v1);
    traveler.checkout_to_latest();
    assert_eq!(deep_json(&traveler), deep_json(&doc));

    Ok(())
}

#[test]
fn state_only_shallow_since_and_updates_till_cover_export_boundaries() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(77)?;
    doc.set_change_merge_interval(0);

    let root = doc.get_map("root");
    let text = root.insert_container("text", LoroText::new())?;
    text.insert(0, "alpha")?;
    doc.commit();
    let v1 = doc.state_frontiers();
    let v1_json = deep_json(&doc);
    let v1_id = v1.as_single().expect("first commit should have a frontier");

    let list = root.insert_container("items", LoroList::new())?;
    list.insert(0, "one")?;
    list.insert(1, "two")?;
    let tree = doc.get_tree("outline");
    let parent = tree.create(TreeParentId::Root)?;
    tree.create(parent)?;
    doc.commit();

    root.insert("done", true)?;
    text.insert(text.len_unicode(), " beta")?;
    doc.commit();
    let latest_json = deep_json(&doc);

    let v1_vv = doc
        .frontiers_to_vv(&v1)
        .ok_or_else(|| anyhow::anyhow!("v1 frontiers should resolve to a version vector"))?;
    let till_v1 = doc.export(ExportMode::updates_till(&v1_vv))?;
    let till_v1_meta = LoroDoc::decode_import_blob_meta(&till_v1, true)?;
    assert_eq!(till_v1_meta.partial_end_vv, v1_vv);
    let till_v1_doc = LoroDoc::new();
    till_v1_doc.import(&till_v1)?;
    assert_eq!(deep_json(&till_v1_doc), v1_json);

    let latest_state_only = doc.export(ExportMode::state_only(None))?;
    let latest_state_only_doc = LoroDoc::new();
    latest_state_only_doc.import(&latest_state_only)?;
    assert!(latest_state_only_doc.is_shallow());
    assert_eq!(deep_json(&latest_state_only_doc), latest_json);

    let owned_shallow = doc.export(ExportMode::shallow_snapshot_owned(v1.clone()))?;
    let owned_shallow_doc = LoroDoc::new();
    owned_shallow_doc.import(&owned_shallow)?;
    assert!(owned_shallow_doc.is_shallow());
    assert_eq!(owned_shallow_doc.shallow_since_frontiers(), v1);

    let since_id = doc.export(ExportMode::shallow_snapshot_since(v1_id))?;
    let since_id_doc = LoroDoc::new();
    since_id_doc.import(&since_id)?;
    assert_eq!(since_id_doc.shallow_since_frontiers(), v1);
    assert_eq!(deep_json(&since_id_doc), latest_json);

    let empty_range = doc.export(ExportMode::updates_in_range(Vec::<IdSpan>::new()))?;
    let empty_range_meta = LoroDoc::decode_import_blob_meta(&empty_range, true)?;
    assert!(empty_range_meta.partial_start_vv.is_empty());
    assert!(empty_range_meta.partial_end_vv.is_empty());
    let empty_doc = LoroDoc::new();
    empty_doc.import(&empty_range)?;
    assert_eq!(deep_json(&empty_doc), serde_json::json!({}));

    Ok(())
}

#[test]
fn import_batch_reports_pending_until_dependencies_arrive() -> anyhow::Result<()> {
    let source = LoroDoc::new();
    source.set_peer_id(63)?;
    source.set_change_merge_interval(0);

    let text = source.get_text("text");
    text.insert(0, "a")?;
    source.commit();
    text.insert(1, "b")?;
    source.commit();
    text.insert(2, "c")?;
    source.commit();

    let peer = source.peer_id();
    let b0 = source.export(ExportMode::updates_in_range(vec![IdSpan::new(peer, 0, 1)]))?;
    let b1 = source.export(ExportMode::updates_in_range(vec![IdSpan::new(peer, 1, 2)]))?;
    let b2 = source.export(ExportMode::updates_in_range(vec![IdSpan::new(peer, 2, 3)]))?;

    let replay = LoroDoc::new();
    let first = replay.import_batch(&[b2.clone()])?;
    assert_eq!(
        first.pending.as_ref().and_then(|p| p.get(&peer).copied()),
        Some((2, 3))
    );
    assert_eq!(replay.get_text("text").to_string(), "");

    let second = replay.import_batch(&[b0.clone(), b1.clone()])?;
    assert!(second.pending.is_none());
    assert_eq!(replay.get_text("text").to_string(), "abc");

    Ok(())
}

#[test]
fn import_with_origin_duplicate_updates_and_batch_order_follow_contract() -> anyhow::Result<()> {
    let source = LoroDoc::new();
    source.set_peer_id(91)?;
    source.set_change_merge_interval(0);

    let root = source.get_map("root");
    root.insert("title", "draft")?;
    source.commit();
    let first_vv = source.oplog_vv();
    let first_updates = source.export(ExportMode::all_updates())?;

    root.insert("title", "published")?;
    root.insert("done", true)?;
    source.commit();
    let second_updates = source.export(ExportMode::updates(&first_vv))?;

    let target = LoroDoc::new();
    let origins = Arc::new(Mutex::new(Vec::<String>::new()));
    let origins_clone = Arc::clone(&origins);
    let _sub = target.subscribe_root(Arc::new(move |event| {
        origins_clone.lock().unwrap().push(event.origin.to_string());
    }));

    let pending = target.import_batch(&[second_updates.clone(), first_updates.clone()])?;
    assert!(pending.pending.is_none());
    assert_eq!(
        target.get_deep_value().to_json_value(),
        serde_json::json!({"root": {"title": "published", "done": true}})
    );
    assert_eq!(
        origins.lock().unwrap().as_slice(),
        &["checkout".to_string()]
    );

    let duplicate = target.import_with(&first_updates, "sync:duplicate")?;
    assert!(duplicate.success.is_empty());
    assert!(duplicate.pending.is_none());
    assert_eq!(
        origins.lock().unwrap().as_slice(),
        &["checkout".to_string()]
    );

    let fresh = LoroDoc::new();
    let fresh_origins = Arc::new(Mutex::new(Vec::<String>::new()));
    let fresh_origins_clone = Arc::clone(&fresh_origins);
    let _sub = fresh.subscribe_root(Arc::new(move |event| {
        fresh_origins_clone
            .lock()
            .unwrap()
            .push(event.origin.to_string());
    }));
    let status = fresh.import_with(&source.export(ExportMode::all_updates())?, "sync:full")?;
    assert!(status.pending.is_none());
    assert!(!status.success.is_empty());
    assert_eq!(deep_json(&fresh), deep_json(&source));
    assert_eq!(
        fresh_origins.lock().unwrap().as_slice(),
        &["sync:full".to_string()]
    );

    Ok(())
}

#[test]
fn state_only_snapshot_at_updates_till_and_shallow_since_roundtrip() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(88)?;
    doc.set_change_merge_interval(0);

    let root = doc.get_map("root");
    root.insert("phase", "draft")?;
    let text = root.insert_container("text", LoroText::new())?;
    text.insert(0, "one")?;
    doc.commit();
    let v1 = doc.state_frontiers();
    let vv1 = doc.oplog_vv();
    let state_v1 = deep_json(&doc);

    text.insert(text.len_unicode(), " two")?;
    let items = root.insert_container("items", LoroList::new())?;
    items.push(1)?;
    doc.commit();
    let v2 = doc.state_frontiers();
    let vv2 = doc.oplog_vv();
    let state_v2 = deep_json(&doc);

    root.insert("done", true)?;
    doc.commit();
    let latest = deep_json(&doc);

    let state_only_latest = LoroDoc::new();
    state_only_latest.import(&doc.export(ExportMode::state_only(None))?)?;
    assert!(state_only_latest.is_shallow());
    assert_eq!(deep_json(&state_only_latest), latest);

    let state_only_v2 = LoroDoc::new();
    state_only_v2.import(&doc.export(ExportMode::state_only(Some(&v2)))?)?;
    assert!(state_only_v2.is_shallow());
    assert_eq!(deep_json(&state_only_v2), state_v2);
    assert!(state_only_v2.checkout(&v1).is_err());

    let snapshot_at_v1 = LoroDoc::new();
    snapshot_at_v1.import(&doc.export(ExportMode::snapshot_at(&v1))?)?;
    assert!(!snapshot_at_v1.is_shallow());
    assert_eq!(deep_json(&snapshot_at_v1), state_v1);
    assert_eq!(snapshot_at_v1.oplog_vv(), vv1);

    let updates_till_v2 = LoroDoc::new();
    updates_till_v2.import(&doc.export(ExportMode::updates_till(&vv2))?)?;
    assert_eq!(deep_json(&updates_till_v2), state_v2);
    assert_eq!(updates_till_v2.oplog_vv(), vv2);
    updates_till_v2.import(&doc.export(ExportMode::updates(&vv2))?)?;
    assert_eq!(deep_json(&updates_till_v2), latest);

    let empty_updates = LoroDoc::new();
    empty_updates.import(&doc.export(ExportMode::updates_till(&VersionVector::default()))?)?;
    assert_eq!(deep_json(&empty_updates), serde_json::json!({}));

    let shallow_since_first = LoroDoc::new();
    shallow_since_first.import(&doc.export(ExportMode::shallow_snapshot_since(ID::new(88, 0)))?)?;
    assert!(shallow_since_first.is_shallow());
    assert_eq!(deep_json(&shallow_since_first), latest);
    assert!(shallow_since_first.shallow_since_vv().get(&88).is_some());

    Ok(())
}

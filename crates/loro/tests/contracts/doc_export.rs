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

use loro::{
    cursor::{CannotFindRelativePosition, Side},
    EncodedBlobMode, ExportMode, IdSpan, LoroDoc, LoroError, ToJson,
};
use pretty_assertions::assert_eq;
use serde_json::json;

fn deep_json(doc: &LoroDoc) -> serde_json::Value {
    doc.get_deep_value().to_json_value()
}

#[test]
fn fork_and_detached_editing_follow_contract() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(1)?;

    let text = doc.get_text("text");
    text.insert(0, "hello")?;
    doc.commit();

    let forked = doc.fork();
    assert_eq!(deep_json(&forked), deep_json(&doc));

    let first_frontiers = doc.state_frontiers();
    let fork_at = doc.fork_at(&first_frontiers)?;
    assert_eq!(fork_at.get_text("text").to_string(), "hello");

    doc.detach();
    assert!(doc.is_detached());

    doc.set_detached_editing(true);
    assert_ne!(doc.peer_id(), 1);
    text.insert(0, "X")?;
    doc.attach();
    assert!(!doc.is_detached());
    assert_eq!(doc.get_text("text").to_string(), "Xhello");

    Ok(())
}

#[test]
fn checkout_and_history_cache_follow_contract() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(2)?;

    let map = doc.get_map("meta");
    map.insert("a", 1)?;
    doc.commit();

    let old_frontiers = doc.state_frontiers();
    map.insert("b", 2)?;
    doc.commit();
    map.insert("c", 3)?;
    doc.commit();

    assert!(!doc.has_history_cache());

    doc.checkout(&old_frontiers)?;
    assert!(doc.is_detached());
    assert!(doc.has_history_cache());
    assert_eq!(deep_json(&doc), json!({"meta": {"a": 1}}));

    match doc.get_map("meta").insert("z", 9) {
        Err(LoroError::EditWhenDetached | LoroError::AutoCommitNotStarted) => {}
        Err(e) => panic!("unexpected error while detached: {e}"),
        Ok(_) => panic!("editing detached doc should fail"),
    }

    doc.free_history_cache();
    assert!(!doc.has_history_cache());

    doc.attach();
    assert!(!doc.is_detached());
    assert_eq!(deep_json(&doc), json!({"meta": {"a": 1, "b": 2, "c": 3}}));

    Ok(())
}

#[test]
fn shallow_snapshot_cursor_and_meta_follow_contract() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(3)?;

    let text = doc.get_text("text");
    text.insert(0, "01234")?;
    doc.commit();

    let first_frontiers = doc.state_frontiers();
    let cursor = text
        .get_cursor(5, Side::Middle)
        .expect("cursor should exist");

    text.insert(0, "01234")?;
    doc.commit();
    assert_eq!(doc.get_cursor_pos(&cursor)?.current.pos, 10);

    text.delete(0, 10)?;
    doc.commit();
    assert_eq!(doc.get_cursor_pos(&cursor)?.current.pos, 0);

    let fork_at = doc.fork_at(&first_frontiers)?;
    assert_eq!(fork_at.get_text("text").to_string(), "01234");

    let shallow = doc.export(ExportMode::shallow_snapshot(&first_frontiers))?;
    let meta = LoroDoc::decode_import_blob_meta(&shallow, false)?;
    assert!(meta.mode.is_snapshot());
    assert_eq!(meta.mode, EncodedBlobMode::ShallowSnapshot);
    assert_eq!(meta.start_frontiers, first_frontiers);
    assert_eq!(meta.partial_end_vv, doc.oplog_vv());

    let shallow_doc = LoroDoc::new();
    shallow_doc.import(&shallow)?;
    assert!(shallow_doc.is_shallow());
    assert_eq!(shallow_doc.shallow_since_frontiers(), first_frontiers);
    assert_eq!(shallow_doc.shallow_since_vv().get(&3).copied(), Some(4));
    assert!(shallow_doc.len_ops() > 0);
    assert!(shallow_doc.len_changes() > 0);
    assert_eq!(shallow_doc.get_text("text").to_string(), "");

    let state_only = doc.export(ExportMode::state_only(Some(&first_frontiers)))?;
    let state_only_meta = LoroDoc::decode_import_blob_meta(&state_only, false)?;
    assert_eq!(state_only_meta.mode, EncodedBlobMode::ShallowSnapshot);
    assert_eq!(state_only_meta.start_frontiers, first_frontiers);
    let state_only_doc = LoroDoc::new();
    state_only_doc.import(&state_only)?;
    assert_eq!(deep_json(&state_only_doc), json!({"text": "01234"}));

    let snapshot_at = doc.export(ExportMode::snapshot_at(&first_frontiers))?;
    let snapshot_at_meta = LoroDoc::decode_import_blob_meta(&snapshot_at, false)?;
    assert_eq!(snapshot_at_meta.mode, EncodedBlobMode::Snapshot);
    assert!(snapshot_at_meta.start_frontiers.is_empty());
    assert_eq!(snapshot_at_meta.change_num, 1);
    let snapshot_at_doc = LoroDoc::new();
    snapshot_at_doc.import(&snapshot_at)?;
    assert_eq!(deep_json(&snapshot_at_doc), json!({"text": "01234"}));
    snapshot_at_doc.import(&doc.export(ExportMode::all_updates())?)?;
    assert_eq!(snapshot_at_doc.get_text("text").to_string(), "");

    assert_eq!(shallow_doc.get_cursor_pos(&cursor)?.current.pos, 0);

    let cleared = LoroDoc::new();
    cleared.set_peer_id(33)?;
    let cleared_text = cleared.get_text("text");
    cleared_text.insert(0, "Hello world")?;
    let cleared_cursor = cleared_text
        .get_cursor(3, Side::Left)
        .expect("cursor should exist");
    cleared_text.delete(0, 5)?;
    cleared.commit();
    let cleared_shallow =
        cleared.export(ExportMode::shallow_snapshot(&cleared.oplog_frontiers()))?;
    let cleared_doc = LoroDoc::new();
    cleared_doc.import(&cleared_shallow)?;
    let err = cleared_doc.get_cursor_pos(&cleared_cursor).unwrap_err();
    assert!(matches!(err, CannotFindRelativePosition::HistoryCleared));

    Ok(())
}

#[test]
fn updates_range_import_batch_and_compaction_follow_contract() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(4)?;
    doc.set_change_merge_interval(0);

    let text = doc.get_text("text");
    doc.set_next_commit_message("insert A");
    text.insert(0, "A")?;
    doc.commit();
    doc.set_next_commit_message("insert B");
    text.insert(1, "B")?;
    doc.commit();
    doc.set_next_commit_message("insert C");
    text.insert(2, "C")?;
    doc.commit();

    assert_eq!(doc.len_ops(), 3);
    assert_eq!(doc.len_changes(), 3);

    let first = doc.export(ExportMode::updates_in_range(vec![IdSpan::new(4, 0, 1)]))?;
    let second = doc.export(ExportMode::updates_in_range(vec![IdSpan::new(4, 1, 3)]))?;

    let first_meta = LoroDoc::decode_import_blob_meta(&first, false)?;
    assert_eq!(first_meta.mode, EncodedBlobMode::Updates);
    assert_eq!(first_meta.partial_start_vv.get(&4).copied(), Some(0));
    assert_eq!(first_meta.partial_end_vv.get(&4).copied(), Some(1));
    assert_eq!(first_meta.change_num, 1);

    let second_meta = LoroDoc::decode_import_blob_meta(&second, false)?;
    assert_eq!(second_meta.mode, EncodedBlobMode::Updates);
    assert_eq!(second_meta.partial_start_vv.get(&4).copied(), Some(1));
    assert_eq!(second_meta.partial_end_vv.get(&4).copied(), Some(3));
    assert_eq!(second_meta.change_num, 2);

    let batch_doc = LoroDoc::new();
    let status = batch_doc.import_batch(&[second.clone(), first.clone()])?;
    assert!(status.pending.is_none());
    assert_eq!(batch_doc.get_text("text").to_string(), "ABC");

    doc.compact_change_store();
    assert_eq!(doc.len_ops(), 3);
    assert_eq!(doc.len_changes(), 3);

    let snapshot = doc.export(ExportMode::Snapshot)?;
    let reloaded = LoroDoc::new();
    reloaded.import(&snapshot)?;
    assert_eq!(reloaded.len_ops(), 3);
    assert_eq!(reloaded.len_changes(), 3);
    assert_eq!(reloaded.get_text("text").to_string(), "ABC");
    assert_eq!(deep_json(&reloaded), json!({"text": "ABC"}));

    Ok(())
}

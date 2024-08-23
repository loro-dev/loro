use loro::{CommitOptions, LoroDoc, VersionVector, ID};

#[test]
fn test_commit_message() {
    let doc = LoroDoc::new();
    let text = doc.get_text("text");
    text.insert(0, "hello").unwrap();
    doc.commit_with(CommitOptions::new().commit_msg("edits"));
    let change = doc.get_change(ID::new(doc.peer_id(), 1)).unwrap();
    assert_eq!(change.message(), "edits");
}

#[test]
fn changes_with_commit_message_won_t_merge() {
    let doc = LoroDoc::new();
    let text = doc.get_text("text");

    text.insert(0, "hello").unwrap();
    doc.commit_with(CommitOptions::new().commit_msg("edit 1"));

    text.insert(5, " world").unwrap();
    doc.commit_with(CommitOptions::new().commit_msg("edit 2"));

    assert_eq!(text.to_string(), "hello world");

    let change1 = doc.get_change(ID::new(doc.peer_id(), 1)).unwrap();
    let change2 = doc.get_change(ID::new(doc.peer_id(), 6)).unwrap();

    assert_eq!(change1.message(), "edit 1");
    assert_eq!(change2.message(), "edit 2");
}

#[test]
fn test_syncing_commit_message() {
    let doc1 = LoroDoc::new();
    doc1.set_peer_id(1).unwrap();
    let text1 = doc1.get_text("text");

    text1.insert(0, "hello").unwrap();
    doc1.commit_with(CommitOptions::new().commit_msg("edit on doc1"));

    let doc2 = LoroDoc::new();
    doc2.set_peer_id(2).unwrap();

    // Export changes from doc1 and import to doc2
    let changes = doc1.export_from(&Default::default());
    doc2.import(&changes).unwrap();

    // Verify the commit message was synced
    let change = doc2.get_change(ID::new(1, 1)).unwrap();
    assert_eq!(change.message(), "edit on doc1");

    // Verify the text content was also synced
    let text2 = doc2.get_text("text");
    assert_eq!(text2.to_string(), "hello");
}

#[test]
fn test_commit_message_sync_via_snapshot() {
    let doc1 = LoroDoc::new();
    doc1.set_peer_id(1).unwrap();
    let text1 = doc1.get_text("text");

    text1.insert(0, "hello").unwrap();
    doc1.commit_with(CommitOptions::new().commit_msg("first edit"));

    text1.insert(5, " world").unwrap();
    doc1.commit_with(CommitOptions::new().commit_msg("second edit"));

    // Create a snapshot of doc1
    let snapshot = doc1.export_snapshot();

    // Create a new doc from the snapshot
    let doc2 = LoroDoc::new();
    doc2.import(&snapshot).unwrap();

    // Verify the commit messages were preserved in the snapshot
    let change1 = doc2.get_change(ID::new(1, 1)).unwrap();
    let change2 = doc2.get_change(ID::new(1, 6)).unwrap();

    assert_eq!(change1.message(), "first edit");
    assert_eq!(change2.message(), "second edit");

    // Verify the text content was also preserved
    let text2 = doc2.get_text("text");
    assert_eq!(text2.to_string(), "hello world");
}

#[test]
fn test_commit_message_sync_via_fast_snapshot() {
    let doc1 = LoroDoc::new();
    let doc2 = LoroDoc::new();
    doc1.set_peer_id(1).unwrap();
    let text1 = doc1.get_text("text");

    text1.insert(0, "hello").unwrap();
    doc1.commit_with(CommitOptions::new().commit_msg("first edit"));

    text1.insert(5, " world").unwrap();
    doc1.commit_with(CommitOptions::new().commit_msg("second edit"));

    let snapshot = doc1.export_fast_snapshot();
    doc2.import(&snapshot).unwrap();

    // Verify the commit messages were preserved in the snapshot
    let change1 = doc2.get_change(ID::new(1, 1)).unwrap();
    let change2 = doc2.get_change(ID::new(1, 6)).unwrap();

    assert_eq!(change1.message(), "first edit");
    assert_eq!(change2.message(), "second edit");

    // Verify the text content was also preserved
    let text2 = doc2.get_text("text");
    assert_eq!(text2.to_string(), "hello world");
    text2.delete(0, 10).unwrap();
    doc2.set_next_commit_message("From text2");
    doc1.import(&doc2.export_fast_snapshot()).unwrap();
    let c = doc1.get_change(ID::new(doc2.peer_id(), 0)).unwrap();
    assert_eq!(c.message(), "From text2");
}

#[test]
fn test_commit_message_json_updates() {
    let doc1 = LoroDoc::new();
    let text1 = doc1.get_text("text");

    text1.insert(0, "hello").unwrap();
    doc1.commit_with(CommitOptions::new().commit_msg("first edit"));

    text1.insert(5, " world").unwrap();
    doc1.commit_with(CommitOptions::new().commit_msg("second edit"));

    let start_vv = VersionVector::new();
    let end_vv = doc1.oplog_vv();
    let json_updates = doc1.export_json_updates(&start_vv, &end_vv);

    let doc2 = LoroDoc::new();
    doc2.import_json_updates(json_updates).unwrap();

    // Verify the commit messages were preserved in the JSON updates
    let change1 = doc2.get_change(ID::new(doc1.peer_id(), 1)).unwrap();
    let change2 = doc2.get_change(ID::new(doc1.peer_id(), 6)).unwrap();

    assert_eq!(change1.message(), "first edit");
    assert_eq!(change2.message(), "second edit");

    // Verify the text content was also preserved
    let text2 = doc2.get_text("text");
    assert_eq!(text2.to_string(), "hello world");
}

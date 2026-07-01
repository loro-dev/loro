use std::borrow::Cow;

use loro_internal::{
    cursor::PosType,
    handler::{HandlerTrait, UpdateOptions},
    loro::ExportMode,
    LoroDoc, UndoManager, UndoScope,
};

#[test]
fn test_basic_undo_group_checkpoint() {
    let doc = LoroDoc::new();
    let undo_manager = UndoManager::new(&doc);
    let text = doc.get_text("text");

    text.update("0", UpdateOptions::default()).unwrap();
    doc.commit_then_renew();
    undo_manager.group_start().unwrap();
    text.update("1", UpdateOptions::default()).unwrap();
    doc.commit_then_renew();
    text.update("12", UpdateOptions::default()).unwrap();
    doc.commit_then_renew();
    undo_manager.group_end();
    undo_manager.undo().unwrap();
    assert_eq!(
        text.to_string(),
        "0",
        "undo should undo the grouped updates"
    );
    undo_manager.redo().unwrap();
    assert_eq!(
        text.to_string(),
        "12",
        "redo should redo the grouped updates"
    );
}

#[test]
fn test_invalid_nested_group() {
    let doc = LoroDoc::new();

    let undo_manager = UndoManager::new(&doc);

    assert!(
        undo_manager.group_start().is_ok(),
        "group start should succeed"
    );
    assert!(
        undo_manager.group_start().is_err(),
        "nested group start should fail"
    );
    undo_manager.group_end();
    assert!(
        undo_manager.group_start().is_ok(),
        "nested group end should fail"
    );
}

#[test]
fn test_simulate_intersecting_remote_undo() {
    let doc = LoroDoc::new();
    let undo_manager = UndoManager::new(&doc);
    let text = doc.get_text("text");
    undo_manager.group_start().unwrap();
    println!("pushing 1");
    text.update("1", UpdateOptions::default()).unwrap();
    doc.commit_then_renew();
    println!("pushing 2");
    text.update("12", UpdateOptions::default()).unwrap();
    doc.commit_then_renew();

    // At this point, the doc has state of "12" in the "text" container

    let snapshot = doc.export(ExportMode::Snapshot).unwrap();
    let doc2 = LoroDoc::from_snapshot(&snapshot).unwrap();
    let text2 = doc2.get_text("text");
    let vv = doc2.state_vv();
    text2.update("123", UpdateOptions::default()).unwrap();
    doc2.commit_then_renew();

    let update = doc2
        .export(ExportMode::Updates {
            from: Cow::Borrowed(&vv),
        })
        .unwrap();

    println!("importing 3");
    doc.import(&update).unwrap();

    // At this point, the doc has state of "123" in the "text" container

    text.update("1234", UpdateOptions::default()).unwrap();
    println!("pushing 4");
    doc.commit_then_renew();
    println!("ending group");
    undo_manager.group_end();

    undo_manager.undo().unwrap();

    assert_eq!(text.to_string(), "123");
}

#[test]
fn test_simulate_non_intersecting_remote_undo() {
    let doc = LoroDoc::new();
    let undo_manager = UndoManager::new(&doc);
    let text = doc.get_text("text");
    undo_manager.group_start().unwrap();
    text.update("1", UpdateOptions::default()).unwrap();
    doc.commit_then_renew();
    text.update("12", UpdateOptions::default()).unwrap();
    doc.commit_then_renew();
    let snapshot = doc.export(ExportMode::Snapshot).unwrap();
    // At the point the first doc has state of "12" in the "text" container

    // Doc 2 makes changes to a separate container
    // The doc 2 has state of "123" in the "text2" container
    let doc2 = LoroDoc::from_snapshot(&snapshot).unwrap();
    let text2 = doc2.get_text("text2");
    let vv = doc2.state_vv();
    text2.update("123", UpdateOptions::default()).unwrap();
    doc2.commit_then_renew();
    let update = doc2
        .export(ExportMode::Updates {
            from: Cow::Borrowed(&vv),
        })
        .unwrap();
    doc.import(&update).unwrap();

    text.update("123", UpdateOptions::default()).unwrap();
    doc.commit_then_renew();
    undo_manager.group_end();

    undo_manager.undo().unwrap();

    assert_eq!(text.to_string(), "");
}

#[test]
fn test_undo_group_start_with_remote_ops() {
    let doc = LoroDoc::new();
    let doc2 = LoroDoc::new();
    let undo_manager = UndoManager::new(&doc);
    doc.get_text("text")
        .insert(0, "hi", PosType::Unicode)
        .unwrap();
    doc2.import(&doc.export(ExportMode::Snapshot).unwrap())
        .unwrap();
    doc2.get_text("text")
        .insert(0, "test", PosType::Unicode)
        .unwrap();
    doc.import(&doc2.export(ExportMode::Snapshot).unwrap())
        .unwrap();
    undo_manager.group_start().unwrap();
    doc.get_text("text")
        .insert(0, "t", PosType::Unicode)
        .unwrap();
    undo_manager.undo().unwrap();
    assert_eq!(doc.get_text("text").to_string(), "testhi");
    undo_manager.undo().unwrap();
    assert_eq!(doc.get_text("text").to_string(), "test");
    undo_manager.redo().unwrap();
    assert_eq!(doc.get_text("text").to_string(), "testhi");
    undo_manager.redo().unwrap();
    assert_eq!(doc.get_text("text").to_string(), "ttesthi");
    undo_manager.undo().unwrap();
    assert_eq!(doc.get_text("text").to_string(), "testhi");
    undo_manager.undo().unwrap();
    assert_eq!(doc.get_text("text").to_string(), "test");
}

#[test]
fn test_clear_redo() {
    let doc = LoroDoc::new();
    let undo_manager = UndoManager::new(&doc);
    let text = doc.get_text("text");

    // Make some edits
    text.update("hello", UpdateOptions::default()).unwrap();
    doc.commit_then_renew();
    text.update("hello world", UpdateOptions::default())
        .unwrap();
    doc.commit_then_renew();

    // Undo to create redo stack
    undo_manager.undo().unwrap();
    assert_eq!(text.to_string(), "hello");
    assert!(undo_manager.can_redo(), "should be able to redo");
    assert!(undo_manager.can_undo(), "should be able to undo");

    // Clear only redo stack
    undo_manager.clear_redo();
    assert!(!undo_manager.can_redo(), "redo stack should be empty");
    assert!(
        undo_manager.can_undo(),
        "undo stack should still have items"
    );

    // Verify undo still works
    undo_manager.undo().unwrap();
    assert_eq!(text.to_string(), "");
}

#[test]
fn test_clear_undo() {
    let doc = LoroDoc::new();
    let undo_manager = UndoManager::new(&doc);
    let text = doc.get_text("text");

    // Make some edits
    text.update("hello", UpdateOptions::default()).unwrap();
    doc.commit_then_renew();
    text.update("hello world", UpdateOptions::default())
        .unwrap();
    doc.commit_then_renew();

    // Undo to create redo stack
    undo_manager.undo().unwrap();
    assert_eq!(text.to_string(), "hello");
    assert!(undo_manager.can_redo(), "should be able to redo");
    assert!(undo_manager.can_undo(), "should be able to undo");

    // Clear only undo stack
    undo_manager.clear_undo();
    assert!(
        undo_manager.can_redo(),
        "redo stack should still have items"
    );
    assert!(!undo_manager.can_undo(), "undo stack should be empty");

    // Verify redo still works
    undo_manager.redo().unwrap();
    assert_eq!(text.to_string(), "hello world");
}

// ---------------------------------------------------------------------------
// UndoScope tests
// ---------------------------------------------------------------------------

#[test]
fn scope_default_is_doc_wide() {
    // UndoScope::Doc is the default and must behave identically to no scope.
    let doc = LoroDoc::new();
    let undo = UndoManager::new(&doc).with_scope(UndoScope::Doc);
    let text = doc.get_text("text");

    text.update("hello", UpdateOptions::default()).unwrap();
    doc.commit_then_renew();
    text.update("hello world", UpdateOptions::default()).unwrap();
    doc.commit_then_renew();

    undo.undo().unwrap();
    assert_eq!(text.to_string(), "hello");
    undo.undo().unwrap();
    assert_eq!(text.to_string(), "");
    undo.redo().unwrap();
    assert_eq!(text.to_string(), "hello");
}

#[test]
fn scope_excludes_out_of_scope_local_commits() {
    // Two text containers a/b. Scope = {a}. Edits to b must not appear on the
    // undo stack, edits to a must.
    let doc = LoroDoc::new();
    let text_a = doc.get_text("a");
    let text_b = doc.get_text("b");
    let undo = UndoManager::new(&doc).with_scope(UndoScope::containers([text_a.id()]));

    text_a.update("a1", UpdateOptions::default()).unwrap();
    doc.commit_then_renew();
    text_b.update("b1", UpdateOptions::default()).unwrap();
    doc.commit_then_renew();
    text_a.update("a1a2", UpdateOptions::default()).unwrap();
    doc.commit_then_renew();

    assert_eq!(undo.undo_count(), 2, "only the two a-edits should be tracked");

    undo.undo().unwrap();
    assert_eq!(text_a.to_string(), "a1", "first undo reverts the second a-edit");
    assert_eq!(text_b.to_string(), "b1", "b is untouched by undo");

    undo.undo().unwrap();
    assert_eq!(text_a.to_string(), "", "second undo reverts the first a-edit");
    assert_eq!(text_b.to_string(), "b1", "b is still untouched");

    assert!(!undo.can_undo(), "no more in-scope undo entries");
}

#[test]
fn scope_redo_only_in_scope() {
    // After undoing, redo should also affect only in-scope containers.
    let doc = LoroDoc::new();
    let text_a = doc.get_text("a");
    let text_b = doc.get_text("b");
    let undo = UndoManager::new(&doc).with_scope(UndoScope::containers([text_a.id()]));

    text_a.update("a1", UpdateOptions::default()).unwrap();
    doc.commit_then_renew();
    text_b.update("b1", UpdateOptions::default()).unwrap();
    doc.commit_then_renew();

    undo.undo().unwrap();
    assert_eq!(text_a.to_string(), "");
    assert_eq!(text_b.to_string(), "b1", "b unaffected by undo");

    undo.redo().unwrap();
    assert_eq!(text_a.to_string(), "a1", "redo restores a");
    assert_eq!(text_b.to_string(), "b1", "b unaffected by redo");
}

#[test]
fn scope_out_of_scope_edits_do_not_corrupt_in_scope_stack() {
    // Out-of-scope local edits between in-scope edits must advance counters
    // cleanly; an undo on a later in-scope edit must not drag the out-of-scope
    // edit into its CounterSpan.
    let doc = LoroDoc::new();
    let text_a = doc.get_text("a");
    let text_b = doc.get_text("b");
    let undo = UndoManager::new(&doc).with_scope(UndoScope::containers([text_a.id()]));

    text_a.update("a1", UpdateOptions::default()).unwrap();
    doc.commit_then_renew();
    // Several out-of-scope commits between the two a-edits
    text_b.update("b1", UpdateOptions::default()).unwrap();
    doc.commit_then_renew();
    text_b.update("b1b2", UpdateOptions::default()).unwrap();
    doc.commit_then_renew();
    text_b.update("b1b2b3", UpdateOptions::default()).unwrap();
    doc.commit_then_renew();
    text_a.update("a1a2", UpdateOptions::default()).unwrap();
    doc.commit_then_renew();

    undo.undo().unwrap();
    assert_eq!(text_a.to_string(), "a1", "second a-edit reverted");
    assert_eq!(text_b.to_string(), "b1b2b3", "b state preserved exactly");
}

#[test]
fn scope_mixed_commit_only_undoes_in_scope() {
    // A single commit touching both in-scope and out-of-scope containers is
    // recorded normally (the record-time filter passes commits with at least
    // one in-scope diff). At replay time, undo_internal masks the resulting
    // DiffBatch so only in-scope containers are reverted.
    let doc = LoroDoc::new();
    let text_a = doc.get_text("a");
    let text_b = doc.get_text("b");
    let undo = UndoManager::new(&doc).with_scope(UndoScope::containers([text_a.id()]));

    // One commit touching BOTH a and b.
    text_a.update("a1", UpdateOptions::default()).unwrap();
    text_b.update("b1", UpdateOptions::default()).unwrap();
    doc.commit_then_renew();

    assert_eq!(undo.undo_count(), 1);
    undo.undo().unwrap();
    assert_eq!(text_a.to_string(), "", "in-scope a is reverted");
    assert_eq!(
        text_b.to_string(),
        "b1",
        "out-of-scope b is preserved even though it shared the commit with a"
    );
}

#[test]
fn scope_mixed_commit_redo_only_restores_in_scope() {
    // Symmetric to undo: redoing a previously-undone mixed commit should also
    // only re-apply the in-scope portion. After undo+redo of a mixed commit,
    // out-of-scope state should be unchanged from its post-original-commit value.
    let doc = LoroDoc::new();
    let text_a = doc.get_text("a");
    let text_b = doc.get_text("b");
    let undo = UndoManager::new(&doc).with_scope(UndoScope::containers([text_a.id()]));

    text_a.update("a1", UpdateOptions::default()).unwrap();
    text_b.update("b1", UpdateOptions::default()).unwrap();
    doc.commit_then_renew();

    undo.undo().unwrap();
    assert_eq!(text_a.to_string(), "");
    assert_eq!(text_b.to_string(), "b1");

    undo.redo().unwrap();
    assert_eq!(text_a.to_string(), "a1", "in-scope a is restored");
    assert_eq!(text_b.to_string(), "b1", "out-of-scope b stays put across undo+redo");
}

#[test]
fn scope_multiple_mixed_commits_chained() {
    // Three commits, each mixed across {a} (in-scope) and {b} (out-of-scope).
    // Undoing twice should peel back only the in-scope edits; b accumulates
    // every commit's contribution untouched.
    let doc = LoroDoc::new();
    let text_a = doc.get_text("a");
    let text_b = doc.get_text("b");
    let undo = UndoManager::new(&doc).with_scope(UndoScope::containers([text_a.id()]));

    text_a.update("a1", UpdateOptions::default()).unwrap();
    text_b.update("b1", UpdateOptions::default()).unwrap();
    doc.commit_then_renew();
    text_a.update("a1a2", UpdateOptions::default()).unwrap();
    text_b.update("b1b2", UpdateOptions::default()).unwrap();
    doc.commit_then_renew();
    text_a.update("a1a2a3", UpdateOptions::default()).unwrap();
    text_b.update("b1b2b3", UpdateOptions::default()).unwrap();
    doc.commit_then_renew();

    assert_eq!(undo.undo_count(), 3);

    undo.undo().unwrap();
    assert_eq!(text_a.to_string(), "a1a2");
    assert_eq!(text_b.to_string(), "b1b2b3", "b unchanged by first undo");

    undo.undo().unwrap();
    assert_eq!(text_a.to_string(), "a1");
    assert_eq!(text_b.to_string(), "b1b2b3", "b still unchanged by second undo");

    undo.undo().unwrap();
    assert_eq!(text_a.to_string(), "");
    assert_eq!(text_b.to_string(), "b1b2b3", "b survives all undos");
}

#[test]
fn scope_mixed_commit_with_three_containers() {
    // Scope = {a, c}, single commit touches a + b + c. Undo reverts a and c
    // but not b.
    let doc = LoroDoc::new();
    let text_a = doc.get_text("a");
    let text_b = doc.get_text("b");
    let text_c = doc.get_text("c");
    let undo = UndoManager::new(&doc).with_scope(UndoScope::containers([text_a.id(), text_c.id()]));

    text_a.update("a1", UpdateOptions::default()).unwrap();
    text_b.update("b1", UpdateOptions::default()).unwrap();
    text_c.update("c1", UpdateOptions::default()).unwrap();
    doc.commit_then_renew();

    undo.undo().unwrap();
    assert_eq!(text_a.to_string(), "", "a in scope, reverted");
    assert_eq!(text_b.to_string(), "b1", "b out of scope, preserved");
    assert_eq!(text_c.to_string(), "", "c in scope, reverted");
}

#[test]
fn scope_change_after_construction() {
    // set_scope mutates an existing manager; commits before/after the change
    // are categorized by the scope active at record time.
    let doc = LoroDoc::new();
    let text_a = doc.get_text("a");
    let text_b = doc.get_text("b");
    let undo = UndoManager::new(&doc);

    // Doc-wide initially: both edits recorded.
    text_a.update("a1", UpdateOptions::default()).unwrap();
    doc.commit_then_renew();
    text_b.update("b1", UpdateOptions::default()).unwrap();
    doc.commit_then_renew();
    assert_eq!(undo.undo_count(), 2);

    // Switch scope to {a}: subsequent b-edits skipped.
    undo.set_scope(UndoScope::containers([text_a.id()]));
    text_b.update("b1b2", UpdateOptions::default()).unwrap();
    doc.commit_then_renew();
    text_a.update("a1a2", UpdateOptions::default()).unwrap();
    doc.commit_then_renew();
    assert_eq!(undo.undo_count(), 3, "the b-edit after scope-change is filtered");
}

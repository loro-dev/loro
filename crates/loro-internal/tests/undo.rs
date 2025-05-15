use loro_internal::{handler::UpdateOptions, LoroDoc, UndoManager};

#[test]
fn test_basic_undo_group_checkpoint() {
    let doc = LoroDoc::new();
    let mut undo_manager = UndoManager::new(&doc);
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

    assert_eq!(text.to_string(), "0", "undo should undo the grouped updates");

    undo_manager.redo().unwrap();

    assert_eq!(text.to_string(), "12", "redo should redo the grouped updates");
}

#[test]
fn test_invalid_nested_group() {
    let doc = LoroDoc::new();

    let mut undo_manager = UndoManager::new(&doc);

    assert!(undo_manager.group_start().is_ok(), "group start should succeed");
    assert!(undo_manager.group_start().is_err(), "nested group start should fail");
    undo_manager.group_end();
    assert!(undo_manager.group_start().is_ok(), "nested group end should fail");
}

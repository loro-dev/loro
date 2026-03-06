use std::borrow::Cow;

use loro_internal::{
    cursor::PosType, handler::UpdateOptions, loro::ExportMode, LoroDoc, UndoManager,
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
    text.update("hello world", UpdateOptions::default()).unwrap();
    doc.commit_then_renew();

    // Undo to create redo stack
    undo_manager.undo().unwrap();
    assert_eq!(text.to_string(), "hello");
    assert!(undo_manager.can_redo(), "should be able to redo");
    assert!(undo_manager.can_undo(), "should be able to undo");

    // Clear only redo stack
    undo_manager.clear_redo();
    assert!(!undo_manager.can_redo(), "redo stack should be empty");
    assert!(undo_manager.can_undo(), "undo stack should still have items");

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
    text.update("hello world", UpdateOptions::default()).unwrap();
    doc.commit_then_renew();

    // Undo to create redo stack
    undo_manager.undo().unwrap();
    assert_eq!(text.to_string(), "hello");
    assert!(undo_manager.can_redo(), "should be able to redo");
    assert!(undo_manager.can_undo(), "should be able to undo");

    // Clear only undo stack
    undo_manager.clear_undo();
    assert!(undo_manager.can_redo(), "redo stack should still have items");
    assert!(!undo_manager.can_undo(), "undo stack should be empty");

    // Verify redo still works
    undo_manager.redo().unwrap();
    assert_eq!(text.to_string(), "hello world");
}

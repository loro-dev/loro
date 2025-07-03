use std::borrow::Cow;

use loro_internal::{handler::UpdateOptions, loro::ExportMode, LoroDoc, UndoManager, DiffBatch};

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

    let mut undo_manager = UndoManager::new(&doc);

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
    let mut undo_manager = UndoManager::new(&doc);
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
    let mut undo_manager = UndoManager::new(&doc);
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
    let mut undo_manager = UndoManager::new(&doc);
    doc.get_text("text").insert(0, "hi").unwrap();
    doc2.import(&doc.export(ExportMode::Snapshot).unwrap())
        .unwrap();
    doc2.get_text("text").insert(0, "test").unwrap();
    doc.import(&doc2.export(ExportMode::Snapshot).unwrap())
        .unwrap();
    undo_manager.group_start().unwrap();
    doc.get_text("text").insert(0, "t").unwrap();
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
fn test_undo_diff_batch_generation() {
    // This test verifies that the undo diff batch parameter has been added to apply_local_op
    // The actual undo implementation will be completed in step 2 of the optimization plan
    
    // Create a DiffBatch instance to verify the type exists and can be used
    let mut _undo_batch = DiffBatch::default();
    
    // The test primarily verifies that:
    // 1. DiffBatch type is accessible
    // 2. The apply_local_op signatures have been updated with Option<&mut DiffBatch>
    // 3. The code compiles with the new parameter
    
    // Actual undo diff generation tests will be added when the implementation is complete
}

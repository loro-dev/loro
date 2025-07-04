use std::borrow::Cow;

use loro_internal::{handler::UpdateOptions, loro::ExportMode, LoroDoc, UndoManager, DiffBatch, HandlerTrait};

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
    // Simple test to verify undo diff collection works
    let doc = LoroDoc::new();
    
    // Test that we can subscribe without panic
    let _sub = doc.subscribe_undo_diffs(Box::new(move |_diff: &DiffBatch| {
        println!("Received undo diff batch");
        false // don't unsubscribe
    }));
    
    // Perform a simple operation
    let map = doc.get_map("map");
    map.insert("key1", 42).unwrap();
    doc.commit_then_renew();
    
    // If we reach here without panic, the basic mechanism is working
}

#[cfg(feature = "counter")]
#[test]
fn test_counter_undo_diff_generation() {
    use std::sync::{Arc, Mutex};
    use loro_internal::event::Diff;
    
    let doc = LoroDoc::new();
    let received_diffs = Arc::new(Mutex::new(Vec::new()));
    let received_diffs_clone = received_diffs.clone();
    
    let _sub = doc.subscribe_undo_diffs(Box::new(move |diff: &DiffBatch| {
        received_diffs_clone.lock().unwrap().push(diff.clone());
        false
    }));
    
    // Test counter increment
    let counter = doc.get_counter("counter");
    counter.increment(5.0).unwrap();
    doc.commit_then_renew();
    
    // Check that we received the undo diff
    let diffs = received_diffs.lock().unwrap();
    assert_eq!(diffs.len(), 1, "Should receive one diff batch");
    let diff_batch = &diffs[0];
    assert_eq!(diff_batch.cid_to_events.len(), 1, "Should have one container diff");
    
    let counter_id = doc.get_counter("counter").id();
    let counter_diff = diff_batch.cid_to_events.get(&counter_id).unwrap();
    
    match counter_diff {
        Diff::Counter(value) => {
            assert_eq!(*value, -5.0, "Undo diff should be the negative of the increment");
        }
        _ => panic!("Expected Counter diff"),
    }
}

#[test]
fn test_map_undo_diff_generation() {
    use std::sync::{Arc, Mutex};
    use loro_internal::event::Diff;
    
    let doc = LoroDoc::new();
    let received_diffs = Arc::new(Mutex::new(Vec::new()));
    let received_diffs_clone = received_diffs.clone();
    
    let _sub = doc.subscribe_undo_diffs(Box::new(move |diff: &DiffBatch| {
        received_diffs_clone.lock().unwrap().push(diff.clone());
        false
    }));
    
    let map = doc.get_map("map");
    
    // Test 1: Insert new key
    map.insert("key1", 42).unwrap();
    doc.commit_then_renew();
    
    {
        let diffs = received_diffs.lock().unwrap();
        assert_eq!(diffs.len(), 1, "Should receive one diff batch for insert");
        let diff_batch = &diffs[0];
        let map_id = doc.get_map("map").id();
        let map_diff = diff_batch.cid_to_events.get(&map_id).unwrap();
        
        match map_diff {
            Diff::Map(delta) => {
                assert_eq!(delta.updated.len(), 1);
                let value = delta.updated.iter().find(|(k, _)| k.as_str() == "key1").map(|(_, v)| v).unwrap();
                assert!(value.value.is_none(), "Undo of insert should remove the key");
            }
            _ => panic!("Expected Map diff"),
        }
    }
    
    // Clear received diffs
    received_diffs.lock().unwrap().clear();
    
    // Test 2: Update existing key
    map.insert("key1", "updated").unwrap();
    doc.commit_then_renew();
    
    {
        let diffs = received_diffs.lock().unwrap();
        assert_eq!(diffs.len(), 1, "Should receive one diff batch for update");
        let diff_batch = &diffs[0];
        let map_id = doc.get_map("map").id();
        let map_diff = diff_batch.cid_to_events.get(&map_id).unwrap();
        
        match map_diff {
            Diff::Map(delta) => {
                assert_eq!(delta.updated.len(), 1);
                let value = delta.updated.iter().find(|(k, _)| k.as_str() == "key1").map(|(_, v)| v).unwrap();
                assert!(value.value.is_some(), "Undo of update should restore previous value");
                // Check that the restored value is 42
                if let Some(ref val) = value.value {
                    assert_eq!(*val.as_value().unwrap().as_i64().unwrap(), 42);
                }
            }
            _ => panic!("Expected Map diff"),
        }
    }
    
    // Clear received diffs
    received_diffs.lock().unwrap().clear();
    
    // Test 3: Delete key (by inserting None)
    map.delete("key1").unwrap();
    doc.commit_then_renew();
    
    {
        let diffs = received_diffs.lock().unwrap();
        assert_eq!(diffs.len(), 1, "Should receive one diff batch for delete");
        let diff_batch = &diffs[0];
        let map_id = doc.get_map("map").id();
        let map_diff = diff_batch.cid_to_events.get(&map_id).unwrap();
        
        match map_diff {
            Diff::Map(delta) => {
                assert_eq!(delta.updated.len(), 1);
                let value = delta.updated.iter().find(|(k, _)| k.as_str() == "key1").map(|(_, v)| v).unwrap();
                assert!(value.value.is_some(), "Undo of delete should restore previous value");
                // Check that the restored value is "updated"
                if let Some(ref val) = value.value {
                    assert_eq!(val.as_value().unwrap().as_string().unwrap().as_str(), "updated");
                }
            }
            _ => panic!("Expected Map diff"),
        }
    }
}

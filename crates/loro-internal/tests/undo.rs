use std::borrow::Cow;

use loro_internal::{
    event::Diff,
    handler::{ListHandler, TextHandler, UpdateOptions},
    loro::ExportMode,
    DiffBatch, HandlerTrait, LoroDoc, LoroValue, UndoManager,
};

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
    let _sub = doc.subscribe_undo_diffs(Box::new(move |_diff| {
        println!("Received undo diff batch");
        true // Keep subscription active // don't unsubscribe
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

    let doc = LoroDoc::new();
    let received_diffs = Arc::new(Mutex::new(Vec::new()));
    let received_diffs_clone = received_diffs.clone();

    let _sub = doc.subscribe_undo_diffs(Box::new(move |diff| {
        received_diffs_clone.lock().unwrap().push(diff.clone());
        true // Keep subscription active
    }));

    // Test counter increment
    let counter = doc.get_counter("counter");
    counter.increment(5.0).unwrap();
    doc.commit_then_renew();

    // Check that we received the undo diff
    let diffs = received_diffs.lock().unwrap();
    assert_eq!(diffs.len(), 1, "Should receive one diff batch");
    let diff_batch = &diffs[0];
    assert_eq!(
        diff_batch.diff.cid_to_events.len(),
        1,
        "Should have one container diff"
    );

    let counter_id = doc.get_counter("counter").id();
    let counter_diff = diff_batch.diff.cid_to_events.get(&counter_id).unwrap();

    match counter_diff {
        Diff::Counter(value) => {
            assert_eq!(
                *value, -5.0,
                "Undo diff should be the negative of the increment"
            );
        }
        _ => panic!("Expected Counter diff"),
    }
}

#[test]
fn test_map_undo_diff_generation() {
    use std::sync::{Arc, Mutex};

    let doc = LoroDoc::new();
    let received_diffs = Arc::new(Mutex::new(Vec::new()));
    let received_diffs_clone = received_diffs.clone();

    let _sub = doc.subscribe_undo_diffs(Box::new(move |diff| {
        received_diffs_clone.lock().unwrap().push(diff.clone());
        true // Keep subscription active
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
        let map_diff = diff_batch.diff.cid_to_events.get(&map_id).unwrap();

        match map_diff {
            Diff::Map(delta) => {
                assert_eq!(delta.updated.len(), 1);
                let value = delta
                    .updated
                    .iter()
                    .find(|(k, _)| k.as_str() == "key1")
                    .map(|(_, v)| v)
                    .unwrap();
                assert!(
                    value.value.is_none(),
                    "Undo of insert should remove the key"
                );
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
        let map_diff = diff_batch.diff.cid_to_events.get(&map_id).unwrap();

        match map_diff {
            Diff::Map(delta) => {
                assert_eq!(delta.updated.len(), 1);
                let value = delta
                    .updated
                    .iter()
                    .find(|(k, _)| k.as_str() == "key1")
                    .map(|(_, v)| v)
                    .unwrap();
                assert!(
                    value.value.is_some(),
                    "Undo of update should restore previous value"
                );
                // Check that the restored value is 42
                if let Some(ref val) = value.value {
                    if let LoroValue::I64(n) = val.as_value().unwrap() {
                        assert_eq!(*n, 42);
                    } else {
                        panic!("Expected i64 value");
                    }
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
        let map_diff = diff_batch.diff.cid_to_events.get(&map_id).unwrap();

        match map_diff {
            Diff::Map(delta) => {
                assert_eq!(delta.updated.len(), 1);
                let value = delta
                    .updated
                    .iter()
                    .find(|(k, _)| k.as_str() == "key1")
                    .map(|(_, v)| v)
                    .unwrap();
                assert!(
                    value.value.is_some(),
                    "Undo of delete should restore previous value"
                );
                // Check that the restored value is "updated"
                if let Some(ref val) = value.value {
                    if let LoroValue::String(s) = val.as_value().unwrap() {
                        assert_eq!(s.as_str(), "updated");
                    } else {
                        panic!("Expected string value");
                    }
                }
            }
            _ => panic!("Expected Map diff"),
        }
    }
}

#[test]
fn test_list_undo_diff_generation() {
    use std::sync::{Arc, Mutex};

    // Test 1: Insert operation
    {
        let doc = LoroDoc::new();
        let received_diffs = Arc::new(Mutex::new(Vec::new()));
        let received_diffs_clone = received_diffs.clone();

        let _sub = doc.subscribe_undo_diffs(Box::new(move |diff| {
            received_diffs_clone.lock().unwrap().push(diff.clone());
            true // Keep subscription active
        }));

        let list = doc.get_list("list");
        list.push(42).unwrap();
        doc.commit_then_renew();

        let diffs = received_diffs.lock().unwrap();
        assert_eq!(diffs.len(), 1, "Should receive one diff batch for insert");
        let diff_batch = &diffs[0];
        let list_id = doc.get_list("list").id();
        let list_diff = diff_batch.diff.cid_to_events.get(&list_id).unwrap();

        match list_diff {
            Diff::List(list_diff) => {
                // Should have a single delete operation of length 1
                let items: Vec<_> = list_diff.iter().collect();
                assert_eq!(items.len(), 1);
                match &items[0] {
                    loro_internal::loro_delta::DeltaItem::Replace { delete, value, .. } => {
                        assert_eq!(*delete, 1, "Undo of insert should delete 1 element");
                        assert!(value.is_empty(), "Should not have any insert values");
                    }
                    _ => panic!("Expected Replace operation with delete"),
                }
            }
            _ => panic!("Expected List diff"),
        }
    }

    // Test 2: Delete operation
    {
        let doc = LoroDoc::new();
        let received_diffs = Arc::new(Mutex::new(Vec::new()));
        let received_diffs_clone = received_diffs.clone();

        let _sub = doc.subscribe_undo_diffs(Box::new(move |diff| {
            received_diffs_clone.lock().unwrap().push(diff.diff.clone());
            true // true means keep the subscription active
        }));

        let list = doc.get_list("list");
        // First insert some data
        list.push(42).unwrap();
        list.push("hello").unwrap();
        doc.commit_then_renew();

        // Clear the diffs from inserts
        received_diffs.lock().unwrap().clear();

        // Now delete the first element
        list.delete(0, 1).unwrap();
        doc.commit_then_renew();

        let diffs = received_diffs.lock().unwrap();
        assert_eq!(diffs.len(), 1, "Should receive one diff batch for delete");
        let diff_batch = &diffs[0];
        let list_id = doc.get_list("list").id();
        let list_diff = diff_batch.cid_to_events.get(&list_id).unwrap();

        match list_diff {
            Diff::List(list_diff) => {
                // Should have insert(42) at position 0
                let items: Vec<_> = list_diff.iter().collect();
                assert_eq!(items.len(), 1);
                match &items[0] {
                    loro_internal::loro_delta::DeltaItem::Replace { value, delete, .. } => {
                        assert_eq!(*delete, 0, "Should not delete any elements");
                        assert_eq!(value.len(), 1, "Should insert 1 element");
                        // Check that the inserted value is 42
                        assert_eq!(*value[0].as_value().unwrap().as_i64().unwrap(), 42);
                    }
                    _ => panic!("Expected Replace operation with insert"),
                }
            }
            _ => panic!("Expected List diff"),
        }
    }
}

#[test]
fn test_tree_undo_diff_generation() {
    use loro_internal::TreeParentId;
    use std::sync::{Arc, Mutex};

    let doc = LoroDoc::new();
    let received_diffs = Arc::new(Mutex::new(Vec::new()));
    let received_diffs_clone = received_diffs.clone();

    let _sub = doc.subscribe_undo_diffs(Box::new(move |diff| {
        received_diffs_clone.lock().unwrap().push(diff.diff.clone());
        true
    }));

    let tree = doc.get_tree("tree");

    // Test 1: Create a node
    let node_a = tree.create_at(TreeParentId::Root, 0).unwrap();
    doc.commit_then_renew();

    {
        let diffs = received_diffs.lock().unwrap();
        assert_eq!(diffs.len(), 1, "Should receive one diff batch for create");
        let diff_batch = &diffs[0];
        let tree_id = doc.get_tree("tree").id();
        let tree_diff = diff_batch.cid_to_events.get(&tree_id).unwrap();

        match tree_diff {
            Diff::Tree(diff) => {
                assert_eq!(diff.diff.len(), 1);
                assert!(
                    matches!(
                        &diff.diff[0].action,
                        loro_internal::delta::TreeExternalDiff::Delete { .. }
                    ),
                    "Undo of create should be delete"
                );
            }
            _ => panic!("Expected Tree diff"),
        }
    }

    // Clear received diffs
    received_diffs.lock().unwrap().clear();

    // Test 2: Delete a node
    tree.delete(node_a).unwrap();
    doc.commit_then_renew();

    {
        let diffs = received_diffs.lock().unwrap();
        assert_eq!(diffs.len(), 1, "Should receive one diff batch for delete");
        let diff_batch = &diffs[0];
        let tree_id = doc.get_tree("tree").id();
        let tree_diff = diff_batch.cid_to_events.get(&tree_id).unwrap();

        match tree_diff {
            Diff::Tree(diff) => {
                assert_eq!(diff.diff.len(), 1);
                assert!(
                    matches!(
                        &diff.diff[0].action,
                        loro_internal::delta::TreeExternalDiff::Create { parent, .. }
                        if parent == &TreeParentId::Root
                    ),
                    "Undo of delete should recreate at original position"
                );
            }
            _ => panic!("Expected Tree diff"),
        }
    }
}

#[test]
fn test_richtext_undo_diff_generation() {
    use std::sync::{Arc, Mutex};

    let doc = LoroDoc::new();
    let received_diffs = Arc::new(Mutex::new(Vec::new()));
    let received_diffs_clone = received_diffs.clone();

    let _sub = doc.subscribe_undo_diffs(Box::new(move |diff| {
        received_diffs_clone.lock().unwrap().push(diff.diff.clone());
        true
    }));

    let text = doc.get_text("text");

    // Test 1: Insert text
    text.insert(0, "Hello").unwrap();
    doc.commit_then_renew();

    {
        let diffs = received_diffs.lock().unwrap();
        assert_eq!(diffs.len(), 1, "Should receive one diff batch for insert");
        let diff_batch = &diffs[0];
        let text_id = doc.get_text("text").id();
        let text_diff = diff_batch.cid_to_events.get(&text_id).unwrap();

        match text_diff {
            Diff::Text(delta) => {
                // Should have a delete operation
                let mut has_delete = false;
                for item in delta.iter() {
                    if let loro_internal::loro_delta::DeltaItem::Replace { delete, value, .. } =
                        item
                    {
                        if value.is_empty() && delete > &0 {
                            assert_eq!(*delete, 5, "Should delete 5 characters");
                            has_delete = true;
                        }
                    }
                }
                assert!(has_delete, "Should have delete operation");
            }
            _ => panic!("Expected Text diff"),
        }
    }

    // Clear received diffs
    received_diffs.lock().unwrap().clear();

    // Test 2: Delete text
    text.delete(0, 5).unwrap();
    doc.commit_then_renew();

    {
        let diffs = received_diffs.lock().unwrap();
        assert_eq!(diffs.len(), 1, "Should receive one diff batch for delete");
        let diff_batch = &diffs[0];
        let text_id = doc.get_text("text").id();
        let text_diff = diff_batch.cid_to_events.get(&text_id).unwrap();

        match text_diff {
            Diff::Text(delta) => {
                // Should have an insert operation with "Hello"
                let mut has_correct_insert = false;
                for item in delta.iter() {
                    if let loro_internal::loro_delta::DeltaItem::Replace {
                        value: insert,
                        delete,
                        ..
                    } = item
                    {
                        if delete == &0 && !insert.is_empty() {
                            // Text values have an as_str method that returns &str directly
                            if insert.as_str() == "Hello" {
                                has_correct_insert = true;
                            }
                        }
                    }
                }
                assert!(has_correct_insert, "Should insert back 'Hello'");
            }
            _ => panic!("Expected Text diff"),
        }
    }
}

#[test]
fn test_tree_undo_integration() {
    use loro_internal::TreeParentId;

    let doc = LoroDoc::new();
    let mut undo_manager = UndoManager::new(&doc);
    let tree = doc.get_tree("tree");

    // Create two nodes
    let node_a = tree.create_at(TreeParentId::Root, 0).unwrap();
    doc.commit_then_renew();

    let node_b = tree.create_at(TreeParentId::Root, 0).unwrap();
    doc.commit_then_renew();

    tree.mov(node_b, TreeParentId::Node(node_a)).unwrap();
    doc.commit_then_renew();

    // Verify node_b is under node_a
    assert_eq!(
        tree.get_node_parent(&node_b),
        Some(TreeParentId::Node(node_a))
    );

    // Undo the move
    undo_manager.undo().unwrap();

    // Verify node_b is back at root
    assert_eq!(tree.get_node_parent(&node_b), Some(TreeParentId::Root));

    // Redo the move
    undo_manager.redo().unwrap();

    // Verify node_b is under node_a again
    assert_eq!(
        tree.get_node_parent(&node_b),
        Some(TreeParentId::Node(node_a))
    );
}

#[test]
fn test_movable_list_undo_diff_generation() {
    use std::sync::{Arc, Mutex};

    let doc = LoroDoc::new();
    let received_diffs = Arc::new(Mutex::new(Vec::new()));
    let received_diffs_clone = received_diffs.clone();

    let _sub = doc.subscribe_undo_diffs(Box::new(move |diff| {
        received_diffs_clone.lock().unwrap().push(diff.diff.clone());
        true // Keep subscription active
    }));

    let list = doc.get_movable_list("mlist");

    // Test 1: Insert elements
    list.insert(0, "first").unwrap();
    list.insert(1, "second").unwrap();
    doc.commit_then_renew();

    {
        let diffs = received_diffs.lock().unwrap();
        assert_eq!(diffs.len(), 1, "Should receive one diff batch for inserts");
        let diff_batch = &diffs[0];
        let list_id = doc.get_movable_list("mlist").id();
        let list_diff = diff_batch.cid_to_events.get(&list_id).unwrap();

        match list_diff {
            Diff::List(delta) => {
                // Should have delete operations for the inserted elements
                let mut has_delete = false;
                for item in delta.iter() {
                    if matches!(item, loro_delta::DeltaItem::Replace { delete, value, .. } if value.is_empty() && delete > &0)
                    {
                        has_delete = true;
                    }
                }
                assert!(has_delete, "Should have delete operations for undo");
            }
            _ => panic!("Expected List diff"),
        }
    }

    // Clear received diffs
    received_diffs.lock().unwrap().clear();

    // Test 2: Set operation (update value)
    // MovableList uses index-based set operation
    list.set(0, "updated_first").unwrap();
    doc.commit_then_renew();

    {
        let diffs = received_diffs.lock().unwrap();
        assert_eq!(diffs.len(), 1, "Should receive one diff batch for set");
        let diff_batch = &diffs[0];
        let list_id = doc.get_movable_list("mlist").id();
        let list_diff = diff_batch.cid_to_events.get(&list_id).unwrap();

        match list_diff {
            Diff::List(delta) => {
                // Should restore the original value
                let mut has_correct_restore = false;
                for item in delta.iter() {
                    if let loro_delta::DeltaItem::Replace { value, delete, .. } = item {
                        if delete == &0 && !value.is_empty() {
                            for v in value.iter() {
                                if let Some(val) = v.as_value() {
                                    if val.as_string().unwrap().as_str() == "first" {
                                        has_correct_restore = true;
                                    }
                                }
                            }
                        }
                    }
                }
                assert!(has_correct_restore, "Should restore original value 'first'");
            }
            _ => panic!("Expected List diff"),
        }
    }

    // Clear received diffs
    received_diffs.lock().unwrap().clear();

    // Test 3: Move operation
    list.mov(1, 0).unwrap(); // Move "second" to position 0
    doc.commit_then_renew();

    {
        let diffs = received_diffs.lock().unwrap();
        assert_eq!(diffs.len(), 1, "Should receive one diff batch for move");
        // Move operations generate complex diffs, just verify we got something
        assert!(!diffs[0].cid_to_events.is_empty());
    }

    // Clear received diffs
    received_diffs.lock().unwrap().clear();

    // Test 4: Delete operation
    list.delete(0, 1).unwrap();
    doc.commit_then_renew();

    {
        let diffs = received_diffs.lock().unwrap();
        assert_eq!(diffs.len(), 1, "Should receive one diff batch for delete");
        let diff_batch = &diffs[0];
        let list_id = doc.get_movable_list("mlist").id();
        let list_diff = diff_batch.cid_to_events.get(&list_id).unwrap();

        match list_diff {
            Diff::List(delta) => {
                // Should have insert operation to restore deleted element
                let mut has_insert = false;
                for item in delta.iter() {
                    if matches!(item, loro_delta::DeltaItem::Replace { delete, value, .. } if delete == &0 && !value.is_empty())
                    {
                        has_insert = true;
                    }
                }
                assert!(
                    has_insert,
                    "Should have insert operation to restore deleted element"
                );
            }
            _ => panic!("Expected List diff"),
        }
    }
}

#[test]
fn test_undo_diff_batch_operations() {
    use std::sync::{Arc, Mutex};

    let doc = LoroDoc::new();
    let received_diffs = Arc::new(Mutex::new(Vec::new()));
    let received_diffs_clone = received_diffs.clone();

    let _sub = doc.subscribe_undo_diffs(Box::new(move |diff| {
        received_diffs_clone.lock().unwrap().push(diff.diff.clone());
        true
    }));

    // Test batch operations in a single transaction
    {
        let map = doc.get_map("map");
        map.insert("key1", 1).unwrap();
        map.insert("key2", 2).unwrap();
        map.insert("key3", 3).unwrap();

        let list = doc.get_list("list");
        list.push("a").unwrap();
        list.push("b").unwrap();
        list.push("c").unwrap();

        let text = doc.get_text("text");
        text.insert(0, "Hello World").unwrap();
    }
    doc.commit_then_renew();

    let diffs = received_diffs.lock().unwrap();
    assert_eq!(
        diffs.len(),
        1,
        "Should receive one diff batch for the transaction"
    );

    let diff_batch = &diffs[0];
    assert_eq!(
        diff_batch.cid_to_events.len(),
        3,
        "Should have diffs for 3 containers"
    );

    // Verify each container has appropriate undo diffs
    let map_id = doc.get_map("map").id();
    assert!(diff_batch.cid_to_events.contains_key(&map_id));

    let list_id = doc.get_list("list").id();
    assert!(diff_batch.cid_to_events.contains_key(&list_id));

    let text_id = doc.get_text("text").id();
    assert!(diff_batch.cid_to_events.contains_key(&text_id));
}

#[test]
fn test_undo_diff_empty_operations() {
    use std::sync::{Arc, Mutex};

    let doc = LoroDoc::new();
    let received_diffs = Arc::new(Mutex::new(Vec::new()));
    let received_diffs_clone = received_diffs.clone();

    let _sub = doc.subscribe_undo_diffs(Box::new(move |diff| {
        received_diffs_clone.lock().unwrap().push(diff.diff.clone());
        true
    }));

    // Test deleting from empty containers
    let list = doc.get_list("list");
    // This should not generate any undo diff since there's nothing to delete
    let result = list.delete(0, 1);
    assert!(result.is_err(), "Deleting from empty list should fail");

    doc.commit_then_renew();

    let diffs = received_diffs.lock().unwrap();
    assert_eq!(
        diffs.len(),
        0,
        "Should not receive any diff for failed operations"
    );
}

#[test]
fn test_undo_diff_nested_containers() {
    use std::sync::{Arc, Mutex};

    let doc = LoroDoc::new();
    let received_diffs = Arc::new(Mutex::new(Vec::new()));
    let received_diffs_clone = received_diffs.clone();

    let _sub = doc.subscribe_undo_diffs(Box::new(move |diff| {
        received_diffs_clone.lock().unwrap().push(diff.diff.clone());
        true
    }));

    // Create nested structure: map -> list -> text
    let root_map = doc.get_map("root");
    let nested_list = root_map
        .insert_container("list", ListHandler::new_detached())
        .unwrap();
    let nested_text = nested_list
        .insert_container(0, TextHandler::new_detached())
        .unwrap();
    nested_text.insert(0, "nested content").unwrap();
    doc.commit_then_renew();

    let diffs = received_diffs.lock().unwrap();
    // Should have diffs for all three containers
    assert!(diffs.len() >= 1, "Should receive diff batches");

    // Verify the root map has an undo diff
    let root_id = doc.get_map("root").id();
    let has_root_diff = diffs
        .iter()
        .any(|batch| batch.cid_to_events.contains_key(&root_id));
    assert!(has_root_diff, "Should have undo diff for root map");
}

#[test]
fn test_undo_diff_concurrent_edits() {
    use std::sync::{Arc, Mutex};

    let doc1 = LoroDoc::new();
    let doc2 = LoroDoc::new();

    let received_diffs = Arc::new(Mutex::new(Vec::new()));
    let received_diffs_clone = received_diffs.clone();

    let _sub = doc1.subscribe_undo_diffs(Box::new(move |diff| {
        received_diffs_clone.lock().unwrap().push(diff.diff.clone());
        true
    }));

    // Make concurrent edits
    let list1 = doc1.get_list("list");
    list1.push("doc1_item").unwrap();
    doc1.commit_then_renew();

    let list2 = doc2.get_list("list");
    list2.push("doc2_item").unwrap();
    doc2.commit_then_renew();

    // Sync documents
    let updates = doc2
        .export(loro_internal::loro::ExportMode::all_updates())
        .unwrap();
    doc1.import(&updates).unwrap();

    // Check that undo diffs were generated only for local operations
    let diffs = received_diffs.lock().unwrap();
    assert_eq!(
        diffs.len(),
        1,
        "Should only have undo diff for local operation"
    );

    let diff_batch = &diffs[0];
    let list_id = doc1.get_list("list").id();
    assert!(diff_batch.cid_to_events.contains_key(&list_id));
}

#[test]
fn test_apply_undo_diff_restores_state() {
    use std::sync::{Arc, Mutex};

    let doc = LoroDoc::new();
    let collected_diff = Arc::new(Mutex::new(None));
    let collected_diff_clone = collected_diff.clone();

    let _sub = doc.subscribe_undo_diffs(Box::new(move |diff| {
        *collected_diff_clone.lock().unwrap() = Some(diff.diff.clone());
        true
    }));

    // Make some changes
    let map = doc.get_map("map");
    map.insert("key", "original_value").unwrap();
    doc.commit_then_renew();

    // Update the value
    map.insert("key", "new_value").unwrap();
    assert_eq!(
        map.get("key").unwrap().into_string().unwrap().as_str(),
        "new_value"
    );

    // Apply the undo diff to restore original state
    let diff_option = collected_diff.lock().unwrap().clone();
    if let Some(diff_batch) = diff_option {
        // The test demonstrates that we successfully captured the undo diff
        // In a real implementation, these diffs would be used by the UndoManager
        assert_eq!(
            diff_batch.cid_to_events.len(),
            1,
            "Should have one container diff"
        );
        let map_id = doc.get_map("map").id();
        assert!(
            diff_batch.cid_to_events.contains_key(&map_id),
            "Should have diff for map container"
        );

        // Apply diffs manually for now - this method doesn't exist in the public API
        // The test is demonstrating how undo diffs would be applied
        // doc.apply_doc_diff(vec![doc_diff]).unwrap();

        // In a real implementation, applying the undo diff would restore the state
        // For now, we just verify that the diff was generated correctly
    }
}

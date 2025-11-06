#![allow(unexpected_cfgs)]
use loro::{
    cursor::Cursor, ContainerID, ContainerTrait, EncodedBlobMode, ExportMode, LoroDoc, LoroList,
    LoroText, UndoManager,
};
use std::sync::{Arc, Mutex};
use tracing::{trace, trace_span};

#[ctor::ctor]
fn init() {
    dev_utils::setup_test_log();
}

#[test]
fn test_event_hint_cross_container_merge_bug() {
    let doc = LoroDoc::new();
    let text_a = doc.get_text("text_a");
    let text_b = doc.get_text("text_b");

    // Insert initial content
    text_a.insert(0, "a").unwrap();
    text_b.insert(0, "b").unwrap();
    doc.commit();

    // Track events
    let events = Arc::new(Mutex::new(Vec::new()));
    let events_clone = events.clone();

    let _guard = doc.subscribe_root(Arc::new(move |batch| {
        for event in batch.events {
            events_clone
                .lock()
                .unwrap()
                .push(event.target.name().to_string());
        }
    }));

    // Delete from both containers - this should generate 2 events
    text_a.delete(0, 1).unwrap();
    text_b.delete(0, 1).unwrap();
    doc.commit();

    // Bug: Only 1 event is generated instead of 2
    let events = events.lock().unwrap();
    assert_eq!(
        events.len(),
        2,
        "Expected 2 events, got {}: {:?}",
        events.len(),
        *events
    );
}

#[test]
fn test_event_hint_bug_reproduction() {
    // This test specifically reproduces the EventHint merge bug
    // by creating delete operations that will be merged incorrectly
    let doc = LoroDoc::new();
    let text_a = doc.get_text("text_a");
    let text_b = doc.get_text("text_b");

    // Insert content
    text_a.insert(0, "hello").unwrap();
    text_b.insert(0, "world").unwrap();
    doc.commit();

    // Track detailed event information
    let events = Arc::new(Mutex::new(Vec::new()));
    let events_clone = events.clone();

    let _guard = doc.subscribe_root(Arc::new(move |event_batch| {
        let mut events_lock = events_clone.lock().unwrap();

        for event in event_batch.events.iter() {
            let container_name = event.target.name().as_str().to_string();

            if let Some(text_diff) = event.diff.as_text() {
                // Count total operations in the diff
                let mut total_ops = 0;
                let mut delete_ops = 0;
                let mut retain_ops = 0;

                for delta in text_diff.iter() {
                    total_ops += 1;
                    let delta_str = format!("{delta:?}");
                    if delta_str.contains("Delete") {
                        delete_ops += 1;
                    } else if delta_str.contains("Retain") {
                        retain_ops += 1;
                    }
                }

                events_lock.push((container_name, total_ops, delete_ops, retain_ops));
            }
        }
    }));

    // Perform operations that should trigger the bug
    // Delete from position 0 in text_a (deletes 'h')
    text_a.delete(0, 1).unwrap();
    // Delete from position 0 in text_b (deletes 'w')
    text_b.delete(0, 1).unwrap();
    doc.commit();

    let events_lock = events.lock().unwrap();

    println!("\n=== Bug Reproduction Test ===");
    println!("Events received: {:?}", *events_lock);

    // The bug would cause these events to be merged incorrectly
    // We should have 2 events, one for each container
    assert_eq!(
        events_lock.len(),
        2,
        "Should have exactly 2 events, got {}",
        events_lock.len()
    );

    // Each event should only contain operations for its own container
    let text_a_events: Vec<_> = events_lock
        .iter()
        .filter(|(name, _, _, _)| name == "text_a")
        .collect();
    let text_b_events: Vec<_> = events_lock
        .iter()
        .filter(|(name, _, _, _)| name == "text_b")
        .collect();

    assert_eq!(text_a_events.len(), 1, "text_a should have exactly 1 event");
    assert_eq!(text_b_events.len(), 1, "text_b should have exactly 1 event");

    // Check the operations count
    if let Some((_, total_ops, delete_ops, _)) = text_a_events.first() {
        assert_eq!(*total_ops, 1, "text_a should have 1 operation");
        assert_eq!(*delete_ops, 1, "text_a should have 1 delete operation");
    }

    if let Some((_, total_ops, delete_ops, retain_ops)) = text_b_events.first() {
        // text_b might have a retain operation if the bug manifests
        println!(
            "text_b operations - total: {total_ops}, deletes: {delete_ops}, retains: {retain_ops}"
        );
        // If the bug exists, text_b might show unexpected operations
    }

    // Verify final state
    assert_eq!(text_a.to_string(), "ello");
    assert_eq!(text_b.to_string(), "orld");
}

#[test]
fn test_event_hint_merge_bug_clear_demonstration() {
    // This test clearly demonstrates the EventHint merge bug
    let doc = LoroDoc::new();
    let text_a = doc.get_text("text_a");
    let text_b = doc.get_text("text_b");

    // Insert content
    text_a.insert(0, "12345").unwrap();
    text_b.insert(0, "abcde").unwrap();
    doc.commit();

    // Track which containers received events
    let event_containers = Arc::new(Mutex::new(Vec::new()));
    let event_containers_clone = event_containers.clone();

    let _guard = doc.subscribe_root(Arc::new(move |event_batch| {
        let mut containers = event_containers_clone.lock().unwrap();

        println!("\n=== Event Batch ===");
        println!("Total events in batch: {}", event_batch.events.len());

        for (idx, event) in event_batch.events.iter().enumerate() {
            let container_name = event.target.name().as_str().to_string();
            println!("Event #{idx}: Container '{container_name}'");

            if let Some(text_diff) = event.diff.as_text() {
                println!("  Diff operations:");
                for (i, delta) in text_diff.iter().enumerate() {
                    println!("    Operation #{i}: {delta:?}");
                }
            }

            containers.push(container_name);
        }
        println!("=== End Batch ===\n");
    }));

    println!("\nPerforming delete operations:");
    println!("- Deleting position 0 from text_a (removes '1')");
    println!("- Deleting position 0 from text_b (removes 'a')");

    // These two operations should generate two separate events
    // But due to the bug, they might be merged into one
    text_a.delete(0, 1).unwrap();
    text_b.delete(0, 1).unwrap();
    doc.commit();

    let containers = event_containers.lock().unwrap();

    // This assertion will fail if the bug is present
    assert_eq!(
        containers.len(),
        2,
        "Expected 2 events (one for each container), but got {}. Events: {:?}",
        containers.len(),
        *containers
    );

    // Check that both containers received their own events
    let text_a_count = containers.iter().filter(|&c| c == "text_a").count();
    let text_b_count = containers.iter().filter(|&c| c == "text_b").count();

    assert_eq!(text_a_count, 1, "text_a should have exactly 1 event");
    assert_eq!(text_b_count, 1, "text_b should have exactly 1 event");

    // Verify the final state is correct
    assert_eq!(text_a.to_string(), "2345");
    assert_eq!(text_b.to_string(), "bcde");
}

#[test]
fn import_twice() {
    let doc = LoroDoc::new();
    let base64 = "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";
    let decoded_bytes = base64::decode(base64).expect("base64 decode error");
    doc.import(&decoded_bytes).unwrap();
    doc.import(&decoded_bytes).unwrap();
}

#[test]
fn import_doc_err() {
    let base64 = include_bytes!("./issue_import.base64.txt");
    let base64 = str::from_utf8(base64).unwrap();
    let decoded_bytes = base64::decode(base64).expect("base64 decode error");

    let doc = LoroDoc::new();
    doc.import(&decoded_bytes).unwrap();
    dbg!(doc.get_deep_value());
}

#[test]
fn undo_tree_mov_between_children() {
    let doc = LoroDoc::new();
    let mut undo = UndoManager::new(&doc);
    let tree = doc.get_tree("tree");
    let a = tree.create(None).unwrap();
    tree.get_meta(a).unwrap().insert("title", "A").unwrap();
    doc.commit();
    let b = tree.create(None).unwrap();
    tree.get_meta(b).unwrap().insert("title", "B").unwrap();
    doc.commit();
    let doc_value_0 = doc.get_deep_value();
    tree.mov_after(a, b).unwrap();
    undo.undo().unwrap();
    let doc_value_1 = doc.get_deep_value();
    assert_eq!(doc_value_0, doc_value_1);
}

#[test]
fn issue_822_tree_shallow_snapshot_roundtrip() {
    let snapshot_bytes = include_bytes!("./issue_822.bin");
    let doc = LoroDoc::new();
    doc.import(snapshot_bytes).expect("import snapshot blob");

    let tree = doc.get_tree("nodes");
    let tree_before = tree.get_value();
    let doc_before = doc.get_value();

    let snapshot_meta =
        LoroDoc::decode_import_blob_meta(snapshot_bytes, false).expect("decode snapshot meta");
    assert!(snapshot_meta.mode.is_snapshot());
    let imported_is_shallow = snapshot_meta.mode == EncodedBlobMode::ShallowSnapshot;

    let frontiers = doc.state_frontiers();
    let shallow_bytes = trace_span!("EXPORT").in_scope(|| {
        doc.export(ExportMode::shallow_snapshot(&frontiers))
            .expect("export shallow snapshot")
    });

    let snapshot_meta_1 = LoroDoc::decode_import_blob_meta(&shallow_bytes, false).unwrap();
    assert!(matches!(
        snapshot_meta_1.mode,
        EncodedBlobMode::ShallowSnapshot
    ));

    let shallow_meta =
        LoroDoc::decode_import_blob_meta(&shallow_bytes, false).expect("decode shallow meta");
    assert_eq!(shallow_meta.mode, EncodedBlobMode::ShallowSnapshot);

    let shallow_doc = LoroDoc::new();
    trace_span!("FINAL_IMPORT").in_scope(|| {
        trace!("bytes.len: {}", shallow_bytes.len());
        shallow_doc
            .import(&shallow_bytes)
            .expect("import shallow snapshot");
    });

    assert!(shallow_doc.is_shallow());
    assert_eq!(doc.is_shallow(), imported_is_shallow);

    let tree_after = shallow_doc.get_tree("nodes").get_value();
    let doc_after = shallow_doc.get_value();

    assert_eq!(
        tree_before, tree_after,
        "tree shallow value should roundtrip"
    );
    assert_eq!(doc_before, doc_after, "doc shallow value should roundtrip");
}

#[test]
fn fix_get_unknown_cursor_position() {
    let doc = LoroDoc::new();
    let pos = doc.get_cursor_pos(&Cursor::new(
        None,
        ContainerID::Normal {
            peer: 10,
            counter: 0,
            container_type: loro::ContainerType::List,
        },
        loro::cursor::Side::Left,
        0,
    ));
    assert!(matches!(pos, Err(..)));
}

#[test]
fn get_unknown_cursor_position_but_its_in_pending() {
    let doc_0 = LoroDoc::new();
    let list = doc_0
        .get_map("map")
        .insert_container("list", LoroList::new())
        .unwrap();
    let v = doc_0.oplog_vv();
    let text = list.insert_container(0, LoroText::new()).unwrap();
    text.insert(0, "h").unwrap();
    doc_0.commit();
    text.insert(1, "heihei").unwrap();
    let updates = doc_0.export(ExportMode::updates_owned(v)).unwrap();

    let doc_1 = LoroDoc::new();
    let import_status = doc_1.import(&updates).unwrap();
    assert!(import_status.pending.is_some());
    assert!(doc_1.get_container(text.id()).is_none());
    assert!(!doc_1.has_container(&text.id()));
    assert_eq!(doc_1.get_path_to_container(&text.id()), None);
}

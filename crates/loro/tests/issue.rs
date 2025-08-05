#![allow(unexpected_cfgs)]
use loro::LoroDoc;
use std::sync::{Arc, Mutex};

#[ctor::ctor]
fn init() {
    dev_utils::setup_test_log();
}

#[test]
fn issue_0() {
    let bytes = include_bytes!("./issue_0.bin");
    let doc = LoroDoc::new();
    doc.import_batch(&[bytes.into()]).unwrap();
    #[allow(deprecated)]
    doc.export_snapshot();
    doc.export(loro::ExportMode::Snapshot).unwrap();
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
                    let delta_str = format!("{:?}", delta);
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
            "text_b operations - total: {}, deletes: {}, retains: {}",
            total_ops, delete_ops, retain_ops
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
            println!("Event #{}: Container '{}'", idx, container_name);

            if let Some(text_diff) = event.diff.as_text() {
                println!("  Diff operations:");
                for (i, delta) in text_diff.iter().enumerate() {
                    println!("    Operation #{}: {:?}", i, delta);
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

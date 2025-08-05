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
            events_clone.lock().unwrap().push(event.target.name().to_string());
        }
    }));
    
    // Delete from both containers - this should generate 2 events
    text_a.delete(0, 1).unwrap();
    text_b.delete(0, 1).unwrap();
    doc.commit();
    
    // Bug: Only 1 event is generated instead of 2
    let events = events.lock().unwrap();
    assert_eq!(events.len(), 2, "Expected 2 events, got {}: {:?}", events.len(), *events);
}

#[test]
fn test_event_hint_cross_container_merge_bug_detailed() {
    let doc = LoroDoc::new();
    let text_a = doc.get_text("text_a");
    let text_b = doc.get_text("text_b");
    
    // Insert initial content
    text_a.insert(0, "abc").unwrap();
    text_b.insert(0, "xyz").unwrap();
    doc.commit();
    
    // Set up event subscription to track events with detailed diff info
    let events = Arc::new(Mutex::new(Vec::new()));
    let events_clone = events.clone();
    
    let _guard = doc.subscribe_root(Arc::new(move |event_batch| {
        let mut events_lock = events_clone.lock().unwrap();
        println!("\n=== Event Batch Received ===");
        println!("Number of events in batch: {}", event_batch.events.len());
        
        for (idx, event) in event_batch.events.iter().enumerate() {
            println!("\nEvent #{}", idx);
            let container_name = event.target.name().as_str().to_string();
            println!("Container: {}", container_name);
            println!("Container ID: {:?}", event.target);
            
            if let Some(text_diff) = event.diff.as_text() {
                println!("Text diff items: {}", text_diff.len());
                for (i, delta) in text_diff.iter().enumerate() {
                    println!("  Delta #{}: {:?}", i, delta);
                }
            }
            
            events_lock.push(container_name);
        }
        println!("=== End Event Batch ===\n");
    }));
    
    println!("\n--- Performing deletes ---");
    println!("Deleting position 0 from text_a (should remove 'a')");
    println!("Deleting position 1 from text_b (should remove 'y')");
    
    // These operations should generate separate events for each container
    text_a.delete(0, 1).unwrap();  // Delete 'a' from text_a
    text_b.delete(1, 1).unwrap();  // Delete 'y' from text_b
    doc.commit();
    
    // Verify the events
    let events_lock = events.lock().unwrap();
    
    // Count events per container
    let text_a_count = events_lock.iter().filter(|&name| name == "text_a").count();
    let text_b_count = events_lock.iter().filter(|&name| name == "text_b").count();
    
    println!("\n--- Event Summary ---");
    println!("Total events received: {}", events_lock.len());
    println!("text_a events: {}", text_a_count);
    println!("text_b events: {}", text_b_count);
    
    // Check final state
    println!("\n--- Final State ---");
    println!("text_a: '{}' (expected: 'bc')", text_a.to_string());
    println!("text_b: '{}' (expected: 'xz')", text_b.to_string());
    
    // Assertions
    assert_eq!(events_lock.len(), 2, "Expected exactly 2 events");
    assert_eq!(text_a_count, 1, "Expected exactly 1 event for text_a");
    assert_eq!(text_b_count, 1, "Expected exactly 1 event for text_b");
    assert_eq!(text_a.to_string(), "bc");
    assert_eq!(text_b.to_string(), "xz");
}

#[test]
fn test_event_hint_merge_bug_with_adjacent_positions() {
    let doc = LoroDoc::new();
    let text_a = doc.get_text("text_a");
    let text_b = doc.get_text("text_b");
    
    // Insert initial content - make the texts different to better see the issue
    text_a.insert(0, "12345").unwrap();
    text_b.insert(0, "abcde").unwrap();
    doc.commit();
    
    // Set up event subscription
    let events = Arc::new(Mutex::new(Vec::new()));
    let events_clone = events.clone();
    
    let _guard = doc.subscribe_root(Arc::new(move |event_batch| {
        let mut events_lock = events_clone.lock().unwrap();
        println!("\n=== Adjacent Position Test Event Batch ===");
        
        for event in event_batch.events.iter() {
            let container_name = event.target.name().as_str().to_string();
            
            if let Some(text_diff) = event.diff.as_text() {
                println!("Container: {} - Diff items: {}", container_name, text_diff.len());
                
                // Check if there are multiple delete operations that shouldn't be merged
                let mut delete_count = 0;
                for delta in text_diff.iter() {
                    println!("  {:?}", delta);
                    // Count delete operations in the delta
                    if format!("{:?}", delta).contains("delete") {
                        delete_count += 1;
                    }
                }
                
                events_lock.push((container_name, text_diff.len(), delete_count));
            }
        }
    }));
    
    // Perform deletions at adjacent positions
    // If EventHints are incorrectly merged across containers, 
    // we might see wrong diff counts or positions
    text_a.delete(2, 1).unwrap();  // Delete '3' from position 2
    text_b.delete(3, 1).unwrap();  // Delete 'd' from position 3
    doc.commit();
    
    let events_lock = events.lock().unwrap();
    
    // Each container should have exactly one event with its own delete operation
    for (container_name, diff_items, delete_ops) in events_lock.iter() {
        println!("Container: {}, Diff items: {}, Delete ops: {}", container_name, diff_items, delete_ops);
    }
    
    assert_eq!(events_lock.len(), 2, "Should have exactly 2 events");
    assert_eq!(text_a.to_string(), "1245");
    assert_eq!(text_b.to_string(), "abce");
}

#[test]
fn test_event_hint_bug_contiguous_deletes() {
    // This test attempts to trigger the EventHint merge bug by creating
    // delete operations that appear contiguous when viewed as raw positions
    let doc = LoroDoc::new();
    let text_a = doc.get_text("text_a");
    let text_b = doc.get_text("text_b");
    
    // Insert content such that positions in different containers
    // might appear contiguous to the EventHint merge logic
    text_a.insert(0, "AAAA").unwrap();
    text_b.insert(0, "BBBB").unwrap();
    doc.commit();
    
    let events = Arc::new(Mutex::new(Vec::new()));
    let events_clone = events.clone();
    
    let _guard = doc.subscribe_root(Arc::new(move |event_batch| {
        let mut events_lock = events_clone.lock().unwrap();
        println!("\n=== Contiguous Delete Test ===");
        println!("Event batch size: {}", event_batch.events.len());
        
        for event in event_batch.events.iter() {
            let container_name = event.target.name().as_str().to_string();
            
            if let Some(text_diff) = event.diff.as_text() {
                let mut total_deleted = 0;
                for delta in text_diff.iter() {
                    // Count delete operations in the TextDelta
                    let delta_str = format!("{:?}", delta);
                    if delta_str.contains("Delete") || delta_str.contains("delete") {
                        // Extract the delete count from the debug string
                        if let Some(delete_pos) = delta_str.find("delete: ") {
                            let delete_str = &delta_str[delete_pos + 8..];
                            if let Some(end) = delete_str.find(' ').or_else(|| delete_str.find('}')) {
                                if let Ok(count) = delete_str[..end].parse::<usize>() {
                                    total_deleted += count;
                                }
                            }
                        }
                    }
                }
                
                println!("Container: {} - Total deleted: {} chars", container_name, total_deleted);
                events_lock.push((container_name, total_deleted));
            }
        }
    }));
    
    // Delete multiple characters from each text in a way that might
    // trigger EventHint merging if container IDs are not considered
    text_a.delete(1, 2).unwrap();  // Delete "AA" from middle
    text_b.delete(0, 2).unwrap();  // Delete "BB" from start
    doc.commit();
    
    let events_lock = events.lock().unwrap();
    
    // Verify we got separate events
    assert_eq!(events_lock.len(), 2, "Should have 2 separate events");
    
    // Verify each container deleted the correct amount
    let text_a_deletes: usize = events_lock
        .iter()
        .filter(|(name, _)| name == "text_a")
        .map(|(_, deletes)| *deletes)
        .sum();
    
    let text_b_deletes: usize = events_lock
        .iter()
        .filter(|(name, _)| name == "text_b")
        .map(|(_, deletes)| *deletes)
        .sum();
    
    assert_eq!(text_a_deletes, 2, "text_a should have deleted 2 chars");
    assert_eq!(text_b_deletes, 2, "text_b should have deleted 2 chars");
    
    // If the bug exists, we might see one container reporting 4 deletes
    // and the other reporting 0, or some other incorrect distribution
    for (container_name, delete_count) in events_lock.iter() {
        assert!(
            *delete_count == 2,
            "Container {} reported {} deletes, expected 2",
            container_name,
            delete_count
        );
    }
}

#[test]
fn test_event_hint_bug_multiple_containers_single_txn() {
    // Test with multiple text containers modified in a single transaction
    let doc = LoroDoc::new();
    let texts: Vec<_> = (0..5)
        .map(|i| {
            let text = doc.get_text(format!("text_{}", i));
            text.insert(0, &format!("{}{}{}", i, i, i)).unwrap();
            text
        })
        .collect();
    
    doc.commit();
    
    let events = Arc::new(Mutex::new(Vec::new()));
    let events_clone = events.clone();
    
    let _guard = doc.subscribe_root(Arc::new(move |event_batch| {
        let mut events_lock = events_clone.lock().unwrap();
        println!("\n=== Multiple Container Test ===");
        
        for event in event_batch.events.iter() {
            let container_name = event.target.name().as_str().to_string();
            events_lock.push(container_name);
        }
    }));
    
    // Delete from all containers in a single transaction
    for (i, text) in texts.iter().enumerate() {
        text.delete(i % 3, 1).unwrap();
    }
    doc.commit();
    
    let events_lock = events.lock().unwrap();
    
    // Should have exactly 5 events, one per container
    assert_eq!(events_lock.len(), 5, "Should have 5 separate events");
    
    // Verify each container got its own event
    for i in 0..5 {
        let container_name = format!("text_{}", i);
        let count = events_lock.iter().filter(|&name| name == &container_name).count();
        assert_eq!(count, 1, "Container {} should have exactly 1 event", container_name);
    }
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
    assert_eq!(events_lock.len(), 2, "Should have exactly 2 events, got {}", events_lock.len());
    
    // Each event should only contain operations for its own container
    let text_a_events: Vec<_> = events_lock.iter()
        .filter(|(name, _, _, _)| name == "text_a")
        .collect();
    let text_b_events: Vec<_> = events_lock.iter()
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
        println!("text_b operations - total: {}, deletes: {}, retains: {}", total_ops, delete_ops, retain_ops);
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

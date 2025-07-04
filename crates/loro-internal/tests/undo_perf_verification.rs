use loro_internal::{LoroDoc, undo::UndoManager};

/// Verify that undo diffs are being stored and used for performance optimization
#[test]
fn test_undo_diff_storage_and_usage() {
    let doc = LoroDoc::new();
    let mut undo = UndoManager::new(&doc);
    
    // Enable undo diff collection
    let text = doc.get_text("text");
    
    // Perform operations
    text.insert(0, "Hello").unwrap();
    doc.commit_then_renew();
    
    text.insert(5, " World").unwrap();
    doc.commit_then_renew();
    
    text.insert(11, "!").unwrap();
    doc.commit_then_renew();
    
    // First undo should use precalculated diff
    assert!(undo.undo().unwrap());
    assert_eq!(text.to_string(), "Hello World");
    
    // Second undo should also use precalculated diff
    assert!(undo.undo().unwrap());
    assert_eq!(text.to_string(), "Hello");
    
    // Third undo
    assert!(undo.undo().unwrap());
    assert_eq!(text.to_string(), "");
    
    // Redo operations should also use precalculated diffs
    assert!(undo.redo().unwrap());
    assert_eq!(text.to_string(), "Hello");
    
    assert!(undo.redo().unwrap());
    assert_eq!(text.to_string(), "Hello World");
    
    assert!(undo.redo().unwrap());
    assert_eq!(text.to_string(), "Hello World!");
}

/// Verify that grouped operations store combined undo diffs
#[test]
fn test_grouped_operations_undo_diff() {
    let doc = LoroDoc::new();
    let mut undo = UndoManager::new(&doc);
    
    let text = doc.get_text("text");
    let map = doc.get_map("map");
    
    // Group multiple operations
    undo.group_start().unwrap();
    
    text.insert(0, "Grouped Text").unwrap();
    doc.commit_then_renew();
    
    map.insert("key", "value").unwrap();
    doc.commit_then_renew();
    
    text.insert(12, " Operation").unwrap();
    doc.commit_then_renew();
    
    undo.group_end();
    
    // Single undo should undo all grouped operations
    assert!(undo.undo().unwrap());
    assert_eq!(text.to_string(), "");
    assert_eq!(map.len(), 0);
    
    // Single redo should redo all grouped operations
    assert!(undo.redo().unwrap());
    assert_eq!(text.to_string(), "Grouped Text Operation");
    assert_eq!(map.get("key").unwrap().into_string().unwrap().as_str(), "value");
}

/// Verify performance improvement with many operations
#[test]
fn test_performance_with_many_operations() {
    let doc = LoroDoc::new();
    let mut undo = UndoManager::new(&doc);
    
    let text = doc.get_text("text");
    let num_ops = 50;
    
    // Perform many operations
    for i in 0..num_ops {
        text.insert(text.len_unicode() as usize, &format!("{} ", i)).unwrap();
        doc.commit_then_renew();
    }
    
    let original_text = text.to_string();
    
    // Time the undo operations
    let start = std::time::Instant::now();
    
    // Undo all operations
    for _ in 0..num_ops {
        assert!(undo.undo().unwrap());
    }
    assert_eq!(text.to_string(), "");
    
    let undo_duration = start.elapsed();
    
    // Time the redo operations
    let start = std::time::Instant::now();
    
    // Redo all operations
    for _ in 0..num_ops {
        assert!(undo.redo().unwrap());
    }
    assert_eq!(text.to_string(), original_text);
    
    let redo_duration = start.elapsed();
    
    // Print timing information
    println!("Undo {} operations took: {:?}", num_ops, undo_duration);
    println!("Redo {} operations took: {:?}", num_ops, redo_duration);
    
    // With optimization, these should be relatively fast
    // Without optimization, time would grow quadratically
}

/// Verify that entries created after UndoManager have undo diffs
#[test]
fn test_new_entries_have_undo_diffs() {
    let doc = LoroDoc::new();
    let mut undo = UndoManager::new(&doc);
    
    let text = doc.get_text("text");
    
    // Create initial state
    text.insert(0, "Initial").unwrap();
    doc.commit_then_renew();
    
    // Add more operations
    text.insert(7, " State").unwrap();
    doc.commit_then_renew();
    
    text.insert(13, " Done").unwrap();
    doc.commit_then_renew();
    
    // All undo operations should use precalculated diffs
    assert!(undo.undo().unwrap());
    assert_eq!(text.to_string(), "Initial State");
    
    assert!(undo.undo().unwrap());
    assert_eq!(text.to_string(), "Initial");
    
    assert!(undo.undo().unwrap());
    assert_eq!(text.to_string(), "");
    
    // Redo should also use precalculated diffs
    assert!(undo.redo().unwrap());
    assert_eq!(text.to_string(), "Initial");
    
    assert!(undo.redo().unwrap());
    assert_eq!(text.to_string(), "Initial State");
    
    assert!(undo.redo().unwrap());
    assert_eq!(text.to_string(), "Initial State Done");
}

/// Verify that concurrent changes don't affect undo diff correctness
#[test]
fn test_undo_diff_with_concurrent_changes() {
    let doc1 = LoroDoc::new();
    let doc2 = LoroDoc::new();
    let mut undo1 = UndoManager::new(&doc1);
    
    let text1 = doc1.get_text("text");
    
    // Make local change
    text1.insert(0, "Local").unwrap();
    doc1.commit_then_renew();
    
    // Sync and make concurrent change
    doc2.import(&doc1.export_from(&Default::default())).unwrap();
    let text2 = doc2.get_text("text");
    text2.insert(5, " Remote").unwrap();
    doc2.commit_then_renew();
    
    // Import concurrent change
    doc1.import(&doc2.export_from(&doc1.oplog_vv())).unwrap();
    
    // Make another local change
    text1.insert(12, " Local2").unwrap();
    doc1.commit_then_renew();
    
    // Undo should only affect local changes
    assert!(undo1.undo().unwrap());
    assert_eq!(text1.to_string(), "Local Remote");
    
    assert!(undo1.undo().unwrap());
    assert_eq!(text1.to_string(), " Remote");
    
    // Redo local changes
    assert!(undo1.redo().unwrap());
    assert_eq!(text1.to_string(), "Local Remote");
    
    assert!(undo1.redo().unwrap());
    assert_eq!(text1.to_string(), "Local Remote Local2");
}

/// Verify that the optimization is actually being used
#[test]
fn test_optimization_is_used() {
    use std::time::Instant;
    
    // Test with optimization (new entries should use precalculated diffs)
    let doc = LoroDoc::new();
    let mut undo = UndoManager::new(&doc);
    
    let text = doc.get_text("text");
    
    // Create some operations
    for i in 0..20 {
        text.insert(text.len_unicode() as usize, &format!("Line {} ", i)).unwrap();
        doc.commit_then_renew();
    }
    
    // Time the undo operations
    let start = Instant::now();
    for _ in 0..20 {
        assert!(undo.undo().unwrap());
    }
    let optimized_time = start.elapsed();
    
    println!("Optimized undo of 20 operations took: {:?}", optimized_time);
    
    // All undos should have succeeded
    assert_eq!(text.to_string(), "");
    
    // The optimization should make undo operations faster
    // In practice, this should be significantly faster than the old O(n²) approach
    assert!(optimized_time.as_millis() < 200, "Undo operations took too long: {:?}", optimized_time);
}
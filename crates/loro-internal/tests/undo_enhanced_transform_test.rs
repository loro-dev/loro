use loro_internal::{LoroDoc, undo::UndoManager};

#[test]
fn test_enhanced_transformation_debug() {
    // Simplified test to debug transformation
    let doc = LoroDoc::new();
    let mut undo = UndoManager::new(&doc);
    
    let text = doc.get_text("text");
    
    // Start with "ABC"
    text.insert(0, "ABC").unwrap();
    doc.commit_then_renew();
    eprintln!("After insert ABC: '{}'", text.to_string());
    
    // Delete "B" at position 1
    text.delete(1, 1).unwrap();
    doc.commit_then_renew();
    eprintln!("After delete B: '{}'", text.to_string());
    
    // Simulate remote delete of "A"
    let doc2 = LoroDoc::new();
    doc2.import(&doc.export_from(&Default::default())).unwrap();
    let text2 = doc2.get_text("text");
    text2.delete(0, 1).unwrap(); // Delete "A"
    doc2.commit_then_renew();
    
    // Import remote changes
    doc.import(&doc2.export_from(&doc.oplog_vv())).unwrap();
    eprintln!("After importing remote delete of A: '{}'", text.to_string());
    
    // Now we should have "C"
    assert_eq!(text.to_string(), "C");
    
    // Undo local delete of "B"
    // Since "A" was deleted by remote, position shifts
    eprintln!("=== About to undo ===");
    assert!(undo.undo().unwrap());
    eprintln!("After undo: '{}'", text.to_string());
    
    // We should now have "BC" (B restored after C)
    assert_eq!(text.to_string(), "BC");
}
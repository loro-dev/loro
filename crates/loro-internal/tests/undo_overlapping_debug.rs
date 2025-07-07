use loro_internal::{LoroDoc, undo::UndoManager};

#[test]
fn test_overlapping_deletes_debug() {
    // Create two docs that will diverge
    let doc1 = LoroDoc::new();
    doc1.set_peer_id(1).unwrap();
    let mut undo = UndoManager::new(&doc1);
    
    let doc2 = LoroDoc::new();
    doc2.set_peer_id(2).unwrap();
    
    // Both start with "ABCDEFGHIJ"
    let text1 = doc1.get_text("text");
    text1.insert(0, "ABCDEFGHIJ").unwrap();
    doc1.commit_then_renew();
    
    // Sync initial state to doc2
    doc2.import(&doc1.export_from(&Default::default())).unwrap();
    let text2 = doc2.get_text("text");
    
    eprintln!("Initial state for both: '{}'", text1.to_string());
    
    // Now they diverge:
    // Doc1 deletes "DEF" (position 3, length 3)
    text1.delete(3, 3).unwrap();
    doc1.commit_then_renew();
    eprintln!("Doc1 after delete DEF: '{}'", text1.to_string());
    // Doc1 has: "ABCGHIJ"
    
    // Doc2 deletes "BCD" (position 1, length 3)
    text2.delete(1, 3).unwrap();
    doc2.commit_then_renew();
    eprintln!("Doc2 after delete BCD: '{}'", text2.to_string());
    // Doc2 has: "AEFGHIJ"
    
    // Import doc2's changes into doc1
    doc1.import(&doc2.export_from(&doc1.oplog_vv())).unwrap();
    eprintln!("After merge: '{}'", text1.to_string());
    
    // After merge: both deletes applied, should have "AGHIJ"
    assert_eq!(text1.to_string(), "AGHIJ");
    
    eprintln!("\n=== Analyzing the situation ===");
    eprintln!("Original: ABCDEFGHIJ");
    eprintln!("Local deleted: DEF at position 3");
    eprintln!("Remote deleted: BCD at position 1");
    eprintln!("Overlap: D was deleted by both");
    eprintln!("Current state: AGHIJ");
    eprintln!("When we undo local delete of DEF:");
    eprintln!("- D was already deleted by remote, can't restore");
    eprintln!("- E and F should be restored");
    eprintln!("- But where? After A, giving us AEFGHIJ");
    
    // Undo local delete of "DEF" - but "D" was already deleted by remote
    eprintln!("\n=== Performing undo ===");
    assert!(undo.undo().unwrap());
    eprintln!("After undo: '{}'", text1.to_string());
    
    // The correct result should be "AEFGHIJ"
    // But the current implementation gives "AHIJ"
}
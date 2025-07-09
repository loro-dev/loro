use loro::{LoroDoc, UndoManager, ToJson};
use serde_json::json;

#[test]
fn test_richtext_undo_simple() {
    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    let mut undo_manager = UndoManager::new(&doc);
    
    // Test the exact sequence from the failing test (without the sync part)
    let text = doc.get_text("text");
    
    // Step 1: Insert "B"
    text.insert(0, "B").unwrap();
    doc.commit();
    println!("After insert B: {:?}", text.get_richtext_value().to_json_value());
    
    // Step 2: Mark "B" as bold
    text.mark(0..1, "bold", true).unwrap();
    doc.commit();
    println!("After mark bold: {:?}", text.get_richtext_value().to_json_value());
    
    // Step 3: Insert "Hello "
    text.insert(0, "Hello ").unwrap();
    doc.commit();
    println!("After insert Hello: {:?}", text.get_richtext_value().to_json_value());
    
    // Step 4: Delete "Hello " (simulate the concurrent operation)
    text.delete(0, 6).unwrap();
    doc.commit();
    println!("After delete Hello: {:?}", text.get_richtext_value().to_json_value());
    
    // Step 5: Undo - this should remove the bold attribute
    undo_manager.undo().unwrap();
    println!("After first undo: {:?}", text.get_richtext_value().to_json_value());
    
    // Check if the bold attribute is removed
    let expected = json!([{"insert": "B"}]);
    let actual = text.get_richtext_value().to_json_value();
    
    assert_eq!(actual, expected, "Bold attribute should be removed after undo");
}

#[test]
fn test_richtext_undo_exact_sequence() {
    // This is the exact sequence from the failing test
    let doc_a = LoroDoc::new();
    doc_a.set_peer_id(1).unwrap();
    let mut undo_a = UndoManager::new(&doc_a);
    let doc_b = LoroDoc::new();
    doc_b.set_peer_id(2).unwrap();
    
    // Initial insert "B" in doc_a
    let text_a = doc_a.get_text("text");
    text_a.insert(0, "B").unwrap();
    doc_a.commit();
    
    // Mark "B" as bold in doc_a
    text_a.mark(0..1, "bold", true).unwrap();
    doc_a.commit();
    
    text_a.insert(0, "Hello ").unwrap();
    doc_a.commit();
    
    // Sync doc_a to doc_b (simulate the sync step)
    let sync = |a: &LoroDoc, b: &LoroDoc| {
        a.import(&b.export_from(&a.oplog_vv())).unwrap();
        b.import(&a.export_from(&b.oplog_vv())).unwrap();
    };
    
    sync(&doc_a, &doc_b);
    
    // Concurrently delete "Hello " in doc_b
    let text_b = doc_b.get_text("text");
    text_b.delete(0, 6).unwrap();
    doc_b.commit();
    
    // Sync back to doc_a
    sync(&doc_a, &doc_b);
    
    // Check the state after concurrent operations
    println!("After concurrent operations: {:?}", text_a.get_richtext_value().to_json_value());
    assert_eq!(
        text_a.get_richtext_value().to_json_value(),
        json!([
            {"insert": "B", "attributes": {"bold": true}}
        ])
    );
    
    // This should remove the bold attribute
    undo_a.undo().unwrap();
    println!("After undo: {:?}", text_a.get_richtext_value().to_json_value());
    
    assert_eq!(
        text_a.get_richtext_value().to_json_value(),
        json!([
            {"insert": "B"}
        ])
    );
}
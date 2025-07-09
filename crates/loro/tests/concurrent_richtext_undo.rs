use loro::{LoroDoc, UndoManager, ToJson};
use serde_json::json;

#[test]
fn test_concurrent_richtext_undo() {
    let doc_a = LoroDoc::new();
    doc_a.set_peer_id(1).unwrap();
    let mut undo_a = UndoManager::new(&doc_a);
    let doc_b = LoroDoc::new();
    doc_b.set_peer_id(2).unwrap();
    
    // Initial insert "B" in doc_a
    let text_a = doc_a.get_text("text");
    text_a.insert(0, "B").unwrap();
    doc_a.commit();
    println!("1. After insert B: {:?}", text_a.get_richtext_value().to_json_value());
    
    // Mark "B" as bold in doc_a
    text_a.mark(0..1, "bold", true).unwrap();
    doc_a.commit();
    println!("2. After mark bold: {:?}", text_a.get_richtext_value().to_json_value());
    
    text_a.insert(0, "Hello ").unwrap();
    doc_a.commit();
    println!("3. After insert Hello: {:?}", text_a.get_richtext_value().to_json_value());
    
    // Sync doc_a to doc_b (simulate the sync step)
    let sync = |a: &LoroDoc, b: &LoroDoc| {
        let data_a = a.export_from(&b.oplog_vv());
        let data_b = b.export_from(&a.oplog_vv());
        if !data_a.is_empty() {
            b.import(&data_a).unwrap();
        }
        if !data_b.is_empty() {
            a.import(&data_b).unwrap();
        }
    };
    
    sync(&doc_a, &doc_b);
    println!("4. After sync: {:?}", text_a.get_richtext_value().to_json_value());
    
    // Concurrently delete "Hello " in doc_b
    let text_b = doc_b.get_text("text");
    text_b.delete(0, 6).unwrap();
    doc_b.commit();
    println!("5. After delete Hello in doc_b: {:?}", text_b.get_richtext_value().to_json_value());
    
    // Sync back to doc_a
    sync(&doc_a, &doc_b);
    println!("6. After sync back: {:?}", text_a.get_richtext_value().to_json_value());
    
    // Check the state after concurrent operations
    assert_eq!(
        text_a.get_richtext_value().to_json_value(),
        json!([
            {"insert": "B", "attributes": {"bold": true}}
        ])
    );
    
    // This should remove the bold attribute (undo the mark operation)
    println!("7. About to undo...");
    undo_a.undo().unwrap();
    println!("8. After undo: {:?}", text_a.get_richtext_value().to_json_value());
    
    // FAILING: This should remove the bold attribute but it doesn't
    assert_eq!(
        text_a.get_richtext_value().to_json_value(),
        json!([
            {"insert": "B"}
        ])
    );
}
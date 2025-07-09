use loro::{LoroDoc, UndoManager, ToJson};
use serde_json::json;

#[test]
fn test_simple_richtext_undo() {
    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    let mut undo_manager = UndoManager::new(&doc);
    
    let text = doc.get_text("text");
    
    // Step 1: Insert "B"
    text.insert(0, "B").unwrap();
    doc.commit();
    println!("After insert B: {:?}", text.get_richtext_value().to_json_value());
    
    // Step 2: Mark "B" as bold
    text.mark(0..1, "bold", true).unwrap();
    doc.commit();
    println!("After mark bold: {:?}", text.get_richtext_value().to_json_value());
    
    // Step 3: Undo the mark operation
    undo_manager.undo().unwrap();
    println!("After undo mark: {:?}", text.get_richtext_value().to_json_value());
    
    // The text should no longer be bold
    assert_eq!(
        text.get_richtext_value().to_json_value(),
        json!([{"insert": "B"}])
    );
    
    // Step 4: Undo the insert operation
    undo_manager.undo().unwrap();
    println!("After undo insert: {:?}", text.get_richtext_value().to_json_value());
    
    // The text should be empty
    assert_eq!(text.get_richtext_value().to_json_value(), json!([]));
}
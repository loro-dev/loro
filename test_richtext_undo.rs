use loro::{LoroDoc, UndoManager};
use serde_json::json;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let doc = LoroDoc::new();
    doc.set_peer_id(1)?;
    let mut undo_manager = UndoManager::new(&doc);
    
    // Test the exact sequence from the failing test
    let text = doc.get_text("text");
    
    // Step 1: Insert "B"
    text.insert(0, "B")?;
    doc.commit();
    println!("After insert B: {:?}", text.get_richtext_value().to_json_value());
    
    // Step 2: Mark "B" as bold
    text.mark(0..1, "bold", true)?;
    doc.commit();
    println!("After mark bold: {:?}", text.get_richtext_value().to_json_value());
    
    // Step 3: Insert "Hello "
    text.insert(0, "Hello ")?;
    doc.commit();
    println!("After insert Hello: {:?}", text.get_richtext_value().to_json_value());
    
    // Step 4: Delete "Hello " (simulate the concurrent operation)
    text.delete(0, 6)?;
    doc.commit();
    println!("After delete Hello: {:?}", text.get_richtext_value().to_json_value());
    
    // Step 5: Undo - this should remove the bold attribute
    undo_manager.undo()?;
    println!("After first undo: {:?}", text.get_richtext_value().to_json_value());
    
    // Check if the bold attribute is removed
    let expected = json!([{"insert": "B"}]);
    let actual = text.get_richtext_value().to_json_value();
    
    if actual == expected {
        println!("✓ Test passed: Bold attribute correctly removed");
    } else {
        println!("✗ Test failed:");
        println!("  Expected: {:?}", expected);
        println!("  Actual:   {:?}", actual);
    }
    
    Ok(())
}
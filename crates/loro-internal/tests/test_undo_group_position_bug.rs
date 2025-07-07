use loro_internal::{LoroDoc, UndoManager};

#[test]
fn test_undo_group_position_calculation_bug() {
    // This test demonstrates the issue with position calculations
    // when composing undo diffs for grouped operations
    
    let doc = LoroDoc::new();
    let mut undo = UndoManager::new(&doc);
    
    let text = doc.get_text("text");
    
    // Start a group
    undo.group_start().unwrap();
    
    // First operation: Insert "Hello"
    text.insert(0, "Hello").unwrap();
    doc.commit_then_renew();
    
    // Second operation: Insert " World" at position 5
    text.insert(5, " World").unwrap();
    doc.commit_then_renew();
    
    // Third operation: Delete "Hello " (6 chars from position 0)
    text.delete(0, 6).unwrap();
    doc.commit_then_renew();
    
    // End the group
    undo.group_end();
    
    // The document should now contain "World"
    assert_eq!(text.to_string(), "World");
    
    // Try to undo the grouped operations
    // This is where the bug occurs - position calculation is incorrect
    // when composing the undo diffs from multiple operations
    let result = undo.undo();
    
    // Without the fix, this would fail with OutOfBound error
    assert!(result.is_ok(), "Undo should succeed but got: {:?}", result);
    assert_eq!(text.to_string(), "", "After undo, text should be empty");
    
    // Try redo
    let result = undo.redo();
    assert!(result.is_ok(), "Redo should succeed but got: {:?}", result);
    assert_eq!(text.to_string(), "World", "After redo, text should be 'World'");
}

#[test]
fn test_complex_undo_group_with_multiple_containers() {
    // Test more complex scenario with multiple containers
    let doc = LoroDoc::new();
    let mut undo = UndoManager::new(&doc);
    
    let text1 = doc.get_text("text1");
    let text2 = doc.get_text("text2");
    
    // Start a group
    undo.group_start().unwrap();
    
    // Operations on text1
    text1.insert(0, "ABC").unwrap();
    doc.commit_then_renew();
    
    // Operations on text2
    text2.insert(0, "123").unwrap();
    doc.commit_then_renew();
    
    // More operations on text1
    text1.insert(3, "DEF").unwrap();
    doc.commit_then_renew();
    
    // Delete from text1
    text1.delete(1, 2).unwrap(); // Delete "BC", leaving "ADEF"
    doc.commit_then_renew();
    
    // More operations on text2
    text2.insert(3, "456").unwrap();
    doc.commit_then_renew();
    
    // End the group
    undo.group_end();
    
    assert_eq!(text1.to_string(), "ADEF");
    assert_eq!(text2.to_string(), "123456");
    
    // Undo the grouped operations
    let result = undo.undo();
    assert!(result.is_ok(), "Undo should succeed but got: {:?}", result);
    assert_eq!(text1.to_string(), "");
    assert_eq!(text2.to_string(), "");
    
    // Redo
    let result = undo.redo();
    assert!(result.is_ok(), "Redo should succeed but got: {:?}", result);
    
    // Debug the actual values after redo
    println!("After redo - text1: '{}' (expected: 'ADEF')", text1.to_string());
    println!("After redo - text2: '{}' (expected: '123456')", text2.to_string());
    
    assert_eq!(text1.to_string(), "ADEF");
    assert_eq!(text2.to_string(), "123456");
}
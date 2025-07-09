use loro::{LoroDoc, ToJson, UndoManager};

#[test]
fn verify_movable_list_undo_issue() {
    // Test 1: Simple move operation
    let doc = LoroDoc::new();
    let list = doc.get_movable_list("list");
    
    // Create [A, B, C]
    list.insert(0, "A").unwrap();
    list.insert(1, "B").unwrap();
    list.insert(2, "C").unwrap();
    doc.commit();
    
    let mut undo_manager = UndoManager::new(&doc);
    
    // Move C to beginning: [A, B, C] -> [C, A, B]
    list.mov(2, 0).unwrap();
    doc.commit();
    
    // Undo should restore: [C, A, B] -> [A, B, C]
    println!("Before undo: {:?}", list.get_deep_value().to_json());
    undo_manager.undo().unwrap();
    println!("After undo: {:?}", list.get_deep_value().to_json());
    
    let result = list.get_deep_value().to_json();
    assert_eq!(result, serde_json::json!(["A", "B", "C"]), "Undo failed to restore original order");
    
    // Test 2: Forward move
    let doc2 = LoroDoc::new();
    let list2 = doc2.get_movable_list("list2");
    
    // Create [0, 1, 2, 3]
    for i in 0..4 {
        list2.insert(i, i as i32).unwrap();
    }
    doc2.commit();
    
    let mut undo_manager2 = UndoManager::new(&doc2);
    
    // Move 0 to position 3: [0, 1, 2, 3] -> [1, 2, 3, 0]
    list2.mov(0, 3).unwrap();
    doc2.commit();
    
    // Undo should restore: [1, 2, 3, 0] -> [0, 1, 2, 3]
    undo_manager2.undo().unwrap();
    
    let result2 = list2.get_deep_value().to_json();
    assert_eq!(result2, serde_json::json!([0, 1, 2, 3]), "Undo failed for forward move");
    
    // Test 3: Complex scenario with multiple moves
    let doc3 = LoroDoc::new();
    let list3 = doc3.get_movable_list("list3");
    
    // Create [A, B, C, D]
    list3.insert(0, "A").unwrap();
    list3.insert(1, "B").unwrap();
    list3.insert(2, "C").unwrap();
    list3.insert(3, "D").unwrap();
    doc3.commit();
    
    let mut undo_manager3 = UndoManager::new(&doc3);
    
    // First move: D to position 1: [A, B, C, D] -> [A, D, B, C]
    list3.mov(3, 1).unwrap();
    doc3.commit();
    
    // Second move: A to position 3: [A, D, B, C] -> [D, B, C, A]
    list3.mov(0, 3).unwrap();
    doc3.commit();
    
    // Undo second move: [D, B, C, A] -> [A, D, B, C]
    undo_manager3.undo().unwrap();
    let intermediate = list3.get_deep_value().to_json();
    assert_eq!(intermediate, serde_json::json!(["A", "D", "B", "C"]), "First undo failed");
    
    // Undo first move: [A, D, B, C] -> [A, B, C, D]
    undo_manager3.undo().unwrap();
    let final_result = list3.get_deep_value().to_json();
    assert_eq!(final_result, serde_json::json!(["A", "B", "C", "D"]), "Second undo failed");
}
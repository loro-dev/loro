use loro::{ContainerTrait, LoroDoc, ToJson, UndoManager};

#[test]
fn debug_simple_movable_list_undo() {
    let doc = LoroDoc::new();
    let list = doc.get_movable_list("list");
    
    // Create a simple list with 3 elements
    list.insert(0, "A").unwrap();
    list.insert(1, "B").unwrap();
    list.insert(2, "C").unwrap();
    doc.commit();
    
    println!("Initial state: {:?}", list.get_deep_value().to_json());
    println!("Initial list length: {}", list.len());
    
    // Set up undo manager
    let mut undo_manager = UndoManager::new(&doc);
    
    // Perform a single move operation: Move "C" (index 2) to index 0
    println!("\nPerforming move: Moving element at index 2 to index 0");
    list.mov(2, 0).unwrap();
    doc.commit();
    
    println!("After move: {:?}", list.get_deep_value().to_json());
    println!("After move length: {}", list.len());
    
    // Try to undo
    println!("\nPerforming undo...");
    let can_undo = undo_manager.can_undo();
    println!("Can undo: {}", can_undo);
    
    if can_undo {
        let undo_result = undo_manager.undo();
        println!("Undo successful: {:?}", undo_result);
    }
    
    println!("After undo: {:?}", list.get_deep_value().to_json());
    println!("After undo length: {}", list.len());
    
    // Expected: ["A", "B", "C"]
    // Check if undo worked correctly
    let final_state = list.get_deep_value().to_json();
    let expected = serde_json::json!(["A", "B", "C"]);
    
    println!("\nExpected: {:?}", expected);
    println!("Actual: {:?}", final_state);
    println!("Undo worked correctly: {}", final_state == expected);
    
    assert_eq!(final_state, expected, "Undo should restore original order");
}

#[test]
fn debug_movable_list_undo_with_events() {
    let doc = LoroDoc::new();
    let list = doc.get_movable_list("list");
    
    // Create a simple list
    list.insert(0, "X").unwrap();
    list.insert(1, "Y").unwrap();
    list.insert(2, "Z").unwrap();
    doc.commit();
    
    println!("Initial: {:?}", list.get_deep_value().to_json());
    
    // Set up undo manager and track events
    let mut undo_manager = UndoManager::new(&doc);
    
    // Subscribe to events to see what's happening
    let events = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let events_clone = events.clone();
    doc.subscribe(
        &doc.get_movable_list("list").id(),
        std::sync::Arc::new(move |event| {
            events_clone.lock().unwrap().push(format!("{:?}", event));
        }),
    );
    
    // Move Z to the beginning
    println!("\nMoving Z (index 2) to index 0");
    list.mov(2, 0).unwrap();
    doc.commit();
    
    println!("After move: {:?}", list.get_deep_value().to_json());
    
    // Print events
    println!("\nEvents during move:");
    for event in events.lock().unwrap().iter() {
        println!("  {}", event);
    }
    events.lock().unwrap().clear();
    
    // Undo
    println!("\nUndoing...");
    undo_manager.undo();
    
    println!("After undo: {:?}", list.get_deep_value().to_json());
    
    // Print events during undo
    println!("\nEvents during undo:");
    for event in events.lock().unwrap().iter() {
        println!("  {}", event);
    }
    
    let final_state = list.get_deep_value().to_json();
    assert_eq!(final_state, serde_json::json!(["X", "Y", "Z"]));
}

#[test]
fn debug_movable_list_multiple_moves() {
    let doc = LoroDoc::new();
    let list = doc.get_movable_list("list");
    
    // Create list [0, 1, 2, 3]
    for i in 0..4 {
        list.insert(i, i as i32).unwrap();
    }
    doc.commit();
    
    println!("Initial: {:?}", list.get_deep_value().to_json());
    
    let mut undo_manager = UndoManager::new(&doc);
    
    // Move 3 to index 1: [0, 1, 2, 3] -> [0, 3, 1, 2]
    println!("\nMove 1: Moving element at index 3 to index 1");
    list.mov(3, 1).unwrap();
    doc.commit();
    println!("After move 1: {:?}", list.get_deep_value().to_json());
    
    // Move 0 to index 3: [0, 3, 1, 2] -> [3, 1, 2, 0]
    println!("\nMove 2: Moving element at index 0 to index 3");
    list.mov(0, 3).unwrap();
    doc.commit();
    println!("After move 2: {:?}", list.get_deep_value().to_json());
    
    // Undo the second move
    println!("\nUndo move 2");
    undo_manager.undo();
    println!("After undo 1: {:?}", list.get_deep_value().to_json());
    println!("Expected: [0, 3, 1, 2]");
    
    // Undo the first move
    println!("\nUndo move 1");
    undo_manager.undo();
    println!("After undo 2: {:?}", list.get_deep_value().to_json());
    println!("Expected: [0, 1, 2, 3]");
    
    let final_state = list.get_deep_value().to_json();
    assert_eq!(final_state, serde_json::json!([0, 1, 2, 3]));
}

#[test]
fn debug_movable_list_positions() {
    // Test to understand how positions work in MovableList
    let doc = LoroDoc::new();
    let list = doc.get_movable_list("list");
    
    // Insert elements and check their positions
    let a = list.insert(0, "A").unwrap();
    let b = list.insert(1, "B").unwrap();
    let c = list.insert(2, "C").unwrap();
    doc.commit();
    
    println!("Initial setup:");
    println!("  A id: {:?}", a);
    println!("  B id: {:?}", b);
    println!("  C id: {:?}", c);
    println!("  List: {:?}", list.get_deep_value().to_json());
    
    // Get the actual element IDs
    let elements: Vec<_> = (0..list.len()).map(|i| list.get(i)).collect();
    println!("\nElement IDs by index:");
    for (i, elem) in elements.iter().enumerate() {
        println!("  Index {}: {:?}", i, elem);
    }
    
    // Move C to the beginning
    println!("\nMoving C (index 2) to index 0");
    list.mov(2, 0).unwrap();
    doc.commit();
    
    println!("After move: {:?}", list.get_deep_value().to_json());
    
    // Check positions again
    let elements_after: Vec<_> = (0..list.len()).map(|i| list.get(i)).collect();
    println!("\nElement IDs by index after move:");
    for (i, elem) in elements_after.iter().enumerate() {
        println!("  Index {}: {:?}", i, elem);
    }
}
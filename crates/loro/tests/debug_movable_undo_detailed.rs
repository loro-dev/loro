use loro::{LoroDoc, ToJson, UndoManager};

#[test]
fn debug_movable_list_undo_detailed() {
    let doc = LoroDoc::new();
    let list = doc.get_movable_list("list");
    
    // Create a simple list
    list.insert(0, "A").unwrap();
    list.insert(1, "B").unwrap();
    list.insert(2, "C").unwrap();
    doc.commit();
    
    println!("=== Initial State ===");
    print_list_state(&list);
    
    // Set up undo manager
    let mut undo_manager = UndoManager::new(&doc);
    
    // Check initial undo stack
    println!("\n=== Before Move ===");
    println!("Can undo: {}", undo_manager.can_undo());
    println!("Can redo: {}", undo_manager.can_redo());
    
    // Perform move: C to beginning
    println!("\n=== Performing Move ===");
    println!("Moving element at index 2 to index 0");
    list.mov(2, 0).unwrap();
    doc.commit();
    
    print_list_state(&list);
    
    // Check undo stack after move
    println!("\n=== After Move ===");
    println!("Can undo: {}", undo_manager.can_undo());
    println!("Can redo: {}", undo_manager.can_redo());
    
    // Try to undo
    println!("\n=== Performing Undo ===");
    match undo_manager.undo() {
        Ok(success) => println!("Undo returned: {}", success),
        Err(e) => println!("Undo error: {:?}", e),
    }
    
    print_list_state(&list);
    
    // Check if we can redo
    println!("\n=== After Undo ===");
    println!("Can undo: {}", undo_manager.can_undo());
    println!("Can redo: {}", undo_manager.can_redo());
    
    // Try redo to see if it changes anything
    if undo_manager.can_redo() {
        println!("\n=== Performing Redo ===");
        match undo_manager.redo() {
            Ok(success) => println!("Redo returned: {}", success),
            Err(e) => println!("Redo error: {:?}", e),
        }
        print_list_state(&list);
    }
}

fn print_list_state(list: &loro::LoroMovableList) {
    let json_value = list.get_deep_value().to_json();
    println!("List content: {:?}", json_value);
    println!("List length: {}", list.len());
    
    // Print individual elements
    print!("Elements by index: ");
    for i in 0..list.len() {
        if let Some(val) = list.get(i) {
            print!("[{}]={:?} ", i, val);
        }
    }
    println!();
}

#[test]
fn debug_movable_list_vs_regular_list() {
    // Compare MovableList behavior with regular List
    println!("=== Testing Regular List ===");
    {
        let doc = LoroDoc::new();
        let list = doc.get_list("regular_list");
        
        list.insert(0, "A").unwrap();
        list.insert(1, "B").unwrap();
        list.insert(2, "C").unwrap();
        doc.commit();
        
        println!("Initial: {:?}", list.get_deep_value().to_json());
        
        let mut undo_manager = UndoManager::new(&doc);
        
        // Move C to beginning using delete + insert
        list.delete(2, 1).unwrap();
        list.insert(0, "C").unwrap();
        doc.commit();
        
        println!("After move: {:?}", list.get_deep_value().to_json());
        
        undo_manager.undo().unwrap();
        println!("After undo: {:?}", list.get_deep_value().to_json());
    }
    
    println!("\n=== Testing MovableList ===");
    {
        let doc = LoroDoc::new();
        let list = doc.get_movable_list("movable_list");
        
        list.insert(0, "A").unwrap();
        list.insert(1, "B").unwrap();
        list.insert(2, "C").unwrap();
        doc.commit();
        
        println!("Initial: {:?}", list.get_deep_value().to_json());
        
        let mut undo_manager = UndoManager::new(&doc);
        
        // Move C to beginning
        list.mov(2, 0).unwrap();
        doc.commit();
        
        println!("After move: {:?}", list.get_deep_value().to_json());
        
        undo_manager.undo().unwrap();
        println!("After undo: {:?}", list.get_deep_value().to_json());
    }
}

#[test]
fn debug_movable_list_operations() {
    // Test individual operations to understand behavior
    let doc = LoroDoc::new();
    let list = doc.get_movable_list("list");
    
    println!("=== Testing Insert Operations ===");
    list.insert(0, 10).unwrap();
    println!("After insert(0, 10): {:?}", list.get_deep_value().to_json());
    
    list.insert(1, 20).unwrap();
    println!("After insert(1, 20): {:?}", list.get_deep_value().to_json());
    
    list.insert(2, 30).unwrap();
    println!("After insert(2, 30): {:?}", list.get_deep_value().to_json());
    
    doc.commit();
    
    println!("\n=== Testing Move Operation ===");
    println!("Before move: {:?}", list.get_deep_value().to_json());
    list.mov(2, 0).unwrap();
    println!("After mov(2, 0): {:?}", list.get_deep_value().to_json());
    
    // Try another move
    list.mov(1, 2).unwrap();
    println!("After mov(1, 2): {:?}", list.get_deep_value().to_json());
    
    doc.commit();
}

#[test]
fn debug_undo_with_multiple_containers() {
    // Test if undo works with other container types
    let doc = LoroDoc::new();
    let movable = doc.get_movable_list("movable");
    let regular = doc.get_list("regular");
    
    // Set up initial state
    movable.insert(0, "M1").unwrap();
    movable.insert(1, "M2").unwrap();
    regular.insert(0, "R1").unwrap();
    regular.insert(1, "R2").unwrap();
    doc.commit();
    
    println!("Initial movable: {:?}", movable.get_deep_value().to_json());
    println!("Initial regular: {:?}", regular.get_deep_value().to_json());
    
    let mut undo_manager = UndoManager::new(&doc);
    
    // Make changes
    movable.mov(1, 0).unwrap();
    regular.insert(2, "R3").unwrap();
    doc.commit();
    
    println!("\nAfter changes:");
    println!("Movable: {:?}", movable.get_deep_value().to_json());
    println!("Regular: {:?}", regular.get_deep_value().to_json());
    
    // Undo
    undo_manager.undo().unwrap();
    
    println!("\nAfter undo:");
    println!("Movable: {:?}", movable.get_deep_value().to_json());
    println!("Regular: {:?}", regular.get_deep_value().to_json());
}
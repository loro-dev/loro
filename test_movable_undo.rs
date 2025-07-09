use loro::{LoroDoc, UndoManager};

fn main() {
    let doc = LoroDoc::new();
    let list = doc.get_movable_list("list");
    let mut undo = UndoManager::new(&doc);
    
    // Initial state
    list.insert(0, "0").unwrap();
    list.insert(1, "1").unwrap(); 
    list.insert(2, "2").unwrap();
    doc.commit();
    
    println!("Initial state: {:?}", list.get_value());
    println!("List container ID: {:?}", list.id());
    
    // Move operation
    list.mov(0, 2).unwrap();
    doc.commit();
    
    println!("After move: {:?}", list.get_value());
    
    // Subscribe to diff events
    let _sub = doc.subscribe_undo_diffs(Box::new(|diff| {
        println!("Undo diff received: {:#?}", diff);
        true
    }));
    
    // Undo
    println!("Attempting undo...");
    undo.undo().unwrap();
    
    println!("After undo: {:?}", list.get_value());
}
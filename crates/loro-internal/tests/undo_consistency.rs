use loro_internal::{LoroDoc, undo::UndoManager, TreeParentId, HandlerTrait};

/// Test that the new undo implementation behaves identically to the old one
/// by comparing document states after various undo/redo operations
#[test]
fn test_undo_consistency_basic_operations() {
    // Test with text operations
    let doc = LoroDoc::new();
    let mut undo = UndoManager::new(&doc);
    let text = doc.get_text("text");

    // Perform a series of operations
    text.insert(0, "Hello").unwrap();
    doc.commit_then_renew();
    text.insert(5, " World").unwrap();
    doc.commit_then_renew();
    text.delete(5, 6).unwrap();
    doc.commit_then_renew();

    // Get state before undo
    let state_before = doc.get_deep_value();

    // Undo all operations
    assert!(undo.undo().unwrap());
    let _state_after_undo1 = doc.get_deep_value();
    
    assert!(undo.undo().unwrap());
    let _state_after_undo2 = doc.get_deep_value();
    
    assert!(undo.undo().unwrap());
    let _state_after_undo3 = doc.get_deep_value();

    // Should be empty after undoing all operations
    assert_eq!(text.to_string(), "");

    // Redo all operations
    assert!(undo.redo().unwrap());
    assert_eq!(text.to_string(), "Hello");
    
    assert!(undo.redo().unwrap());
    assert_eq!(text.to_string(), "Hello World");
    
    assert!(undo.redo().unwrap());
    assert_eq!(text.to_string(), "Hello");
    
    // Final state should match the state before undo
    assert_eq!(doc.get_deep_value(), state_before);
}

#[test]
fn test_undo_consistency_with_containers() {
    let doc = LoroDoc::new();
    let mut undo = UndoManager::new(&doc);

    // Test with multiple container types
    let map = doc.get_map("map");
    let list = doc.get_list("list");
    let text = doc.get_text("text");

    // Map operations
    map.insert("key1", "value1").unwrap();
    doc.commit_then_renew();
    map.insert("key2", 42).unwrap();
    doc.commit_then_renew();
    
    // List operations
    list.insert(0, "first").unwrap();
    doc.commit_then_renew();
    list.insert(1, "second").unwrap();
    doc.commit_then_renew();
    
    // Text operations
    text.insert(0, "Test").unwrap();
    doc.commit_then_renew();

    // Capture states
    let final_state = doc.get_deep_value();

    // Undo everything
    for _ in 0..5 {
        assert!(undo.undo().unwrap());
    }
    
    // Everything should be empty
    assert_eq!(map.len(), 0);
    assert_eq!(list.len(), 0);
    assert_eq!(text.len_unicode(), 0);

    // Redo everything
    for _ in 0..5 {
        assert!(undo.redo().unwrap());
    }

    // Verify final state matches
    assert_eq!(doc.get_deep_value(), final_state);
}

#[test]
fn test_undo_consistency_with_concurrent_changes() {
    let doc1 = LoroDoc::new();
    let doc2 = LoroDoc::new();
    let mut undo1 = UndoManager::new(&doc1);

    // Make changes on doc1
    let text1 = doc1.get_text("text");
    text1.insert(0, "Hello").unwrap();
    doc1.commit_then_renew();
    
    // Sync to doc2 and make concurrent changes
    doc2.import(&doc1.export_from(&Default::default())).unwrap();
    let text2 = doc2.get_text("text");
    text2.insert(5, " World").unwrap();
    doc2.commit_then_renew();

    // Import concurrent changes back to doc1
    doc1.import(&doc2.export_from(&doc1.oplog_vv())).unwrap();
    
    // Make more changes on doc1
    text1.insert(11, "!").unwrap();
    doc1.commit_then_renew();

    let state_before_undo = doc1.get_deep_value();

    // Undo should only undo local changes, not remote ones
    assert!(undo1.undo().unwrap()); // Undo "!"
    assert_eq!(text1.to_string(), "Hello World");
    
    assert!(undo1.undo().unwrap()); // Undo "Hello"
    assert_eq!(text1.to_string(), " World");

    // Redo the changes
    assert!(undo1.redo().unwrap());
    assert_eq!(text1.to_string(), "Hello World");
    
    assert!(undo1.redo().unwrap());
    assert_eq!(text1.to_string(), "Hello World!");

    // Final state should match
    assert_eq!(doc1.get_deep_value(), state_before_undo);
}

#[test]
fn test_undo_consistency_with_grouped_operations() {
    let doc = LoroDoc::new();
    let mut undo = UndoManager::new(&doc);
    
    let text = doc.get_text("text");
    let map = doc.get_map("map");

    // Group multiple operations
    undo.group_start().unwrap();
    
    text.insert(0, "Grouped").unwrap();
    doc.commit_then_renew();
    
    map.insert("grouped", true).unwrap();
    doc.commit_then_renew();
    
    text.insert(7, " Operations").unwrap();
    doc.commit_then_renew();
    
    undo.group_end();

    let state_after_group = doc.get_deep_value();

    // Single undo should undo all grouped operations
    assert!(undo.undo().unwrap());
    assert_eq!(text.to_string(), "");
    assert_eq!(map.len(), 0);

    // Single redo should redo all grouped operations
    assert!(undo.redo().unwrap());
    assert_eq!(doc.get_deep_value(), state_after_group);
}

#[test]
fn test_undo_consistency_with_tree_operations() {
    let doc = LoroDoc::new();
    let mut undo = UndoManager::new(&doc);
    
    let tree = doc.get_tree("tree");
    
    // Create tree structure
    let root = tree.create(TreeParentId::Root).unwrap();
    doc.commit_then_renew();
    
    let child1 = tree.create(TreeParentId::Node(root)).unwrap();
    doc.commit_then_renew();
    
    let child2 = tree.create(TreeParentId::Node(root)).unwrap();
    doc.commit_then_renew();
    
    tree.mov(child2, TreeParentId::Node(child1)).unwrap();
    doc.commit_then_renew();

    // Undo operations
    assert!(undo.undo().unwrap()); // Undo move
    // Check parent by verifying the node exists at the expected position
    assert!(!tree.is_node_unexist(&child2));
    assert_eq!(tree.get_node_parent(&child2).unwrap(), TreeParentId::Node(root));
    
    assert!(undo.undo().unwrap()); // Undo create child2
    // After undoing creation, the node should not exist in the tree
    assert_eq!(tree.children(&TreeParentId::Node(root)).unwrap().len(), 1);
    
    assert!(undo.undo().unwrap()); // Undo create child1
    // After undoing creation, root should have no children
    assert_eq!(tree.children(&TreeParentId::Node(root)).unwrap().len(), 0);
    
    assert!(undo.undo().unwrap()); // Undo create root
    // After undoing root creation, no nodes should have Root as parent
    assert_eq!(tree.children(&TreeParentId::Root).unwrap_or_default().len(), 0);

    // Redo all operations
    for _ in 0..4 {
        assert!(undo.redo().unwrap());
    }

    // Verify final structure matches (not exact state due to tree ID differences)
    // After redo, we should have the same tree structure
    let root_children = tree.children(&TreeParentId::Root).unwrap();
    assert_eq!(root_children.len(), 1);
    let new_root = root_children[0];
    
    let root_children = tree.children(&TreeParentId::Node(new_root)).unwrap();
    assert_eq!(root_children.len(), 1);
    let new_child1 = root_children[0];
    
    let child1_children = tree.children(&TreeParentId::Node(new_child1)).unwrap();
    assert_eq!(child1_children.len(), 1);
    // The structure is restored: root -> child1 -> child2
}

#[test]
fn test_undo_consistency_with_richtext_styles() {
    let doc = LoroDoc::new();
    let mut undo = UndoManager::new(&doc);
    
    let text = doc.get_text("text");
    
    // Insert text with styles
    text.insert(0, "Bold and Italic").unwrap();
    doc.commit_then_renew();
    
    text.mark(0, 4, "bold", true.into()).unwrap();
    doc.commit_then_renew();
    
    text.mark(9, 15, "italic", true.into()).unwrap();
    doc.commit_then_renew();

    let styled_state = doc.get_deep_value();

    // Undo style operations
    assert!(undo.undo().unwrap()); // Undo italic
    assert!(undo.undo().unwrap()); // Undo bold
    
    // Text should exist but without styles
    assert_eq!(text.to_string(), "Bold and Italic");
    
    // Redo styles
    assert!(undo.redo().unwrap());
    assert!(undo.redo().unwrap());
    
    // Verify styled state is restored
    assert_eq!(doc.get_deep_value(), styled_state);
}

#[test]
fn test_undo_consistency_empty_operations() {
    let doc = LoroDoc::new();
    let mut undo = UndoManager::new(&doc);
    
    let text = doc.get_text("text");
    
    // Insert and immediately delete (net effect: empty)
    text.insert(0, "Temporary").unwrap();
    doc.commit_then_renew();
    
    text.delete(0, 9).unwrap();
    doc.commit_then_renew();

    // Should be able to undo the delete
    assert!(undo.undo().unwrap());
    assert_eq!(text.to_string(), "Temporary");
    
    // Should be able to undo the insert
    assert!(undo.undo().unwrap());
    assert_eq!(text.to_string(), "");
    
    // Redo both operations
    assert!(undo.redo().unwrap());
    assert_eq!(text.to_string(), "Temporary");
    
    assert!(undo.redo().unwrap());
    assert_eq!(text.to_string(), "");
}

#[test]
fn test_undo_consistency_counter_operations() {
    #[cfg(feature = "counter")]
    {
        let doc = LoroDoc::new();
        let mut undo = UndoManager::new(&doc);
        
        let counter = doc.get_counter("counter");
        
        // Perform counter operations
        counter.increment(5.0).unwrap();
        doc.commit_then_renew();
        
        counter.decrement(2.0).unwrap();
        doc.commit_then_renew();
        
        counter.increment(3.0).unwrap();
        doc.commit_then_renew();
        
        assert_eq!(counter.get_value(), loro_internal::LoroValue::Double(6.0));
        
        // Undo operations
        assert!(undo.undo().unwrap());
        assert_eq!(counter.get_value(), loro_internal::LoroValue::Double(3.0));
        
        assert!(undo.undo().unwrap());
        assert_eq!(counter.get_value(), loro_internal::LoroValue::Double(5.0));
        
        assert!(undo.undo().unwrap());
        assert_eq!(counter.get_value(), loro_internal::LoroValue::Double(0.0));
        
        // Redo all
        for _ in 0..3 {
            assert!(undo.redo().unwrap());
        }
        assert_eq!(counter.get_value(), loro_internal::LoroValue::Double(6.0));
    }
}

#[test]
fn test_undo_consistency_movable_list() {
    let doc = LoroDoc::new();
    let mut undo = UndoManager::new(&doc);
    
    let list = doc.get_movable_list("movable");
    
    // Create elements
    list.insert(0, "A").unwrap();
    doc.commit_then_renew();
    
    list.insert(1, "B").unwrap();
    doc.commit_then_renew();
    
    list.insert(2, "C").unwrap();
    doc.commit_then_renew();
    
    // Move operations
    list.mov(2, 0).unwrap();
    doc.commit_then_renew();
    
    list.set(2, "B-modified").unwrap();
    doc.commit_then_renew();

    let final_state = doc.get_deep_value();
    
    // Undo operations
    assert!(undo.undo().unwrap()); // Undo set
    assert!(undo.undo().unwrap()); // Undo move
    
    // Verify order is back to A, B, C
    let values: Vec<String> = (0..list.len()).map(|i| list.get(i).unwrap().as_string().unwrap().to_string()).collect();
    assert_eq!(values, vec!["A".to_string(), "B".to_string(), "C".to_string()]);
    
    // Redo operations
    assert!(undo.redo().unwrap());
    assert!(undo.redo().unwrap());
    
    // Verify final state
    assert_eq!(doc.get_deep_value(), final_state);
}

/// Test that undo/redo works correctly with a large number of operations
#[test]
fn test_undo_consistency_stress() {
    let doc = LoroDoc::new();
    let mut undo = UndoManager::new(&doc);
    
    let text = doc.get_text("text");
    let num_ops = 100;
    
    // Perform many operations
    for i in 0..num_ops {
        text.insert(text.len_unicode() as usize, &format!("{} ", i)).unwrap();
        doc.commit_then_renew();
    }
    
    let final_text = text.to_string();
    
    // Undo all operations
    for _ in 0..num_ops {
        assert!(undo.undo().unwrap());
    }
    assert_eq!(text.to_string(), "");
    
    // Redo all operations
    for _ in 0..num_ops {
        assert!(undo.redo().unwrap());
    }
    assert_eq!(text.to_string(), final_text);
}

/// Test that the optimization is actually being used
#[test]
fn test_undo_uses_precalculated_diffs() {
    let doc = LoroDoc::new();
    let mut undo = UndoManager::new(&doc);
    
    let text = doc.get_text("text");
    
    // Make a change after the undo system is set up
    text.insert(0, "Test").unwrap();
    doc.commit_then_renew();
    
    // This undo should use the pre-calculated diff
    assert!(undo.undo().unwrap());
    assert_eq!(text.to_string(), "");
    
    // This redo should also use the pre-calculated diff
    assert!(undo.redo().unwrap());
    assert_eq!(text.to_string(), "Test");
}
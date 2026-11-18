use loro::{ExportMode, LoroDoc, LoroText, TreeParentId, UndoManager};

/// loro-dev/loro#1310: undoing text typed into a tree node's data must not
/// panic after a remote peer created a sibling node.
#[test]
fn undo_text_in_tree_node_after_remote_sibling_create() {
    let a = LoroDoc::new();
    a.set_peer_id(1).unwrap();
    let root = a.get_tree("tree").create(TreeParentId::Root).unwrap();
    a.commit();

    let b = LoroDoc::from_snapshot(&a.export(ExportMode::Snapshot).unwrap()).unwrap();
    b.set_peer_id(2).unwrap();
    let mut undo = UndoManager::new(&b);
    undo.set_merge_interval(0);
    let tree = b.get_tree("tree");
    let node = tree.create(root).unwrap();
    let text = tree
        .get_meta(node)
        .unwrap()
        .insert_container("t", LoroText::new())
        .unwrap();
    b.commit();
    text.insert(0, "ab").unwrap();
    b.commit();

    a.get_tree("tree").create(root).unwrap();
    a.commit();
    b.import(&a.export(ExportMode::all_updates()).unwrap())
        .unwrap();

    assert!(undo.undo().unwrap());
    assert_eq!(text.to_string(), "");
    assert!(undo.undo().unwrap());
    assert!(!tree.contains(node) || tree.is_node_deleted(&node).unwrap());
}

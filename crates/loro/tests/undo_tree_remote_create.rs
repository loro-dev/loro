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

fn paragraph_with_text(doc: &LoroDoc, s: &str) -> LoroText {
    let tree = doc.get_tree("tree");
    tree.enable_fractional_index(0);
    let node = tree.create_at(TreeParentId::Root, 0).unwrap();
    let meta = tree.get_meta(node).unwrap();
    meta.insert("kind", "paragraph").unwrap();
    let text = meta.insert_container("content", LoroText::new()).unwrap();
    text.insert(0, s).unwrap();
    doc.commit();
    text
}

/// loro-dev/loro#915: undo of a text edit in a tree node's data after
/// importing an independent doc.
#[test]
fn undo_text_in_tree_node_after_independent_import() {
    let a = LoroDoc::new();
    a.set_peer_id(1).unwrap();
    let text = paragraph_with_text(&a, "Hello");
    let mut undo = UndoManager::new(&a);
    undo.set_merge_interval(0);
    text.insert(5, " world").unwrap();
    a.commit();

    let b = LoroDoc::new();
    b.set_peer_id(2).unwrap();
    paragraph_with_text(&b, "From B");
    a.import(&b.export(ExportMode::all_updates()).unwrap())
        .unwrap();

    assert!(undo.undo().unwrap());
    assert_eq!(text.to_string(), "Hello");
}

/// The case from loro-dev/loro#980's review: the import carries a real remote
/// edit to the same text along with the re-created node.
#[test]
fn undo_text_in_tree_node_after_remote_edit_and_independent_node() {
    let a = LoroDoc::new();
    a.set_peer_id(1).unwrap();
    let text = paragraph_with_text(&a, "Hello");
    let base_vv = a.oplog_vv();
    let base = a.export(ExportMode::all_updates()).unwrap();
    let mut undo = UndoManager::new(&a);
    undo.set_merge_interval(0);
    text.insert(5, " world").unwrap();
    a.commit();

    let c = LoroDoc::new();
    c.set_peer_id(2).unwrap();
    c.import(&base).unwrap();
    let node = a.get_tree("tree").roots()[0];
    let c_text = c
        .get_tree("tree")
        .get_meta(node)
        .unwrap()
        .get("content")
        .unwrap()
        .into_container()
        .unwrap()
        .into_text()
        .unwrap();
    c_text.insert(0, "Hi ").unwrap();
    c.commit();

    let d = LoroDoc::new();
    d.set_peer_id(3).unwrap();
    let d_tree = d.get_tree("tree");
    d_tree.enable_fractional_index(0);
    let d_node = d_tree.create_at(TreeParentId::Root, 0).unwrap();
    d_tree
        .get_meta(d_node)
        .unwrap()
        .insert("kind", "from-d")
        .unwrap();
    d.commit();
    c.import(&d.export(ExportMode::all_updates()).unwrap())
        .unwrap();

    a.import(&c.export(ExportMode::updates(&base_vv)).unwrap())
        .unwrap();
    assert_eq!(text.to_string(), "Hi Hello world");

    assert!(undo.undo().unwrap());
    assert_eq!(text.to_string(), "Hi Hello");
}

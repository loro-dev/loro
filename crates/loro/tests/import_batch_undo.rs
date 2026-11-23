use loro::{ExportMode, LoroDoc, UndoManager};

fn remote_update(peer: u64, text: &str) -> Vec<u8> {
    let doc = LoroDoc::new();
    doc.set_peer_id(peer).unwrap();
    doc.get_text("text").insert(0, text).unwrap();
    doc.commit();
    doc.export(ExportMode::all_updates()).unwrap()
}

#[test]
fn import_batch_keeps_undo_stack() {
    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    let mut undo = UndoManager::new(&doc);
    let text = doc.get_text("text");
    text.insert(0, "R").unwrap();
    doc.commit();
    assert!(undo.can_undo());

    doc.import_batch(&[remote_update(2, "S"), remote_update(3, "T")])
        .unwrap();

    assert!(undo.can_undo());
    assert!(undo.undo().unwrap());
    assert!(!text.to_string().contains('R'));
}

#[test]
fn import_batch_matches_sequential_import_for_undo() {
    let (a, b) = (remote_update(2, "S"), remote_update(3, "T"));
    for batched in [false, true] {
        let doc = LoroDoc::new();
        doc.set_peer_id(1).unwrap();
        let undo = UndoManager::new(&doc);
        doc.get_text("text").insert(0, "R").unwrap();
        doc.commit();
        if batched {
            doc.import_batch(&[a.clone(), b.clone()]).unwrap();
        } else {
            doc.import(&a).unwrap();
            doc.import(&b).unwrap();
        }
        assert!(undo.can_undo(), "batched = {batched}");
    }
}

#[test]
fn import_batch_keeps_redo_stack() {
    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    let mut undo = UndoManager::new(&doc);
    doc.get_map("local").insert("value", 1).unwrap();
    doc.commit();
    assert!(undo.undo().unwrap());
    assert!(undo.can_redo());

    doc.import_batch(&[remote_update(2, "S"), remote_update(3, "T")])
        .unwrap();

    assert!(undo.can_redo());
    assert!(undo.redo().unwrap());
    assert!(doc.get_map("local").get("value").is_some());
    assert_eq!(doc.get_text("text").len_unicode(), 2);
}

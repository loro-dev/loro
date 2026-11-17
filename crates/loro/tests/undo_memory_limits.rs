use loro::{ExportMode, LoroDoc, UndoManager};

#[test]
fn checkpoint_for_imported_same_peer_edits_transforms_against_remote_text() {
    let source = LoroDoc::new();
    source.set_peer_id(1).unwrap();
    source.get_text("text").insert(0, "local").unwrap();
    source.commit();

    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    let mut undo = UndoManager::new(&doc);
    doc.import(&source.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    undo.record_new_checkpoint().unwrap();

    let remote = doc.fork();
    remote.set_peer_id(2).unwrap();
    remote.get_text("text").insert(0, "remote-").unwrap();
    remote.commit();
    doc.import(&remote.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    assert_eq!(doc.get_text("text").to_string(), "remote-local");
    assert!(undo.undo().unwrap());
    assert_eq!(doc.get_text("text").to_string(), "remote-");
    assert!(undo.redo().unwrap());
    assert_eq!(doc.get_text("text").to_string(), "remote-local");
}

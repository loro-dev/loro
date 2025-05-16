use loro_internal::{handler::UpdateOptions, loro::ExportMode, LoroDoc, ToJson, UndoManager};

// A types 1
// A types 2 (now 12)
// B types 3 (now 123)
// A types 4 (now 1234)
// A does undo
// state should be 123

pub fn main() {
    let doc = LoroDoc::new_auto_commit();

    let mut undo_manager = UndoManager::new(&doc);

    undo_manager.group_start().unwrap();

    doc.get_text("content")
        .update("1", UpdateOptions::default())
        .unwrap();
    doc.commit_then_renew();

    doc.get_text("content")
        .update("12", UpdateOptions::default())
        .unwrap();

    let snapshot = doc.export(ExportMode::Snapshot).unwrap();

    doc.get_text("content")
        .update("123", UpdateOptions::default())
        .unwrap();

    let doc2 = LoroDoc::new();

    doc2.import(snapshot.as_slice()).unwrap();

    doc2.get_text("content")
        .update("123", UpdateOptions::default())
        .unwrap();

    let update = doc2
        .export(ExportMode::Updates {
            from: std::borrow::Cow::Owned(doc2.state_vv()),
        })
        .unwrap();

    println!("importing");
    doc.import(update.as_slice()).unwrap();

    println!("after import ",);
    doc.get_text("content")
        .update("123", UpdateOptions::default())
        .unwrap();
    doc.commit_then_renew();

    undo_manager.group_end();

    println!(
        "before undo {}",
        doc.get_text("content").get_richtext_value().to_json()
    );

    undo_manager.undo().unwrap();

    println!(
        "after undo {}",
        doc.get_text("content").get_richtext_value().to_json()
    );

    undo_manager.redo().unwrap();

    println!(
        "after redo {}",
        doc.get_text("content").get_richtext_value().to_json()
    );
}

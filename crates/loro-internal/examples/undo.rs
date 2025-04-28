use loro_internal::{HandlerTrait, LoroDoc, ToJson, UndoManager};

pub fn auto_checkpint() {
    let loro = LoroDoc::new_auto_commit();
    let undo_manager = UndoManager::new(&loro);
    loro.get_text("text").insert(0, "hello").unwrap();
    loro.commit_then_renew();
    assert!(undo_manager.can_undo());
}

pub fn manual_checkpoint() {
    let loro = LoroDoc::new_auto_commit();
    let mut undo_manager = UndoManager::new_with_manual_checkpoint(&loro);

    undo_manager.set_on_push(Some(Box::new(|a, b, c| None)));

    loro.get_text("text").insert(0, "hello").unwrap();

    loro.commit_then_renew();

    // Should not be able to undo yet
    assert!(!undo_manager.can_undo());

    // Manually checkpoint
    undo_manager.record_new_checkpoint().unwrap();

    assert!(undo_manager.can_undo());

    undo_manager.undo().unwrap();

    let text = loro.get_text("text");

    assert_eq!(text.get_richtext_value().to_json().to_string(), "[]");
}

pub fn main() {
    auto_checkpint();
    manual_checkpoint();
}

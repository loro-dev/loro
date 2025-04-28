use std::sync::{atomic::AtomicU64, Arc};

use loro_common::LoroValue;
use loro_internal::{handler::UpdateOptions, undo::UndoItemMeta, LoroDoc, UndoManager};

#[test]
fn undo_default_checkpoint() {
    let doc = LoroDoc::new();
    let undo_manager = UndoManager::new(&doc);
    let text = doc.get_text("text");

    let counter = Arc::new(AtomicU64::new(0));

    let counter_clone = Arc::clone(&counter);
    undo_manager.set_on_push(Some(Box::new(move |_, _, _| {
        counter_clone.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        UndoItemMeta {
            value: LoroValue::Null,
            cursors: Default::default(),
        }
    })));

    text.update("hello", UpdateOptions::default()).unwrap();

    doc.commit_then_renew();

    // assert only one thing was pushed to the stack
    assert_eq!(counter.load(std::sync::atomic::Ordering::Relaxed), 1);
}

#[test]
fn undo_manual_checkpoint() {
    let doc = LoroDoc::new();
    let mut undo_manager = UndoManager::new_with_manual_checkpoint(&doc);
    let text = doc.get_text("text");

    let counter = Arc::new(AtomicU64::new(0));
    let counter_clone = Arc::clone(&counter);
    undo_manager.set_on_push(Some(Box::new(move |_, _, _| {
        counter_clone.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        UndoItemMeta {
            value: LoroValue::Null,
            cursors: Default::default(),
        }
    })));

    text.update("hello", UpdateOptions::default()).unwrap();

    doc.commit_then_renew();

    // Nothing should have been pushed to the stack
    assert_eq!(counter.load(std::sync::atomic::Ordering::Relaxed), 0);

    undo_manager.record_new_checkpoint().unwrap();

    // assert only one thing was pushed to the stack
    assert_eq!(counter.load(std::sync::atomic::Ordering::Relaxed), 1);
}

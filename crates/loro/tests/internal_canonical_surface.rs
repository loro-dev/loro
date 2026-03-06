use loro::internal::{
    Diff, Handler, LoroDoc, MapHandler, TextHandler, UndoItemMeta, UndoManager, ValueOrHandler,
};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

#[test]
fn canonical_internal_handler_surface_covers_read_paths() {
    let doc = LoroDoc::new_auto_commit();
    let list = doc.get_list("list");
    let nested = list
        .insert_container(0, MapHandler::new_detached())
        .unwrap();
    nested.insert("flag", true).unwrap();
    let text = nested
        .insert_container("text", TextHandler::new_detached())
        .unwrap();
    text.insert_unicode(0, "hello").unwrap();

    assert!(matches!(
        list.get_(0),
        Some(ValueOrHandler::Handler(Handler::Map(_)))
    ));
    assert!(matches!(
        nested.get_("text"),
        Some(ValueOrHandler::Handler(Handler::Text(_)))
    ));
    assert!(matches!(
        doc.get_by_str_path("list/0/text"),
        Some(ValueOrHandler::Handler(Handler::Text(_)))
    ));

    let mut iterated = Vec::new();
    nested.for_each(|_, value| iterated.push(value));
    assert!(iterated
        .iter()
        .any(|value| matches!(value, ValueOrHandler::Handler(Handler::Text(_)))));

    let values: Vec<_> = nested.values().collect();
    assert!(values
        .iter()
        .any(|value| matches!(value, ValueOrHandler::Handler(Handler::Text(_)))));
}

#[test]
fn canonical_internal_event_surface_drives_subscriptions_and_undo() {
    let doc = LoroDoc::new_auto_commit();
    let text = doc.get_text("text");

    let root_events = Arc::new(AtomicUsize::new(0));
    let root_events_clone = root_events.clone();
    let _sub = doc.subscribe_root(Arc::new(move |event| {
        assert!(matches!(&event.events[0].diff, Diff::Text(_)));
        root_events_clone.fetch_add(event.events.len(), Ordering::Relaxed);
    }));

    let undo_events = Arc::new(AtomicUsize::new(0));
    let undo_events_clone = undo_events.clone();
    let undo = UndoManager::new(&doc);
    undo.set_merge_interval(0);
    undo.set_on_push(Some(Box::new(move |_, _, event| {
        let event = event.expect("undo push should carry the canonical diff event");
        assert!(matches!(&event.events[0].diff, Diff::Text(_)));
        undo_events_clone.fetch_add(event.events.len(), Ordering::Relaxed);
        UndoItemMeta::new()
    })));

    text.insert_unicode(0, "hello").unwrap();
    doc.commit_then_renew();

    assert_eq!(root_events.load(Ordering::Relaxed), 1);
    assert_eq!(undo_events.load(Ordering::Relaxed), 1);
}

//! A user callback that panics must not break the document: the panic reaches
//! the caller, and the doc, its locks, and its subscriptions keep working.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc};
use std::time::Duration;

use loro::{ExportMode, LoroDoc, LoroValue};

fn panic_message(payload: Box<dyn std::any::Any + Send>) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|s| s.to_string())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_default()
}

#[test]
fn pre_commit_callback_panic_finishes_the_commit_and_keeps_the_doc_usable() {
    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let calls_in_callback = calls.clone();
    let _sub = doc.subscribe_pre_commit(Box::new(move |_| {
        if calls_in_callback.fetch_add(1, Ordering::SeqCst) == 0 {
            panic!("pre-commit bug");
        }
        true
    }));

    let result = catch_unwind(AssertUnwindSafe(|| {
        doc.get_text("t").insert(0, "a").unwrap();
        doc.commit();
    }));
    assert_eq!(panic_message(result.unwrap_err()), "pre-commit bug");

    // The change was committed, and the txn lock is not poisoned.
    assert_eq!(doc.oplog_vv().get(&1).copied(), Some(1));
    doc.get_text("t").insert(1, "b").unwrap();
    doc.commit();
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert_eq!(doc.get_text("t").to_string(), "ab");

    let replay = LoroDoc::new();
    replay
        .import(&doc.export(ExportMode::Snapshot).unwrap())
        .unwrap();
    assert_eq!(replay.get_deep_value(), doc.get_deep_value());
}

#[test]
fn subscriber_panic_keeps_the_subscriptions_working() {
    let doc = LoroDoc::new();
    let calls = Arc::new(AtomicUsize::new(0));
    let calls_in_callback = calls.clone();
    let _panicking = doc.subscribe_root(Arc::new(move |_| {
        if calls_in_callback.fetch_add(1, Ordering::SeqCst) == 0 {
            panic!("subscriber bug");
        }
    }));
    let other_calls = Arc::new(AtomicUsize::new(0));
    let other_calls_in_callback = other_calls.clone();
    let _other = doc.subscribe_root(Arc::new(move |_| {
        other_calls_in_callback.fetch_add(1, Ordering::SeqCst);
    }));
    let updates = Arc::new(AtomicUsize::new(0));
    let updates_in_callback = updates.clone();
    let _local = doc.subscribe_local_update(Box::new(move |_| {
        if updates_in_callback.fetch_add(1, Ordering::SeqCst) == 0 {
            panic!("local update bug");
        }
        true
    }));

    let result = catch_unwind(AssertUnwindSafe(|| {
        doc.get_text("t").insert(0, "a").unwrap();
        doc.commit();
    }));
    assert!(result.is_err());
    // The other subscriber still got the event.
    assert_eq!(other_calls.load(Ordering::SeqCst), 1);

    doc.get_text("t").insert(0, "b").unwrap();
    doc.commit();
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert_eq!(other_calls.load(Ordering::SeqCst), 2);
    assert_eq!(updates.load(Ordering::SeqCst), 2);

    // Another thread can still emit on these subscriber sets: it used to wait
    // forever for the subscribers the panic had taken.
    let (tx, rx) = mpsc::channel();
    let doc_in_thread = doc.clone();
    std::thread::spawn(move || {
        doc_in_thread.get_text("t").insert(0, "c").unwrap();
        doc_in_thread.commit();
        tx.send(()).unwrap();
    });
    rx.recv_timeout(Duration::from_secs(10))
        .expect("an emit from another thread hung");
    assert_eq!(other_calls.load(Ordering::SeqCst), 3);
    assert_eq!(
        doc.get_deep_value(),
        LoroValue::from(std::collections::HashMap::from([(
            "t".to_string(),
            LoroValue::from("cba")
        )]))
    );
}

#[test]
fn subscriber_panic_during_import_keeps_the_doc_editable() {
    let source = LoroDoc::new();
    source.get_text("t").insert(0, "remote").unwrap();
    let update = source.export(ExportMode::all_updates()).unwrap();

    let doc = LoroDoc::new();
    let calls = Arc::new(AtomicUsize::new(0));
    let calls_in_callback = calls.clone();
    let _sub = doc.subscribe_root(Arc::new(move |_| {
        if calls_in_callback.fetch_add(1, Ordering::SeqCst) == 0 {
            panic!("subscriber bug");
        }
    }));

    // Import events are emitted while the import holds the txn lock.
    let result = catch_unwind(AssertUnwindSafe(|| doc.import(&update)));
    assert_eq!(panic_message(result.unwrap_err()), "subscriber bug");
    assert_eq!(doc.get_text("t").to_string(), "remote");

    doc.get_text("t").insert(0, "local ").unwrap();
    doc.commit();
    assert_eq!(doc.get_text("t").to_string(), "local remote");
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

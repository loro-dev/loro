//! Dropping a subscriber's callback runs arbitrary code. When that callback owns
//! another `Subscription` or an `UndoManager` on the same doc, their drop takes the
//! subscriber set's lock again, so the set must not hold it while dropping
//! callbacks. See loro-dev/loro#1162.

use loro::{ContainerTrait, LoroDoc, UndoManager};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

fn returns_within_5s(f: impl FnOnce() + Send + 'static) -> bool {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        f();
        let _ = tx.send(());
    });
    rx.recv_timeout(Duration::from_secs(5)).is_ok()
}

#[test]
fn dropping_a_callback_that_owns_another_subscription_does_not_deadlock() {
    let doc = LoroDoc::new();
    let inner = doc.subscribe_root(Arc::new(|_| {}));
    let outer = doc.subscribe_root(Arc::new(move |_| {
        let _owned = &inner;
    }));
    assert!(returns_within_5s(move || drop(outer)));
}

#[test]
fn dropping_a_callback_that_owns_an_undo_manager_does_not_deadlock() {
    let doc = LoroDoc::new();
    let undo = Mutex::new(UndoManager::new(&doc));
    let outer = doc.subscribe_root(Arc::new(move |_| {
        let _owned = &undo;
    }));
    assert!(returns_within_5s(move || drop(outer)));
}

/// A container subscription whose callback owns a root subscription.
#[test]
fn dropping_a_container_callback_that_owns_a_root_subscription_does_not_deadlock() {
    let doc = LoroDoc::new();
    let text = doc.get_text("t");
    let inner = doc.subscribe_root(Arc::new(|_| {}));
    let outer = doc.subscribe(
        &text.id(),
        Arc::new(move |_| {
            let _owned = &inner;
        }),
    );
    assert!(returns_within_5s(move || drop(outer)));
}

/// The subscription is dropped while its own callback runs, so `retain` removes it
/// after the emit, together with the subscription its callback owns.
#[test]
fn unsubscribing_during_emit_drops_owned_subscription_without_deadlock() {
    assert!(returns_within_5s(|| {
        let doc = LoroDoc::new();
        let inner = doc.subscribe_root(Arc::new(|_| {}));
        let slot: Arc<Mutex<Option<loro::Subscription>>> = Arc::new(Mutex::new(None));
        let slot_in_cb = slot.clone();
        let outer = doc.subscribe_root(Arc::new(move |_| {
            let _owned = &inner;
            // Drop our own subscription mid-emit.
            slot_in_cb.lock().unwrap().take();
        }));
        *slot.lock().unwrap() = Some(outer);
        doc.get_text("t").insert(0, "a").unwrap();
        doc.commit();
        // The doc still works and emits to new subscribers.
        let hits = Arc::new(Mutex::new(0));
        let hits_in_cb = hits.clone();
        let _sub = doc.subscribe_root(Arc::new(move |_| *hits_in_cb.lock().unwrap() += 1));
        doc.get_text("t").insert(0, "b").unwrap();
        doc.commit();
        assert_eq!(*hits.lock().unwrap(), 1);
    }));
}

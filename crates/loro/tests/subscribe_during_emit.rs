//! Subscribing to an emitter while that same emitter is emitting must not
//! panic. The subscriber set checks the emitter's map out during an emit, so
//! these subscriptions have to be parked and merged back afterwards.
use loro::{ContainerTrait, LoroDoc, Subscription};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    mpsc, Arc, Mutex,
};

#[test]
fn subscribe_local_update_inside_local_update_callback() {
    let doc = LoroDoc::new();
    let late_calls = Arc::new(AtomicUsize::new(0));
    let late_subs: Arc<Mutex<Vec<Subscription>>> = Default::default();

    let doc_clone = doc.clone();
    let late_calls_clone = late_calls.clone();
    let late_subs_clone = late_subs.clone();
    let _sub = doc.subscribe_local_update(Box::new(move |_| {
        let mut subs = late_subs_clone.lock().unwrap();
        if subs.is_empty() {
            let late_calls = late_calls_clone.clone();
            subs.push(doc_clone.subscribe_local_update(Box::new(move |_| {
                late_calls.fetch_add(1, Ordering::SeqCst);
                true
            })));
        }
        true
    }));

    doc.get_text("text").insert(0, "a").unwrap();
    doc.commit();
    // Added mid-emit: does not see the update that is being delivered.
    assert_eq!(late_calls.load(Ordering::SeqCst), 0);

    doc.get_text("text").insert(0, "b").unwrap();
    doc.commit();
    assert_eq!(late_calls.load(Ordering::SeqCst), 1);

    late_subs.lock().unwrap().clear();
    doc.get_text("text").insert(0, "c").unwrap();
    doc.commit();
    assert_eq!(late_calls.load(Ordering::SeqCst), 1);
}

#[test]
fn subscribe_root_inside_root_callback() {
    let doc = LoroDoc::new();
    let late_calls = Arc::new(AtomicUsize::new(0));
    let late_subs: Arc<Mutex<Vec<Subscription>>> = Default::default();

    let doc_clone = doc.clone();
    let late_calls_clone = late_calls.clone();
    let late_subs_clone = late_subs.clone();
    let _sub = doc.subscribe_root(Arc::new(move |_| {
        let mut subs = late_subs_clone.lock().unwrap();
        if subs.is_empty() {
            let late_calls = late_calls_clone.clone();
            subs.push(doc_clone.subscribe_root(Arc::new(move |_| {
                late_calls.fetch_add(1, Ordering::SeqCst);
            })));
        }
    }));

    doc.get_text("text").insert(0, "a").unwrap();
    doc.commit();
    assert_eq!(late_calls.load(Ordering::SeqCst), 0);

    doc.get_text("text").insert(0, "b").unwrap();
    doc.commit();
    assert_eq!(late_calls.load(Ordering::SeqCst), 1);
}

#[test]
fn subscribe_and_unsubscribe_inside_callback_never_fires() {
    let doc = LoroDoc::new();
    let late_calls = Arc::new(AtomicUsize::new(0));

    let doc_clone = doc.clone();
    let late_calls_clone = late_calls.clone();
    let _sub = doc.subscribe_local_update(Box::new(move |_| {
        let late_calls = late_calls_clone.clone();
        let sub = doc_clone.subscribe_local_update(Box::new(move |_| {
            late_calls.fetch_add(1, Ordering::SeqCst);
            true
        }));
        drop(sub);
        true
    }));

    for i in 0..3 {
        doc.get_text("text").insert(0, &i.to_string()).unwrap();
        doc.commit();
    }
    assert_eq!(late_calls.load(Ordering::SeqCst), 0);
}

/// An emitting subscriber that unsubscribes itself after a new subscriber was
/// added in the same emit must still be removed.
#[test]
fn self_unsubscribe_after_subscribing_inside_callback() {
    let doc = LoroDoc::new();
    let first_calls = Arc::new(AtomicUsize::new(0));
    let late_calls = Arc::new(AtomicUsize::new(0));
    let first_sub: Arc<Mutex<Option<Subscription>>> = Default::default();
    let late_subs: Arc<Mutex<Vec<Subscription>>> = Default::default();

    let doc_clone = doc.clone();
    let first_calls_clone = first_calls.clone();
    let late_calls_clone = late_calls.clone();
    let first_sub_clone = first_sub.clone();
    let late_subs_clone = late_subs.clone();
    let sub = doc.subscribe_local_update(Box::new(move |_| {
        first_calls_clone.fetch_add(1, Ordering::SeqCst);
        let late_calls = late_calls_clone.clone();
        late_subs_clone
            .lock()
            .unwrap()
            .push(doc_clone.subscribe_local_update(Box::new(move |_| {
                late_calls.fetch_add(1, Ordering::SeqCst);
                true
            })));
        drop(first_sub_clone.lock().unwrap().take());
        true
    }));
    *first_sub.lock().unwrap() = Some(sub);

    doc.get_text("text").insert(0, "a").unwrap();
    doc.commit();
    doc.get_text("text").insert(0, "b").unwrap();
    doc.commit();
    assert_eq!(first_calls.load(Ordering::SeqCst), 1);
    assert_eq!(late_calls.load(Ordering::SeqCst), 1);
}

/// One thread is delivering a local update while another subscribes.
#[test]
fn subscribe_local_update_while_another_thread_emits() {
    let doc = LoroDoc::new();
    let (entered_tx, entered_rx) = mpsc::channel::<()>();
    let (release_tx, release_rx) = mpsc::channel::<()>();
    let entered_tx = Mutex::new(Some(entered_tx));
    let release_rx = Mutex::new(release_rx);
    let _blocking = doc.subscribe_local_update(Box::new(move |_| {
        if let Some(tx) = entered_tx.lock().unwrap().take() {
            tx.send(()).unwrap();
            release_rx.lock().unwrap().recv().unwrap();
        }
        true
    }));

    let emitter = doc.clone();
    let handle = std::thread::spawn(move || {
        emitter.get_text("text").insert(0, "a").unwrap();
        emitter.commit();
    });

    // The other thread is now parked inside the callback, with the
    // subscriber map checked out.
    entered_rx.recv().unwrap();
    let late_calls = Arc::new(AtomicUsize::new(0));
    let late_calls_clone = late_calls.clone();
    let _late = doc.subscribe_local_update(Box::new(move |_| {
        late_calls_clone.fetch_add(1, Ordering::SeqCst);
        true
    }));
    release_tx.send(()).unwrap();
    handle.join().unwrap();
    assert_eq!(late_calls.load(Ordering::SeqCst), 0);

    doc.get_text("text").insert(0, "b").unwrap();
    doc.commit();
    assert_eq!(late_calls.load(Ordering::SeqCst), 1);
}

/// A container subscriber added from another thread while that container's
/// event is being delivered.
#[test]
fn subscribe_container_while_another_thread_emits() {
    let doc = LoroDoc::new();
    let text = doc.get_text("text");
    let (entered_tx, entered_rx) = mpsc::channel::<()>();
    let (release_tx, release_rx) = mpsc::channel::<()>();
    let entered_tx = Mutex::new(Some(entered_tx));
    let release_rx = Mutex::new(release_rx);
    let _blocking = doc.subscribe(
        &text.id(),
        Arc::new(move |_| {
            if let Some(tx) = entered_tx.lock().unwrap().take() {
                tx.send(()).unwrap();
                release_rx.lock().unwrap().recv().unwrap();
            }
        }),
    );

    let emitter = doc.clone();
    let handle = std::thread::spawn(move || {
        emitter.get_text("text").insert(0, "a").unwrap();
        emitter.commit();
    });

    entered_rx.recv().unwrap();
    let late_calls = Arc::new(AtomicUsize::new(0));
    let late_calls_clone = late_calls.clone();
    let _late = doc.subscribe(
        &text.id(),
        Arc::new(move |_| {
            late_calls_clone.fetch_add(1, Ordering::SeqCst);
        }),
    );
    release_tx.send(()).unwrap();
    handle.join().unwrap();
    assert_eq!(late_calls.load(Ordering::SeqCst), 0);

    text.insert(0, "b").unwrap();
    doc.commit();
    assert_eq!(late_calls.load(Ordering::SeqCst), 1);
}

use std::{
    sync::atomic::{AtomicUsize, Ordering},
    thread::sleep,
    time::Duration,
};

use loro::{ExportMode, LoroDoc};

#[ctor::ctor]
fn init() {
    dev_utils::setup_test_log();
}

#[test]
fn concurrently_inserting_text_content() {
    let doc = LoroDoc::new();
    let doc1 = doc.clone();
    let doc2 = doc.clone();
    let h0 = std::thread::spawn(move || {
        for _ in 0..10 {
            doc1.get_text("text").insert(0, "1").unwrap();
        }
    });
    let h1 = std::thread::spawn(move || {
        for _ in 0..10 {
            doc2.get_text("text").insert(0, "2").unwrap();
        }
    });

    h0.join().unwrap();
    h1.join().unwrap();
    let text = doc.get_text("text");
    assert_eq!(text.len_utf8(), 20);
    dbg!("{}", text.to_string());
}

#[test]
fn concurrently_creating_events_with_subscriber() {
    let doc = LoroDoc::new();
    let count = std::sync::Arc::new(AtomicUsize::new(0));
    let count_clone = count.clone();
    let _sub = doc.subscribe_root(std::sync::Arc::new(move |e| {
        for e in e.events {
            let v = e.diff.as_text().unwrap();
            for v in v {
                match &v {
                    loro::TextDelta::Retain { .. } => unreachable!(),
                    loro::TextDelta::Delete { .. } => unreachable!(),
                    loro::TextDelta::Insert { insert, .. } => {
                        count_clone.fetch_add(insert.len(), Ordering::SeqCst);
                    }
                }
            }
        }
    }));
    let doc1 = doc.clone();
    let doc2 = doc.clone();
    let h0 = std::thread::spawn(move || {
        for _ in 0..100 {
            doc1.get_text("text").insert(0, "1").unwrap();
            doc1.commit();
        }
    });
    let h1 = std::thread::spawn(move || {
        for _ in 0..100 {
            doc2.get_text("text").insert(0, "2").unwrap();
            doc2.commit();
        }
    });

    h0.join().unwrap();
    h1.join().unwrap();
    let text = doc.get_text("text");
    assert_eq!(text.len_utf8(), 200);
    assert_eq!(count.load(Ordering::SeqCst), 200);
}

#[test]
fn concurrently_import_export() {
    let doc1 = LoroDoc::new();
    let doc1_clone = doc1.clone();
    let doc1_clone2 = doc1.clone();
    let doc2 = LoroDoc::new();
    let doc2_clone = doc2.clone();
    let doc2_clone2 = doc2.clone();
    let doc3 = LoroDoc::new();
    let doc3_clone = doc3.clone();
    let doc3_clone2 = doc3.clone();

    let mut handlers = vec![];
    handlers.push(std::thread::spawn(move || {
        for _ in 0..10 {
            doc1_clone.get_text("text").insert(0, "1").unwrap();
            doc1_clone.commit();
        }
    }));
    handlers.push(std::thread::spawn(move || {
        for _ in 0..10 {
            doc2_clone.get_text("text").insert(0, "2").unwrap();
            doc2_clone.commit();
        }
    }));
    handlers.push(std::thread::spawn(move || {
        for _ in 0..10 {
            doc3_clone.get_text("text").insert(0, "3").unwrap();
            doc3_clone.commit();
        }
    }));
    handlers.push(std::thread::spawn(move || {
        for _ in 0..10 {
            doc2_clone2
                .import(
                    &doc1_clone2
                        .export(ExportMode::updates(&doc2_clone2.oplog_vv()))
                        .unwrap(),
                )
                .unwrap();
            sleep(Duration::from_micros(10));
            doc1_clone2
                .import(
                    &doc2_clone2
                        .export(ExportMode::updates(&doc1_clone2.oplog_vv()))
                        .unwrap(),
                )
                .unwrap();
        }
    }));

    let doc2_clone2 = doc2.clone();
    handlers.push(std::thread::spawn(move || {
        for _ in 0..10 {
            doc2_clone2
                .import(
                    &doc3_clone2
                        .export(ExportMode::updates(&doc2_clone2.oplog_vv()))
                        .unwrap(),
                )
                .unwrap();
            sleep(Duration::from_micros(10));
            doc3_clone2
                .import(
                    &doc2_clone2
                        .export(ExportMode::updates(&doc3_clone2.oplog_vv()))
                        .unwrap(),
                )
                .unwrap();
        }
    }));

    for h in handlers {
        h.join().unwrap();
    }

    // Get final text values
    let text1 = doc1.get_text("text").to_string();
    let text2 = doc2.get_text("text").to_string();
    let text3 = doc3.get_text("text").to_string();

    // Assert all texts are equal
    assert_eq!(text1, text2);
    assert_eq!(text2, text3);
}

#[test]
fn concurrent_nested_operations() {
    let doc = LoroDoc::new();
    let doc1 = doc.clone();
    let doc2 = doc.clone();

    // Use a barrier to synchronize threads
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let barrier1 = barrier.clone();
    let barrier2 = barrier.clone();

    let h1 = std::thread::spawn(move || {
        // Get a text container
        let text = doc1.get_text("text");

        // First operation
        text.insert(0, "Thread: ").unwrap();

        // Wait for both threads to reach this point
        barrier1.wait();

        // Both threads will attempt to commit simultaneously
        doc1.commit();

        // Second operation while thread 2 might be committing
        text.insert(text.len_utf8(), " End").unwrap();
        doc1.commit();
    });

    let h2 = std::thread::spawn(move || {
        // Get the same text container
        let text = doc2.get_text("text");

        // First operation
        text.insert(0, "Thread: ").unwrap();

        // Wait for both threads to reach this point
        barrier2.wait();

        // Both threads will attempt to commit simultaneously
        doc2.commit();

        // Second operation while thread 1 might be committing
        text.insert(text.len_utf8(), " End").unwrap();
        doc2.commit();
    });

    h1.join().unwrap();
    h2.join().unwrap();

    let text = doc.get_text("text").to_string();
    assert_eq!(text, "Thread: Thread:  End End");
}

#[test]
fn concurrent_callbacks_modifying_same_doc() {
    let doc = LoroDoc::new();
    let text_id = "shared_text";

    // Set up a condition variable to control thread execution
    let pair = std::sync::Arc::new((std::sync::Mutex::new(false), std::sync::Condvar::new()));
    let pair_clone1 = pair.clone();
    let pair_clone2 = pair.clone();

    // Create a counter to track callbacks
    let counter = std::sync::Arc::new(AtomicUsize::new(0));
    let counter_clone = counter.clone();

    // First subscription modifies the document in its callback
    let doc1 = doc.clone();
    let _sub1 = doc.subscribe_root(std::sync::Arc::new(move |_| {
        let count = counter_clone.fetch_add(1, Ordering::SeqCst);

        // Only react to the first few events to avoid infinite loops
        if count < 5 {
            // This will trigger another event
            doc1.get_text(text_id).insert(0, "A").unwrap();
            doc1.commit();

            // Signal the other thread
            let (lock, cvar) = &*pair_clone1;
            let mut started = lock.lock().unwrap();
            *started = true;
            cvar.notify_one();
        }
    }));

    // Second thread also has a subscription that modifies the document
    let doc2 = doc.clone();
    let doc3 = doc.clone();

    let h = std::thread::spawn(move || {
        // Wait for the first thread's callback to trigger
        let (lock, cvar) = &*pair_clone2;
        let mut started = lock.lock().unwrap();
        while !*started {
            // Wait until the condition variable is signaled
            started = cvar.wait(started).unwrap();
        }

        // Now both threads are modifying the document based on events
        doc2.get_text(text_id).insert(0, "B").unwrap();
        doc2.commit();

        // Sleep to increase chance of race condition
        sleep(Duration::from_millis(10));

        doc2.get_text(text_id).insert(0, "C").unwrap();
        doc2.commit();
    });

    // Start the chain of events
    doc3.get_text(text_id).insert(0, "Start").unwrap();
    doc3.commit();

    h.join().unwrap();

    let text = doc.get_text(text_id).to_string();
    println!("Final text with concurrent callbacks: {}", text);
}

#[test]
fn concurrent_document_checkout_with_modifications() {
    let doc = LoroDoc::new();
    doc.set_detached_editing(true);

    // Set up multiple frontiers to test checkout
    doc.get_text("text").insert(0, "Initial state").unwrap();
    doc.commit();
    let initial_frontier = doc.state_frontiers();

    doc.get_text("text")
        .insert(doc.get_text("text").len_utf8(), " - First update")
        .unwrap();
    doc.commit();
    let second_frontier = doc.state_frontiers();

    doc.get_text("text")
        .insert(doc.get_text("text").len_utf8(), " - Second update")
        .unwrap();
    doc.commit();
    let third_frontier = doc.state_frontiers();

    // Now create threads that will concurrently checkout and modify
    let doc1 = doc.clone();
    let doc2 = doc.clone();
    let initial_clone = initial_frontier.clone();
    let second_clone = second_frontier.clone();
    let third_clone = third_frontier.clone();

    let h1 = std::thread::spawn(move || {
        // First checkout to an earlier state
        doc1.checkout(&initial_clone).unwrap();

        // Modify the document
        doc1.get_text("text")
            .insert(doc1.get_text("text").len_utf8(), " - Thread 1 modification")
            .unwrap();
        doc1.commit();

        // Sleep to increase chance of race condition
        sleep(Duration::from_millis(5));

        // Checkout to a later state
        doc1.checkout(&third_clone).unwrap();

        // Modify again
        doc1.get_text("text")
            .insert(
                doc1.get_text("text").len_utf8(),
                " - Thread 1 after checkout",
            )
            .unwrap();
        doc1.commit();
    });

    let h2 = std::thread::spawn(move || {
        // Sleep to ensure operations interleave
        sleep(Duration::from_millis(2));

        // Checkout to the middle state
        doc2.checkout(&second_clone).unwrap();

        // Modify
        doc2.get_text("text")
            .insert(doc2.get_text("text").len_utf8(), " - Thread 2 modification")
            .unwrap();
        doc2.commit();

        // Sleep
        sleep(Duration::from_millis(5));

        // Checkout to latest
        doc2.checkout_to_latest();

        // Modify again
        doc2.get_text("text")
            .insert(
                doc2.get_text("text").len_utf8(),
                " - Thread 2 after checkout",
            )
            .unwrap();
        doc2.commit();
    });

    h1.join().unwrap();
    h2.join().unwrap();

    // Make sure we end up at latest
    doc.checkout_to_latest();
    let text = doc.get_text("text").to_string();
    println!("Final text after concurrent checkouts: {}", text);
}

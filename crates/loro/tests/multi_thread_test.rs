#[cfg(loom)]
mod loom_test {

    use loom::{explore, stop_exploring, sync::atomic::AtomicUsize, thread::yield_now};
    use loro::{ExportMode, LoroDoc};
    use std::{sync::atomic::Ordering, thread::sleep, time::Duration};

    #[test]
    fn concurrently_inserting_text_content() {
        loom::model(|| {
            let doc = LoroDoc::new();
            let doc1 = doc.clone();
            let doc2 = doc.clone();
            let h0 = loom::thread::spawn(move || {
                doc1.get_text("text").insert(0, "1").unwrap();
            });
            let h1 = loom::thread::spawn(move || {
                for _ in 0..2 {
                    doc2.get_text("text").insert(0, "2").unwrap();
                }
            });

            if let Err(e) = h0.join() {
                eprintln!("Thread h0 failed: {:?}", e);
                panic!("Thread h0 failed: {:?}", e);
            }

            if let Err(e) = h1.join() {
                eprintln!("Thread h1 failed: {:?}", e);
                panic!("Thread h1 failed: {:?}", e);
            }
            let text = doc.get_text("text");
            assert_eq!(text.len_utf8(), 3);
            dbg!("{}", text.to_string());
        });
    }

    #[test]
    fn concurrently_creating_events_with_subscriber() {
        loom::model(|| {
            let doc = LoroDoc::new();
            let count = std::sync::Arc::new(AtomicUsize::new(0));
            let count_clone = count.clone();
            let _sub = doc.subscribe_root(std::sync::Arc::new(move |e| {
                stop_exploring();
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
                explore();
            }));
            let doc1 = doc.clone();
            let doc2 = doc.clone();
            let h0 = loom::thread::spawn(move || {
                doc1.get_text("text").insert(0, "1").unwrap();
                doc1.commit();
            });
            let h1 = loom::thread::spawn(move || {
                doc2.get_text("text").insert(0, "2").unwrap();
                doc2.commit();
            });

            h0.join().unwrap();
            h1.join().unwrap();
            let text = doc.get_text("text");
            assert_eq!(text.len_utf8(), 2);
            assert_eq!(count.load(Ordering::SeqCst), 2);
        });
    }

    #[test]
    fn concurrent_callbacks_modifying_same_doc() {
        loom::model(|| {
            let doc = LoroDoc::new();
            let text_id = "shared_text";

            // Set up a condition variable to control thread execution
            let pair =
                std::sync::Arc::new((loom::sync::Mutex::new(false), loom::sync::Condvar::new()));
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
                if count < 2 {
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

            let h = loom::thread::spawn(move || {
                // Wait for the first thread's callback to trigger
                let (lock, cvar) = &*pair_clone2;
                let mut started = lock.lock().unwrap();
                while !*started {
                    // Wait until the condition variable is signaled
                    started = cvar.wait(started).unwrap();
                }

                drop(started);
                // Now both threads are modifying the document based on events
                doc2.get_text(text_id).insert(0, "B").unwrap();
                doc2.commit();

                doc2.commit();
            });

            // Start the chain of events
            doc3.get_text(text_id).insert(0, "Start").unwrap();
            doc3.commit();

            h.join().unwrap();
        });
    }

    #[test]
    fn concurrent_document_checkout_with_modifications() {
        let mut builder = loom::model::Builder::new();
        builder.max_branches = 2000;
        builder.check(|| {
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

            let h1 = loom::thread::spawn(move || {
                // First checkout to an earlier state
                doc1.checkout(&initial_clone).unwrap();

                // Modify the document
                doc1.get_text("text")
                    .insert(doc1.get_text("text").len_utf8(), " - Thread 1 modification")
                    .unwrap();
                doc1.commit();

                // Sleep to increase chance of race condition
                yield_now();

                // Checkout to a later state
                doc1.checkout(&third_clone).unwrap();

                // Modify again
                doc1.get_text("text")
                    .insert(0, " - Thread 1 after checkout")
                    .unwrap();
                doc1.commit();
            });

            let h2 = loom::thread::spawn(move || {
                yield_now();

                // Checkout to the middle state
                doc2.checkout(&second_clone).unwrap();

                // Modify
                doc2.get_text("text")
                    .insert(doc2.get_text("text").len_utf8(), " - Thread 2 modification")
                    .unwrap();
                doc2.commit();

                yield_now();

                // Checkout to latest
                doc2.checkout_to_latest();

                // Modify again
                doc2.get_text("text")
                    .insert(0, " - Thread 2 after checkout")
                    .unwrap();
                doc2.commit();
            });

            h1.join().unwrap();
            h2.join().unwrap();
        });
    }

    #[test]
    fn concurrently_import_export() {
        let mut builder = loom::model::Builder::new();
        builder.max_branches = 2000;
        builder.check(|| {
            let doc1 = LoroDoc::new();
            let doc1_clone = doc1.clone();
            let doc1_clone2 = doc1.clone();
            let doc2 = LoroDoc::new();
            let doc2_clone = doc2.clone();
            let doc2_clone2 = doc2.clone();

            let mut handlers = vec![];
            handlers.push(loom::thread::spawn(move || {
                doc1_clone.get_text("text").insert(0, "1").unwrap();
                doc1_clone.commit();
                doc1_clone.get_text("text").insert(0, "1").unwrap();
                doc1_clone.commit();
            }));
            handlers.push(loom::thread::spawn(move || {
                doc2_clone.get_text("text").insert(0, "2").unwrap();
                doc2_clone.commit();
            }));
            handlers.push(loom::thread::spawn(move || {
                let e = &doc1_clone2
                    .export(ExportMode::updates(&doc2_clone2.oplog_vv()))
                    .unwrap();
                doc2_clone2.import(e).unwrap();
                yield_now();
                let e = &doc2_clone2
                    .export(ExportMode::updates(&doc1_clone2.oplog_vv()))
                    .unwrap();
                doc1_clone2.import(e).unwrap();
            }));

            for h in handlers {
                h.join().unwrap();
            }
        });
    }
}

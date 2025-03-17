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

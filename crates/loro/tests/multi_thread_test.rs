use std::sync::atomic::{AtomicUsize, Ordering};

use loro::LoroDoc;

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

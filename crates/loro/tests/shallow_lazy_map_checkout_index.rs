//! A shallow doc seeds its checkout index one map at a time: a map's
//! shallow-root entries are added the first time a diff needs that map.
//! Merging concurrent map edits, checking out and undoing on a shallow replica
//! must give exactly what a full-history replica gives.
//!
//! Perf regression: seeding every map of the shallow root up front made the
//! first import that is concurrent with a local edit to the same map cost
//! O(all maps in the shallow root), independent of the update's size.
//!
//! Measured (Windows x86_64, --release, 100k maps in the shallow root, one
//! concurrent key per side):
//!
//! | scenario                                  | before   | after   |
//! |-------------------------------------------|----------|---------|
//! | first concurrent same-map import          | ~239 ms  | ~3.5 ms |
//! | second concurrent same-map import         | < 1 ms   | < 1 ms  |
//!
//! Run with:
//! cargo test -p loro --release --test shallow_lazy_map_checkout_index -- --ignored --nocapture

use loro::{ExportMode, LoroDoc, LoroMap, LoroValue, UndoManager};
use std::sync::{Arc, Mutex};
use std::time::Instant;

fn doc_with_rows(rows: usize) -> LoroDoc {
    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    let map = doc.get_map("rows");
    for i in 0..rows {
        let row = map
            .insert_container(&format!("r{i}"), LoroMap::new())
            .unwrap();
        row.insert("title", format!("row {i}")).unwrap();
        row.insert("n", i as i64).unwrap();
    }
    doc.commit();
    // Give some keys several versions.
    for round in 0..3 {
        for i in (0..rows).step_by(17) {
            row(&doc, i).insert("n", round as i64).unwrap();
        }
        doc.commit();
    }
    doc
}

fn row(doc: &LoroDoc, i: usize) -> LoroMap {
    doc.get_map("rows")
        .get(&format!("r{i}"))
        .unwrap()
        .into_container()
        .unwrap()
        .into_map()
        .unwrap()
}

struct Outcome {
    events: Vec<String>,
    merged: LoroValue,
    at_local_edit: LoroValue,
    after_undo: LoroValue,
}

/// Two replicas load `bytes`, edit the same rows concurrently, and the receiver
/// imports the sender's edits.
fn merge_concurrent_row_edits(bytes: &[u8]) -> Outcome {
    let sender = LoroDoc::new();
    sender.import(bytes).unwrap();
    sender.set_peer_id(2).unwrap();
    let from = sender.oplog_vv();
    for i in [5, 17, 34] {
        row(&sender, i)
            .insert("title", format!("sender {i}"))
            .unwrap();
    }
    row(&sender, 51).delete("n").unwrap();
    sender.commit();
    let update = sender.export(ExportMode::updates(&from)).unwrap();

    let receiver = LoroDoc::new();
    receiver.import(bytes).unwrap();
    receiver.set_peer_id(3).unwrap();
    let mut undo = UndoManager::new(&receiver);
    for i in [5, 17, 99] {
        row(&receiver, i)
            .insert("title", format!("receiver {i}"))
            .unwrap();
    }
    row(&receiver, 51).insert("n", -1).unwrap();
    receiver.commit();
    let local_edit = receiver.oplog_frontiers();

    let events = Arc::new(Mutex::new(Vec::new()));
    let sink = events.clone();
    let _sub = receiver.subscribe_root(Arc::new(move |e| {
        for d in e.events.iter() {
            sink.lock()
                .unwrap()
                .push(format!("{:?} {:?}", d.target, d.diff));
        }
    }));
    receiver.import(&update).unwrap();
    let merged = receiver.get_deep_value();
    // Undo before checkout: a checkout clears the undo stack.
    undo.undo().unwrap();
    let after_undo = receiver.get_deep_value();
    receiver.checkout(&local_edit).unwrap();
    let at_local_edit = receiver.get_deep_value();
    receiver.checkout_to_latest();
    let mut events = events.lock().unwrap().clone();
    events.sort();
    Outcome {
        events,
        merged,
        at_local_edit,
        after_undo,
    }
}

#[test]
fn concurrent_map_import_on_shallow_doc_matches_full_history() {
    let doc = doc_with_rows(200);
    let full = doc.export(ExportMode::Snapshot).unwrap();
    let shallow = doc
        .export(ExportMode::shallow_snapshot(&doc.oplog_frontiers()))
        .unwrap();

    let expected = merge_concurrent_row_edits(&full);
    let actual = merge_concurrent_row_edits(&shallow);
    assert!(!expected.events.is_empty());
    assert_eq!(actual.events, expected.events);
    assert_eq!(actual.merged, expected.merged);
    assert_eq!(actual.at_local_edit, expected.at_local_edit);
    assert_eq!(actual.after_undo, expected.after_undo);
    assert_ne!(actual.after_undo, actual.merged);
}

#[test]
#[ignore]
fn perf_first_concurrent_map_import_on_large_shallow_doc() {
    let rows = std::env::var("LORO_PERF_ROWS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(100_000);
    let doc = doc_with_rows(rows);
    let bytes = doc
        .export(ExportMode::shallow_snapshot(&doc.oplog_frontiers()))
        .unwrap();

    let sender = LoroDoc::new();
    sender.import(&bytes).unwrap();
    sender.set_peer_id(2).unwrap();
    let receiver = LoroDoc::new();
    receiver.import(&bytes).unwrap();
    receiver.set_peer_id(3).unwrap();

    for round in 0..2 {
        let from = sender.oplog_vv();
        row(&sender, 7)
            .insert("title", format!("sender {round}"))
            .unwrap();
        sender.commit();
        let update = sender.export(ExportMode::updates(&from)).unwrap();
        row(&receiver, 7)
            .insert("title", format!("receiver {round}"))
            .unwrap();
        receiver.commit();
        let t = Instant::now();
        receiver.import(&update).unwrap();
        println!(
            "{rows} maps, concurrent same-map import #{}: {:.2} ms ({} update bytes)",
            round + 1,
            t.elapsed().as_secs_f64() * 1000.0,
            update.len()
        );
    }
    receiver
        .import(&sender.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    sender
        .import(&receiver.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    assert_eq!(receiver.get_deep_value(), sender.get_deep_value());
}

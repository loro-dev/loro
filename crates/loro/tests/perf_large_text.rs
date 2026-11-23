//! Timings for undo, redo, version diff, checkout and batch import of large
//! text. All of them were quadratic in the size of one inserted text; see
//! `context/large-text-linear-time.md`.
//!
//! Print the timings with `cargo test --release -p loro --test perf_large_text
//! -- --ignored --nocapture --test-threads=1`. `LARGE_TEXT_SIZES` (comma
//! separated byte counts) and `LARGE_TEXT_BATCH` (comma separated counts of
//! 100 KB updates) override the defaults.

use loro::{ExportMode, LoroDoc, UndoManager};
use std::sync::{Arc, Mutex};
use std::time::Instant;

fn gen_text(n: usize) -> String {
    let line = "loro note sync the a markdown edit peer text of and **bold** `code`\n";
    line.repeat(n / line.len() + 1)[..n].to_string()
}

fn sizes() -> Vec<usize> {
    list_from_env("LARGE_TEXT_SIZES", &[100_000, 300_000, 1_000_000])
}

fn list_from_env(name: &str, default: &[usize]) -> Vec<usize> {
    match std::env::var(name) {
        Ok(s) => s.split(',').map(|x| x.trim().parse().unwrap()).collect(),
        Err(_) => default.to_vec(),
    }
}

fn ms(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1000.0
}

/// A doc with a little text, then `n` bytes inserted in one commit.
fn doc_with_insert(n: usize) -> (LoroDoc, loro::Frontiers, loro::Frontiers) {
    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    let text = doc.get_text("note");
    text.insert(0, "Hello world\n").unwrap();
    doc.commit();
    let before = doc.oplog_frontiers();
    text.insert(6, &gen_text(n)).unwrap();
    doc.commit();
    let after = doc.oplog_frontiers();
    (doc, before, after)
}

#[test]
#[ignore]
fn perf_undo_redo_large_insert() {
    for n in sizes() {
        let doc = LoroDoc::new();
        doc.set_peer_id(1).unwrap();
        let mut undo = UndoManager::new(&doc);
        undo.set_merge_interval(0);
        let text = doc.get_text("note");
        text.insert(0, "Hello world\n").unwrap();
        doc.commit();
        let big = gen_text(n);
        text.insert(6, &big).unwrap();
        doc.commit();
        let full = text.to_string();

        let s = Instant::now();
        assert!(undo.undo().unwrap());
        let undo_ms = ms(s);
        assert_eq!(text.to_string(), "Hello world\n");

        let s = Instant::now();
        assert!(undo.redo().unwrap());
        let redo_ms = ms(s);
        assert_eq!(text.to_string(), full);

        let s = Instant::now();
        assert!(undo.undo().unwrap());
        let undo2_ms = ms(s);
        println!(
            "undo/redo one insert of {n} B: undo {undo_ms:.1} ms, redo {redo_ms:.1} ms, second undo {undo2_ms:.1} ms"
        );
    }
}

#[test]
#[ignore]
fn perf_undo_small_edit_before_large_insert() {
    for n in sizes() {
        let doc = LoroDoc::new();
        doc.set_peer_id(1).unwrap();
        let mut undo = UndoManager::new(&doc);
        undo.set_merge_interval(0);
        let text = doc.get_text("note");
        text.insert(0, "Hello world\n").unwrap();
        doc.commit();
        text.insert(0, "small ").unwrap();
        doc.commit();
        text.insert(text.len_unicode(), &gen_text(n)).unwrap();
        doc.commit();
        text.insert(3, "x").unwrap();
        doc.commit();

        let s = Instant::now();
        assert!(undo.undo().unwrap());
        let after_ms = ms(s);
        let s = Instant::now();
        assert!(undo.undo().unwrap());
        let big_ms = ms(s);
        let s = Instant::now();
        assert!(undo.undo().unwrap());
        let before_ms = ms(s);
        assert_eq!(text.to_string(), "Hello world\n");
        println!(
            "undo around an insert of {n} B: edit after it {after_ms:.1} ms, the insert {big_ms:.1} ms, small edit before it {before_ms:.1} ms"
        );
    }
}

#[test]
#[ignore]
fn perf_diff_and_checkout_large_insert() {
    for n in sizes() {
        let (doc, before, after) = doc_with_insert(n);
        let full = doc.get_text("note").to_string();

        let s = Instant::now();
        let forward = doc.diff(&before, &after).unwrap();
        let forward_ms = ms(s);
        let s = Instant::now();
        let backward = doc.diff(&after, &before).unwrap();
        let backward_ms = ms(s);
        assert_eq!(forward.iter().count(), 1);
        assert_eq!(backward.iter().count(), 1);

        let s = Instant::now();
        doc.checkout(&before).unwrap();
        let checkout_ms = ms(s);
        assert_eq!(doc.get_text("note").to_string(), "Hello world\n");
        let s = Instant::now();
        doc.try_checkout_to_latest().unwrap();
        let latest_ms = ms(s);
        assert_eq!(doc.get_text("note").to_string(), full);
        println!(
            "diff/checkout over an insert of {n} B: diff forward {forward_ms:.1} ms, diff backward {backward_ms:.1} ms, checkout before {checkout_ms:.1} ms, checkout to latest {latest_ms:.1} ms"
        );
    }
}

/// A big delete (select all, delete), a big insert inside another one, and a
/// big insert made concurrently with another peer's edit.
#[test]
#[ignore]
fn perf_large_delete_nested_and_concurrent_insert() {
    for n in sizes() {
        let doc = LoroDoc::new();
        doc.set_peer_id(1).unwrap();
        let mut undo = UndoManager::new(&doc);
        undo.set_merge_interval(0);
        let text = doc.get_text("note");
        let big = gen_text(n);
        text.insert(0, &big).unwrap();
        doc.commit();
        // A second big insert in the middle of the first.
        text.insert(n / 2, &big).unwrap();
        doc.commit();
        let full = text.to_string();
        let before_delete = doc.oplog_frontiers();
        text.delete(0, text.len_unicode()).unwrap();
        doc.commit();
        let after_delete = doc.oplog_frontiers();

        let s = Instant::now();
        assert!(undo.undo().unwrap());
        let undo_delete_ms = ms(s);
        assert_eq!(text.to_string(), full);
        let s = Instant::now();
        assert!(undo.undo().unwrap());
        let undo_nested_ms = ms(s);
        assert_eq!(text.to_string(), big);
        let s = Instant::now();
        assert!(undo.redo().unwrap());
        assert!(undo.redo().unwrap());
        let redo_both_ms = ms(s);
        assert_eq!(text.len_unicode(), 0);
        let s = Instant::now();
        doc.checkout(&before_delete).unwrap();
        doc.checkout(&after_delete).unwrap();
        doc.try_checkout_to_latest().unwrap();
        let checkouts_ms = ms(s);

        // Peer 2 edits concurrently, then receives everything from peer 1.
        let other = LoroDoc::new();
        other.set_peer_id(2).unwrap();
        other.get_text("note").insert(0, "concurrent ").unwrap();
        other.commit();
        let updates = doc.export(ExportMode::all_updates()).unwrap();
        let s = Instant::now();
        other.import(&updates).unwrap();
        let concurrent_import_ms = ms(s);
        doc.import(&other.export(ExportMode::all_updates()).unwrap())
            .unwrap();
        assert_eq!(text.to_string(), other.get_text("note").to_string());

        println!(
            "two inserts of {n} B, one inside the other, then delete all: undo the delete {undo_delete_ms:.1} ms, undo the inner insert {undo_nested_ms:.1} ms, redo both {redo_both_ms:.1} ms, three checkouts across the delete {checkouts_ms:.1} ms, import into a doc with a concurrent edit {concurrent_import_ms:.1} ms"
        );
    }
}

/// `count` updates of `piece` bytes each, one per commit, from one peer.
fn updates(piece: usize, count: usize) -> (LoroDoc, Vec<Vec<u8>>) {
    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    let blobs = Arc::new(Mutex::new(Vec::new()));
    let sink = blobs.clone();
    let sub = doc.subscribe_local_update(Box::new(move |b| {
        sink.lock().unwrap().push(b.to_vec());
        true
    }));
    let text = doc.get_text("note");
    let chunk = gen_text(piece);
    for _ in 0..count {
        text.insert(text.len_unicode(), &chunk).unwrap();
        doc.commit();
    }
    drop(sub);
    let blobs = blobs.lock().unwrap().clone();
    assert_eq!(blobs.len(), count);
    (doc, blobs)
}

#[test]
#[ignore]
fn perf_import_batch_large_updates() {
    for count in list_from_env("LARGE_TEXT_BATCH", &[2, 4, 8]) {
        let piece = 100_000;
        let (src, blobs) = updates(piece, count);
        let expected = src.get_text("note").to_string();

        let one = LoroDoc::new();
        let s = Instant::now();
        for b in &blobs {
            one.import(b).unwrap();
        }
        let one_ms = ms(s);
        assert_eq!(one.get_text("note").to_string(), expected);

        let batch = LoroDoc::new();
        let s = Instant::now();
        batch.import_batch(&blobs).unwrap();
        let batch_ms = ms(s);
        assert_eq!(batch.get_text("note").to_string(), expected);

        // Into a doc that already has content and a concurrent edit.
        let base = LoroDoc::new();
        base.set_peer_id(2).unwrap();
        base.get_text("note").insert(0, "concurrent ").unwrap();
        base.commit();
        let s = Instant::now();
        base.import_batch(&blobs).unwrap();
        let concurrent_ms = ms(s);

        println!(
            "import {count} updates of {piece} B: one by one {one_ms:.1} ms, import_batch {batch_ms:.1} ms, import_batch into a doc with a concurrent edit {concurrent_ms:.1} ms"
        );
    }
}

//! Cost of one small local commit on a doc that was loaded from a snapshot
//! (or has exported one), with a local-update subscriber. Every such commit
//! exports its own change, and that export looked the change up in the encoded
//! history store even though the change was still in memory.
//! See `context/commit-export-after-snapshot.md`.
//!
//! Print the timings with `cargo test --release -p loro --test
//! perf_commit_after_snapshot -- --ignored --nocapture`. `COMMIT_DOC_SIZES`
//! (comma separated byte counts) overrides the document sizes.

use loro::{ExportMode, LoroDoc};
use std::time::Instant;

fn gen_text(n: usize) -> String {
    let line = "loro note sync the a markdown edit peer text of and **bold** `code`\n";
    line.repeat(n / line.len() + 1)[..n].to_string()
}

fn sizes() -> Vec<usize> {
    match std::env::var("COMMIT_DOC_SIZES") {
        Ok(s) => s.split(',').map(|x| x.trim().parse().unwrap()).collect(),
        Err(_) => vec![4_000_000, 5_000_000, 8_000_000],
    }
}

/// A doc of `size` bytes written by peer 1 in commits of `piece` bytes.
fn build(size: usize, piece: usize) -> LoroDoc {
    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    let text = doc.get_text("note");
    let content = gen_text(size);
    let mut at = 0;
    while at < size {
        let end = (at + piece).min(size);
        text.insert(text.len_unicode(), &content[at..end]).unwrap();
        doc.commit();
        at = end;
    }
    doc
}

/// Median and 95th percentile, in milliseconds, of `n` one-character commits.
fn type_into(doc: &LoroDoc, n: usize, subscribe: bool) -> (f64, f64) {
    let _sub = subscribe.then(|| doc.subscribe_local_update(Box::new(|_| true)));
    let text = doc.get_text("note");
    let middle = text.len_unicode() / 2;
    let mut times = Vec::with_capacity(n);
    for pos in middle..middle + n {
        let s = Instant::now();
        text.insert(pos, "x").unwrap();
        doc.commit();
        times.push(s.elapsed().as_secs_f64() * 1000.0);
    }
    times.sort_by(|a, b| a.partial_cmp(b).unwrap());
    (times[n / 2], times[n * 95 / 100])
}

fn from_snapshot(src: &LoroDoc, peer: u64) -> LoroDoc {
    let doc = LoroDoc::new();
    doc.set_peer_id(peer).unwrap();
    doc.import(&src.export(ExportMode::Snapshot).unwrap())
        .unwrap();
    doc
}

/// The updates a snapshot-loaded doc hands its subscriber are the same bytes an
/// explicit export gives, and another doc can apply them, whichever blocks of
/// the history are in memory at the time.
#[test]
fn local_updates_after_snapshot_match_explicit_exports() {
    use std::sync::{Arc, Mutex};
    for (size, piece) in [(20_000, 20_000), (20_000, 512), (300_000, 4096)] {
        let src = build(size, piece);
        let snapshot = src.export(ExportMode::Snapshot).unwrap();
        // Peer 1 continues its own history; peers 0 and 2 sort before and after it.
        for peer in [0, 1, 2] {
            let doc = from_snapshot(&src, peer);
            let blobs = Arc::new(Mutex::new(Vec::<Vec<u8>>::new()));
            let sink = blobs.clone();
            let _sub = doc.subscribe_local_update(Box::new(move |b| {
                sink.lock().unwrap().push(b.to_vec());
                true
            }));
            let text = doc.get_text("note");
            let mut explicit = Vec::new();
            for i in 0..40 {
                let before = doc.oplog_vv();
                if i % 7 == 3 {
                    text.delete(i * 5, 3).unwrap();
                } else {
                    text.insert(i * 11, "typed").unwrap();
                }
                doc.commit();
                explicit.push(doc.export(ExportMode::updates(&before)).unwrap());
                if i == 20 {
                    // Reading old history loads more blocks in the middle of the run.
                    let _ = doc.export(ExportMode::all_updates()).unwrap();
                }
            }
            let blobs = blobs.lock().unwrap().clone();
            assert_eq!(blobs, explicit, "size {size} piece {piece} peer {peer}");

            let other = LoroDoc::new();
            other.import(&snapshot).unwrap();
            for b in &blobs {
                other.import(b).unwrap();
            }
            assert_eq!(other.get_text("note").to_string(), text.to_string());
            assert_eq!(other.oplog_vv(), doc.oplog_vv());
            // The whole history still exports and reloads.
            let reloaded = LoroDoc::new();
            reloaded
                .import(&doc.export(ExportMode::all_updates()).unwrap())
                .unwrap();
            assert_eq!(reloaded.get_text("note").to_string(), text.to_string());
        }
    }
}

#[test]
#[ignore]
fn perf_typing_after_snapshot() {
    for size in sizes() {
        for (label, piece) in [("one insert", size), ("4 KB commits", 4096)] {
            let src = build(size, piece);
            let fresh = type_into(&build(size, piece), 200, true);
            let above = type_into(&from_snapshot(&src, 2), 200, true);
            let below = type_into(&from_snapshot(&src, 0), 200, true);
            let unsubscribed = type_into(&from_snapshot(&src, 2), 200, false);
            let _ = src.export(ExportMode::Snapshot).unwrap();
            let exported = type_into(&src, 200, true);
            println!(
                "{size} B, {label}: never snapshotted {:.3}/{:.3} ms, after exporting a snapshot {:.3}/{:.3} ms, loaded from a snapshot (peer above) {:.3}/{:.3} ms, (peer below) {:.3}/{:.3} ms, (no subscriber) {:.3}/{:.3} ms",
                fresh.0, fresh.1, exported.0, exported.1, above.0, above.1, below.0, below.1,
                unsubscribed.0, unsubscribed.1
            );
        }
    }
}

//! Ad-hoc perf probe for `import_batch` / `import` rollback bookkeeping.
//!
//! Run on two revisions and compare. Not part of CI.
use loro::{ExportMode, LoroDoc};
use std::time::{Duration, Instant};

fn make_blobs(n_blobs: usize, ops_per_blob: usize, peer: u64) -> Vec<Vec<u8>> {
    let doc = LoroDoc::new();
    doc.set_peer_id(peer).unwrap();
    let text = doc.get_text("text");
    let map = doc.get_map("map");
    let list = doc.get_list("list");
    let mut blobs = Vec::with_capacity(n_blobs);
    let mut last_vv = doc.oplog_vv();
    for i in 0..n_blobs {
        for j in 0..ops_per_blob {
            text.insert(text.len_unicode().min(j), "hello world ").unwrap();
            map.insert(&format!("k{}", (i * ops_per_blob + j) % 64), j as i64)
                .unwrap();
            list.push(j as i64).unwrap();
        }
        doc.commit();
        let vv = doc.oplog_vv();
        blobs.push(doc.export(ExportMode::updates(&last_vv)).unwrap());
        last_vv = vv;
    }
    blobs
}

/// Target doc that already has its own history, so the rollback journal is armed
/// (`import_rollback_has_journal` is only set when the pre-import vv is non-empty).
fn make_target(ops: usize) -> LoroDoc {
    let doc = LoroDoc::new();
    doc.set_peer_id(999).unwrap();
    let text = doc.get_text("text");
    let map = doc.get_map("map");
    for i in 0..ops {
        text.insert(0, "seed ").unwrap();
        map.insert(&format!("s{}", i % 32), i as i64).unwrap();
    }
    doc.commit();
    doc
}

fn bench<F: FnMut()>(name: &str, iters: usize, mut f: F) {
    // warmup
    f();
    let mut samples = Vec::with_capacity(iters);
    for _ in 0..iters {
        let t = Instant::now();
        f();
        samples.push(t.elapsed());
    }
    samples.sort();
    let median = samples[samples.len() / 2];
    let min = samples[0];
    let total: Duration = samples.iter().sum();
    println!(
        "{name:<44} median {:>10.3?}  min {:>10.3?}  mean {:>10.3?}",
        median,
        min,
        total / samples.len() as u32
    );
}

fn main() {
    let seed_ops = 2000;

    // A: many small blobs (the "drain the sync queue" shape).
    let small = make_blobs(200, 5, 1);
    bench("A import_batch 200 blobs x 5 ops", 20, || {
        let doc = make_target(seed_ops);
        doc.import_batch(&small).unwrap();
        std::hint::black_box(doc.oplog_vv());
    });

    // A': same payload, one-by-one import (control: not the batch path).
    bench("A' import x200 (one by one)", 20, || {
        let doc = make_target(seed_ops);
        for b in &small {
            doc.import(b).unwrap();
        }
        std::hint::black_box(doc.oplog_vv());
    });

    // B: few large blobs.
    let large = make_blobs(8, 400, 2);
    bench("B import_batch 8 blobs x 400 ops", 20, || {
        let doc = make_target(seed_ops);
        doc.import_batch(&large).unwrap();
        std::hint::black_box(doc.oplog_vv());
    });

    // C: reversed order -> every blob but the last parks in pending_changes and is
    // unlocked later in the same batch. This is what the pending-rollback log change
    // touches.
    let mut reversed = small.clone();
    reversed.reverse();
    bench("C import_batch 200 blobs reversed (pending)", 20, || {
        let doc = make_target(seed_ops);
        doc.import_batch(&reversed).unwrap();
        std::hint::black_box(doc.oplog_vv());
    });

    let mut reversed_large = large.clone();
    reversed_large.reverse();
    bench("C' import_batch 8 large blobs reversed", 20, || {
        let doc = make_target(seed_ops);
        doc.import_batch(&reversed_large).unwrap();
        std::hint::black_box(doc.oplog_vv());
    });

    // D: batch into an empty doc (journal disarmed on both revisions).
    bench("D import_batch 200 blobs into empty doc", 20, || {
        let doc = LoroDoc::new();
        doc.import_batch(&small).unwrap();
        std::hint::black_box(doc.oplog_vv());
    });

    // E: single-blob import in a loop, the plain sync path. Only touched by the
    // added `has_import_rollback()` check.
    let stream = make_blobs(2000, 1, 3);
    bench("E import x2000 single-change blobs", 5, || {
        let doc = make_target(seed_ops);
        for b in &stream {
            doc.import(b).unwrap();
        }
        std::hint::black_box(doc.oplog_vv());
    });

    // F: snapshot import (unrelated control).
    let snap = {
        let doc = make_target(20000);
        doc.export(ExportMode::Snapshot).unwrap()
    };
    bench("F import snapshot (control)", 20, || {
        let doc = LoroDoc::new();
        doc.import(&snap).unwrap();
        std::hint::black_box(doc.oplog_vv());
    });
}

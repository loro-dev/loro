//! Perf regression scenarios for the replay base of concurrent imports.
//!
//! Regression: with a single-head-only critical version fallback, the replay
//! base of every concurrent import on a criss-cross sync DAG stays stuck at
//! the initial fork point, so the richtext diff replays (and rebuilds its
//! tracker from) the entire accumulated history: per-import cost grows
//! without bound. With the multi-head critical version as replay base, only
//! the ops since the last sync are replayed.
//!
//! Measured (Apple M-series, --release), before -> after the multi-head base:
//!
//! | scenario                                          | before    | after    |
//! |---------------------------------------------------|-----------|----------|
//! | ping-pong 400 x 25/side, round-399 (both dirs)    | ~68 ms    | 0.11 ms  |
//! | ping-pong 400 x 25 ops/side, whole session        | ~12.5 s   | 55 ms    |
//! | 1 concurrent char into 40k-op history, old fork   | ~46 ms    | 24 ms    |
//! | ... again after a fresh sync point                | ~52 ms    | 0.05 ms  |
//!
//! (The one-way stream control stays within noise, ~0.1-0.3 ms.)
//!
//! Run with:
//! cargo test -p loro --release perf_concurrent_import_ping_pong -- --ignored --nocapture --test-threads=1
//! cargo test -p loro --release perf_single_concurrent_edit_after_long_history -- --ignored --nocapture --test-threads=1
//!
//! Scale the ping-pong with:
//! LORO_PERF_ROUNDS=1000 LORO_PERF_OPS=50 cargo test -p loro --release perf_concurrent_import_ping_pong -- --ignored --nocapture --test-threads=1

use loro::{ExportMode, LoroDoc};
use std::time::{Duration, Instant};

struct Lcg(u64);
impl Lcg {
    fn below(&mut self, n: usize) -> usize {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let f = ((self.0 >> 11) as f64) / ((1u64 << 53) as f64);
        (f * n as f64) as usize
    }
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

/// Two devices in a live editing session on a 100 KB note: both type, sync
/// after every round. Every import is a concurrent import. The one-way phase
/// first runs the same volume without concurrency as a control (its imports
/// use the Linear fast path and were always cheap).
#[test]
#[ignore]
fn perf_concurrent_import_ping_pong() {
    let rounds: usize = std::env::var("LORO_PERF_ROUNDS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(400);
    let k: usize = std::env::var("LORO_PERF_OPS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(25);

    // --- control: one-way stream, no concurrency ---
    let doc_a = LoroDoc::new();
    doc_a.set_peer_id(1).unwrap();
    let doc_b = LoroDoc::new();
    doc_b.set_peer_id(2).unwrap();
    let text_a = doc_a.get_text("t");
    text_a.insert(0, &"a".repeat(100 * 1024)).unwrap();
    doc_a.commit();
    doc_b
        .import(&doc_a.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    let mut rng = Lcg(99);
    let mut worst = Duration::ZERO;
    for _ in 0..rounds {
        for _ in 0..k {
            let pa = rng.below(text_a.len_unicode() + 1);
            text_a.insert(pa, "x").unwrap();
        }
        doc_a.commit();
        let ua = doc_a
            .export(ExportMode::updates(&doc_b.oplog_vv()))
            .unwrap();
        let t = Instant::now();
        doc_b.import(&ua).unwrap();
        worst = worst.max(t.elapsed());
    }
    println!(
        "one-way control: {rounds} rounds x {k} ops, worst_import={:.3}ms",
        ms(worst)
    );

    // --- ping-pong: both sides edit every round ---
    let doc_a = LoroDoc::new();
    doc_a.set_peer_id(1).unwrap();
    let doc_b = LoroDoc::new();
    doc_b.set_peer_id(2).unwrap();
    let text_a = doc_a.get_text("t");
    let text_b = doc_b.get_text("t");
    text_a.insert(0, &"a".repeat(100 * 1024)).unwrap();
    doc_a.commit();
    doc_b
        .import(&doc_a.export(ExportMode::all_updates()).unwrap())
        .unwrap();

    let mut rng = Lcg(99);
    let mut worst_import = Duration::ZERO;
    let mut worst_export = Duration::ZERO;
    let start = Instant::now();
    for round in 0..rounds {
        for _ in 0..k {
            let pa = rng.below(text_a.len_unicode() + 1);
            text_a.insert(pa, "x").unwrap();
            let pb = rng.below(text_b.len_unicode() + 1);
            text_b.insert(pb, "y").unwrap();
        }
        doc_a.commit();
        doc_b.commit();
        let t = Instant::now();
        let ua = doc_a
            .export(ExportMode::updates(&doc_b.oplog_vv()))
            .unwrap();
        let ub = doc_b
            .export(ExportMode::updates(&doc_a.oplog_vv()))
            .unwrap();
        worst_export = worst_export.max(t.elapsed());
        let t = Instant::now();
        doc_b.import(&ua).unwrap();
        doc_a.import(&ub).unwrap();
        let d = t.elapsed();
        worst_import = worst_import.max(d);
        if round % 50 == 0 || round == rounds - 1 {
            println!("round {:>4}: import={:>8.3}ms", round, ms(d));
        }
    }
    println!(
        "ping-pong: {rounds} rounds x {k} ops/side  total={:.1}ms  worst_export={:.3}ms  worst_import={:.3}ms  len={}",
        ms(start.elapsed()),
        ms(worst_export),
        ms(worst_import),
        text_a.len_unicode()
    );
    assert_eq!(text_a.to_string(), text_b.to_string());
}

/// The broad realistic case: a note with a long (purely sequential) history
/// gets ONE small concurrent edit from a second device that synced long ago.
/// The import legitimately pays O(divergence) once — the fork point IS the
/// latest critical version there — after which a fresh sync point makes the
/// next concurrent import O(1)-ish again.
#[test]
#[ignore]
fn perf_single_concurrent_edit_after_long_history() {
    for &n in &[10_000usize, 20_000, 40_000] {
        let doc_a = LoroDoc::new();
        doc_a.set_peer_id(1).unwrap();
        let text_a = doc_a.get_text("t");
        let mut rng = Lcg(3);
        text_a.insert(0, &"a".repeat(1000)).unwrap();
        doc_a.commit();

        // Second device synced early, then went offline.
        let doc_b = LoroDoc::new();
        doc_b.set_peer_id(2).unwrap();
        doc_b
            .import(&doc_a.export(ExportMode::all_updates()).unwrap())
            .unwrap();
        let b_vv = doc_a.oplog_vv();

        // Long sequential typing history on device A.
        for _ in 0..n {
            let len = text_a.len_unicode();
            text_a.insert(rng.below(len + 1), "x").unwrap();
            doc_a.commit();
        }

        // Device B comes back online with one offline edit.
        doc_b.get_text("t").insert(0, "z").unwrap();
        doc_b.commit();
        let ub = doc_b.export(ExportMode::updates(&b_vv)).unwrap();
        let t = Instant::now();
        doc_a.import(&ub).unwrap();
        println!(
            "history n={:>6}: import of 1 concurrent char (old fork) = {:>8.3}ms (update bytes={})",
            n,
            ms(t.elapsed()),
            ub.len()
        );

        // After a full sync, the next concurrent edit only replays the ops
        // since that sync point.
        doc_b
            .import(
                &doc_a
                    .export(ExportMode::updates(&doc_b.oplog_vv()))
                    .unwrap(),
            )
            .unwrap();
        let b_vv2 = doc_a.oplog_vv();
        for _ in 0..10 {
            let len = text_a.len_unicode();
            text_a.insert(rng.below(len + 1), "x").unwrap();
            doc_a.commit();
        }
        doc_b.get_text("t").insert(0, "w").unwrap();
        doc_b.commit();
        let ub2 = doc_b.export(ExportMode::updates(&b_vv2)).unwrap();
        let t = Instant::now();
        doc_a.import(&ub2).unwrap();
        println!(
            "history n={:>6}: import of 1 concurrent char (fresh sync point) = {:>8.3}ms",
            n,
            ms(t.elapsed())
        );
        assert_eq!(
            doc_a.get_text("t").len_unicode(),
            1000 + n + 10 + 2,
            "unexpected final length"
        );
    }
}

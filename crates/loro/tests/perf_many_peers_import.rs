//! Concurrent imports into a doc that has seen many peer ids.
//!
//! Regression: `try_shrink_frontiers` walked the DAG back from each kept head
//! for every version-vector entry, down to that entry's lamport. The import
//! of a concurrent change turns its replay base (a version vector over every
//! peer the doc has seen) into frontiers, so each import cost
//! O(peers x history). An app that starts a new peer id per session hits
//! this: two docs, same edits, a fresh peer every 25 commits instead of one
//! peer each. The walk now stops at nodes whose version vector is cached,
//! which the import path fills for the heads it builds on.
//!
//! Measured (Windows, x86-64, --release, LORO_PERF_ROUNDS=3000), mean import
//! over the last 500 rounds, before -> after:
//!
//! | scenario                              | before    | after    |
//! |---------------------------------------|-----------|----------|
//! | 2 peers                               | 0.017 ms  | 0.016 ms |
//! | 240 peers (fresh peer every 25)       | 164 ms    | 0.17 ms  |
//! | 240 peers, whole session              | 378 s     | 0.70 s   |
//!
//! Run with:
//! cargo test -p loro --release perf_concurrent_import_many_peers -- --ignored --nocapture --test-threads=1

use loro::{ExportMode, Frontiers, LoroDoc};
use std::time::{Duration, Instant};

struct Lcg(u64);
impl Lcg {
    fn below(&mut self, n: usize) -> usize {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((self.0 >> 33) as usize) % n
    }
}

fn sorted(frontiers: &Frontiers) -> Vec<(u64, i32)> {
    let mut ids: Vec<_> = frontiers.iter().map(|id| (id.peer, id.counter)).collect();
    ids.sort();
    ids
}

/// Two docs type into one text and swap changes after every commit; every
/// `launch` commits each takes a fresh peer id. Returns the mean import time
/// of the last 500 rounds.
fn ping_pong(rounds: usize, launch: Option<usize>) -> Duration {
    let docs = [LoroDoc::new(), LoroDoc::new()];
    let mut next_peer = 1;
    let mut rng = Lcg(1);
    let mut last = Duration::ZERO;
    for round in 0..rounds {
        if round == 0 || launch.is_some_and(|k| round % k == 0) {
            for d in &docs {
                d.set_peer_id(next_peer).unwrap();
                next_peer += 1;
            }
        }
        let synced = docs[0].oplog_vv();
        for d in &docs {
            let text = d.get_text("t");
            text.insert(rng.below(text.len_unicode() + 1), "ab")
                .unwrap();
            d.commit();
        }
        let updates = docs
            .each_ref()
            .map(|d| d.export(ExportMode::updates(&synced)).unwrap());
        for (i, d) in docs.iter().enumerate() {
            let t = Instant::now();
            d.import(&updates[1 - i]).unwrap();
            if round + 500 >= rounds {
                last += t.elapsed();
            }
        }
    }
    assert_eq!(
        docs[0].get_text("t").to_string(),
        docs[1].get_text("t").to_string()
    );
    last / 1000
}

#[test]
#[ignore]
fn perf_concurrent_import_many_peers() {
    let rounds: usize = std::env::var("LORO_PERF_ROUNDS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1500);
    let fixed = ping_pong(rounds, None);
    let many = ping_pong(rounds, Some(25));
    println!(
        "{rounds} rounds, mean import over the last 500: 2 peers {:.3} ms, {} peers {:.3} ms",
        fixed.as_secs_f64() * 1000.0,
        2 * rounds.div_ceil(25),
        many.as_secs_f64() * 1000.0
    );
}

/// `vv_to_frontiers` keeps exactly the concurrent heads of any version, on a
/// doc with many peers and many forks, with the version vectors on the DAG
/// cached (the doc that made the history) and not (a doc fresh from a
/// snapshot).
#[test]
fn vv_to_frontiers_finds_heads_with_many_peers() {
    let docs: Vec<LoroDoc> = (0..3).map(|_| LoroDoc::new()).collect();
    let mut next_peer = 1;
    let mut rng = Lcg(7);
    let mut seen: Vec<Frontiers> = Vec::new();
    for round in 0..300 {
        let d = &docs[rng.below(3)];
        if round % 7 == 0 {
            d.set_peer_id(next_peer).unwrap();
            next_peer += 1;
        }
        let text = d.get_text("t");
        text.insert(rng.below(text.len_unicode() + 1), "x").unwrap();
        d.commit();
        if rng.below(3) == 0 {
            let (a, b) = (rng.below(3), rng.below(3));
            docs[a]
                .import(
                    &docs[b]
                        .export(ExportMode::updates(&docs[a].oplog_vv()))
                        .unwrap(),
                )
                .unwrap();
        }
        seen.push(d.oplog_frontiers());
    }
    for i in 1..3 {
        docs[0]
            .import(
                &docs[i]
                    .export(ExportMode::updates(&docs[0].oplog_vv()))
                    .unwrap(),
            )
            .unwrap();
    }
    seen.push(docs[0].oplog_frontiers());
    let versions: Vec<_> = seen
        .iter()
        .map(|f| (sorted(f), docs[0].frontiers_to_vv(f).unwrap()))
        .collect();
    let cold = LoroDoc::new();
    cold.import(&docs[0].export(ExportMode::Snapshot).unwrap())
        .unwrap();
    for doc in [&cold, &docs[0]] {
        for (heads, vv) in versions.iter().rev() {
            assert_eq!(&sorted(&doc.vv_to_frontiers(vv)), heads);
        }
    }
}

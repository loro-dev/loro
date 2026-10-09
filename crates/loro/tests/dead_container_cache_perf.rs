//! Run with `CARGO_BUILD_JOBS=4 cargo test --release -p loro
//! --test dead_container_cache_perf -- --ignored --nocapture`.

use loro::{ContainerTrait, LoroDoc, TreeParentId};
use std::{hint::black_box, time::Instant};

fn measure(n: usize) -> (f64, f64) {
    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    let tree = doc.get_tree("tree");
    let deleted: Vec<_> = (0..n)
        .map(|_| tree.create(TreeParentId::Root).unwrap())
        .collect();
    for id in &deleted {
        tree.delete(*id).unwrap();
    }
    let p1 = tree.create(TreeParentId::Root).unwrap();
    let p2 = tree.create(TreeParentId::Root).unwrap();
    let target = tree.create(p1).unwrap();
    doc.commit();
    let metas: Vec<_> = deleted
        .iter()
        .map(|id| tree.get_meta(*id).unwrap())
        .collect();

    // Grow the cache to n entries, warm invalidation, then refill it. A clear
    // that retains the peak allocation scans that table on every later move.
    for meta in &metas {
        assert!(meta.is_deleted());
    }
    for _ in 0..8 {
        assert!(black_box(metas[0].is_deleted()));
        tree.mov(target, p2).unwrap();
        tree.mov(target, p1).unwrap();
    }
    for meta in &metas {
        assert!(meta.is_deleted());
    }

    let start = Instant::now();
    for i in 0..n {
        assert!(black_box(metas[0].is_deleted()));
        tree.mov(target, if i % 2 == 0 { p2 } else { p1 }).unwrap();
    }
    let interleaved = start.elapsed().as_secs_f64();

    // Same moves without deletion queries, to expose unrelated machine load.
    let start = Instant::now();
    for i in 0..n {
        tree.mov(target, if i % 2 == 0 { p2 } else { p1 }).unwrap();
    }
    let control = start.elapsed().as_secs_f64();
    for meta in &metas {
        assert!(meta.is_deleted());
    }
    assert!(!tree.is_node_deleted(&target).unwrap());
    (interleaved, control)
}

fn median(samples: &mut [f64]) -> f64 {
    samples.sort_by(f64::total_cmp);
    samples[samples.len() / 2]
}

#[test]
#[ignore = "release-only scaling measurement; builds 32k and 128k deleted nodes"]
fn is_deleted_interleaved_with_moves() {
    assert!(!cfg!(debug_assertions), "run this test with --release");
    let n = 32_000;
    let mut small = Vec::new();
    let mut large = Vec::new();
    // Alternate sizes to reduce drift from other sessions sharing the host.
    for run in 0..3 {
        for size in if run % 2 == 0 { [n, 4 * n] } else { [4 * n, n] } {
            let (secs, control) = measure(size);
            println!("RESULT n={size} run={run} secs={secs:.6} control_secs={control:.6}");
            if size == n {
                small.push(secs);
            } else {
                large.push(secs);
            }
        }
    }
    let ratio = median(&mut large) / median(&mut small);
    println!("RESULT scaling_ratio={ratio:.3}");
    // Linear work should scale near 4x; allow host noise while rejecting the
    // retained-table regression, whose repeated clearing approaches 16x.
    assert!(ratio < 8.0, "time(4n)/time(n) = {ratio:.3}, expected < 8");
}

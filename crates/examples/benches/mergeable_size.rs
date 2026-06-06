//! Measure the wire-size cost of mergeable containers.
//!
//! Two dimensions to characterize:
//!   1. **Cid size** — how many bytes a mergeable `ContainerID::to_bytes()` takes as a function of
//!      mergeable-map nesting depth and key length. Cids are embedded in op headers, snapshots,
//!      and event payloads, so this scales with every op that references the cid.
//!   2. **Snapshot size** — how a snapshot containing N mergeable counters compares to one with
//!      N regular `set_container`'d counters, to surface the on-disk overhead.
//!
//! This is a measurement bench, not a perf bench: it prints sizes to stdout and exits. Run with
//! `cargo run --release --bin mergeable_size --manifest-path crates/examples/Cargo.toml` after
//! adding to the `bench` list, or invoke directly via criterion's `cargo bench` machinery.

use criterion::{criterion_group, criterion_main, Criterion};
use loro::{ContainerID, ContainerType, ExportMode, LoroDoc, LoroMap};

fn nested_mergeable_cid(depth: usize, key: &str) -> ContainerID {
    let mut cid = ContainerID::new_root("root", ContainerType::Map);
    for _ in 0..depth {
        cid = ContainerID::new_mergeable(&cid, key, ContainerType::Map);
    }
    // Final leaf is a Counter under the deepest map.
    ContainerID::new_mergeable(&cid, key, ContainerType::Counter)
}

fn measure_cid_size_by_depth() {
    println!("\n=== Cid byte size by mergeable nesting depth ===");
    println!("depth  key_len   cid_bytes");
    for &key_len in &[4usize, 16, 64] {
        let key: String = std::iter::repeat('k').take(key_len).collect();
        for &depth in &[0usize, 1, 2, 3, 5, 8] {
            let cid = nested_mergeable_cid(depth, &key);
            let bytes = cid.to_bytes().len();
            println!("{depth:>5}  {key_len:>7}   {bytes:>9}");
        }
    }
}

fn build_doc_with_mergeable_counters(n: usize) -> LoroDoc {
    let doc = LoroDoc::new();
    let map: LoroMap = doc.get_map("state");
    for i in 0..n {
        let counter = map
            .ensure_mergeable_counter(&format!("counter_{i}"))
            .unwrap();
        counter.increment(i as f64).unwrap();
    }
    doc.commit();
    doc
}

fn build_doc_with_regular_counters(n: usize) -> LoroDoc {
    let doc = LoroDoc::new();
    let map: LoroMap = doc.get_map("state");
    for i in 0..n {
        let counter = doc
            .get_map("state")
            .insert_container(&format!("counter_{i}"), loro::LoroCounter::new())
            .unwrap();
        counter.increment(i as f64).unwrap();
        let _ = map; // satisfy borrow
    }
    doc.commit();
    doc
}

fn measure_snapshot_size_mergeable_vs_regular() {
    println!("\n=== Snapshot byte size: mergeable vs regular counters ===");
    println!("n_counters   regular_snapshot   mergeable_snapshot   delta_per_counter");
    for &n in &[1usize, 10, 100, 1000] {
        let regular = build_doc_with_regular_counters(n);
        let mergeable = build_doc_with_mergeable_counters(n);
        let r_bytes = regular.export(ExportMode::Snapshot).unwrap().len();
        let m_bytes = mergeable.export(ExportMode::Snapshot).unwrap().len();
        let delta_per = (m_bytes as i64 - r_bytes as i64) / n as i64;
        println!("{n:>10}   {r_bytes:>16}   {m_bytes:>18}   {delta_per:>17}");
    }
}

fn measure(_c: &mut Criterion) {
    measure_cid_size_by_depth();
    measure_snapshot_size_mergeable_vs_regular();
}

criterion_group!(benches, measure);
criterion_main!(benches);

//! `checkout`/`fork_at` harness for documents with many containers
//! (regression harness for loro-dev/loro#1056).
//!
//! The document is edited by peer 1 while peer 2 holds a concurrent branch, so the
//! current frontiers have two elements and the target version is a strict subset of
//! them. That is the shape where a degenerate common ancestor makes the diff
//! calculator replay the whole history for every container.
//!
//! Usage:
//!   cargo run --release -p examples --example fork_at_many_containers [containers] [edits]
use std::time::Instant;

use loro::{ExportMode, LoroDoc};

fn main() {
    let containers: usize = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(100);
    let edits: usize = std::env::args()
        .nth(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(50);

    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    let map = doc.get_map("blocks");
    let texts: Vec<_> = (0..containers)
        .map(|i| {
            map.insert_container(&format!("b{i}"), loro::LoroText::new())
                .unwrap()
        })
        .collect();
    doc.commit();

    // A concurrent peer that branches off before peer 1's edits.
    let other = LoroDoc::new();
    other
        .import(&doc.export(ExportMode::Snapshot).unwrap())
        .unwrap();
    other.set_peer_id(2).unwrap();

    for _round in 0..edits {
        for t in texts.iter() {
            t.insert(t.len_unicode(), "hello ").unwrap();
            doc.commit();
        }
    }
    doc.commit();

    let target = doc.oplog_frontiers();
    let total_ops = doc.oplog_vv().get(&1).copied().unwrap();

    other.get_map("blocks").insert("x", 1).unwrap();
    other.commit();
    doc.import(&other.export(ExportMode::all_updates()).unwrap())
        .unwrap();

    println!("containers={containers} ops={total_ops}");
    let start = Instant::now();
    doc.checkout(&target).unwrap();
    println!("checkout took {:?}", start.elapsed());
    doc.attach();

    let start = Instant::now();
    let forked = doc.fork_at(&target).unwrap();
    println!(
        "fork_at took {:?} (first block len={})",
        start.elapsed(),
        forked
            .get_map("blocks")
            .get("b0")
            .unwrap()
            .into_container()
            .unwrap()
            .into_text()
            .unwrap()
            .len_unicode()
    );
}

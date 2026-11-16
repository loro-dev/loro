//! Native global-budget commit cost as the number of live documents grows.
//! Run with `cargo run -p loro --release --example global_budget_scaling_bench`.
//!
//! A 1 TiB global heap quota (never reached) is set, `docs` documents are kept
//! alive, and one of them commits small edits. Columns are microseconds per
//! commit. `read_then` reads a different idle document before every commit, so
//! the global check has one more document to refresh.
use loro::{set_global_memory_budget, GlobalMemoryBudget, LoroDoc};
use std::time::Instant;

const COMMITS: usize = 400;

fn per_commit_us(docs: &[LoroDoc], mode: &str) -> f64 {
    let doc = &docs[0];
    let map = doc.get_map("m");
    let start = Instant::now();
    for i in 0..COMMITS {
        if mode == "read_then" && docs.len() > 1 {
            let other = &docs[1 + i % (docs.len() - 1)];
            let _ = other.get_map("m").get("v");
        }
        map.insert("v", i as i64).unwrap();
        match mode {
            "commit" => doc.commit(),
            _ => doc.try_commit().unwrap(),
        }
    }
    start.elapsed().as_secs_f64() * 1e6 / COMMITS as f64
}

fn main() {
    set_global_memory_budget(GlobalMemoryBudget::new().with_max_heap_bytes(1 << 40));
    println!("docs,round,try_commit_us,commit_us,read_then_try_commit_us");
    for round in 0..3 {
        for n in [1usize, 100, 1000] {
            let docs: Vec<LoroDoc> = (0..n)
                .map(|i| {
                    let d = LoroDoc::new();
                    d.set_peer_id(i as u64 + 1).unwrap();
                    d.get_map("m").insert("v", 0).unwrap();
                    d.get_text("t").insert(0, "hello world").unwrap();
                    d.commit();
                    d
                })
                .collect();
            let t = per_commit_us(&docs, "try_commit");
            let c = per_commit_us(&docs, "commit");
            let r = per_commit_us(&docs, "read_then");
            println!("{n},{round},{t:.2},{c:.2},{r:.2}");
        }
    }
}

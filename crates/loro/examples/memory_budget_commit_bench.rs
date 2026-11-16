//! Narrow native benchmark: 2,000 commits to one of 10,000 materialized maps.
//! Run with `cargo run -p loro --release --example memory_budget_commit_bench`.
use loro::{LoroDoc, MemoryBudget};
use std::time::Instant;

fn main() {
    println!("mode,round,commits,elapsed_ms,total_bytes");
    for round in 0..7 {
        for budgeted in [false, true] {
            let doc = LoroDoc::new();
            doc.set_peer_id(1).unwrap();
            for i in 0..10_000 {
                doc.get_map(format!("m{i}")).insert("v", 0).unwrap();
            }
            doc.commit();
            if budgeted {
                doc.set_memory_budget(MemoryBudget::new().with_max_doc_bytes(1 << 30));
            }
            let map = doc.get_map("m0");
            let start = Instant::now();
            for i in 1..=2_000 {
                map.insert("v", i).unwrap();
                doc.commit();
            }
            let elapsed = start.elapsed().as_secs_f64() * 1000.0;
            assert_eq!(
                map.get("v").unwrap().into_value().unwrap(),
                loro::LoroValue::I64(2_000)
            );
            let stats = doc.memory_stats();
            assert!(stats.total_bytes < 1 << 30);
            println!(
                "{},{round},2000,{elapsed:.6},{}",
                if budgeted { "doc" } else { "none" },
                stats.total_bytes
            );
        }
    }
}

//! Local editing cost with an undo manager attached (which now records each
//! op's inverse). Run with
//! `cargo run -p loro --release --example undo_edit_overhead_bench`.
use loro::{LoroDoc, UndoManager};
use std::time::Instant;

fn main() {
    println!("scenario,undo_manager,ms");
    for round in 0..3 {
        for with_undo in [false, true] {
            let doc = LoroDoc::new();
            doc.set_peer_id(1).unwrap();
            let _undo = with_undo.then(|| UndoManager::new(&doc));
            let text = doc.get_text("t");
            let start = Instant::now();
            for i in 0..20_000 {
                text.insert(i, "a").unwrap();
                doc.commit();
            }
            let typing = start.elapsed().as_secs_f64() * 1000.0;
            let start = Instant::now();
            for _ in 0..10_000 {
                text.delete(text.len_unicode() / 2, 1).unwrap();
                doc.commit();
            }
            let deleting = start.elapsed().as_secs_f64() * 1000.0;
            let map = doc.get_map("m");
            let start = Instant::now();
            for i in 0..20_000 {
                map.insert(&format!("k{}", i % 100), i as i64).unwrap();
                doc.commit();
            }
            let map_ms = start.elapsed().as_secs_f64() * 1000.0;
            let _ = round;
            println!("typing,{with_undo},{typing:.1}");
            println!("deleting,{with_undo},{deleting:.1}");
            println!("map_writes,{with_undo},{map_ms:.1}");
        }
    }
}

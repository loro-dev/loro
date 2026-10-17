use loro::{LoroDoc, UndoManager};
use std::time::{Duration, Instant};

/// Run with `cargo test --release -p loro --test movable_list_undo_perf -- --ignored --nocapture`.
#[test]
#[ignore]
fn one_move_undo_in_a_large_list() {
    let mut ten_thousand = Duration::ZERO;
    for n in [10_000, 100_000] {
        let mut timings = Vec::new();
        for _ in 0..5 {
            let doc = LoroDoc::new();
            doc.set_peer_id(1).unwrap();
            let list = doc.get_movable_list("list");
            for i in 0..n {
                list.push(i as i64).unwrap();
            }
            doc.commit();
            let mut undo = UndoManager::new(&doc);
            undo.set_merge_interval(0);
            list.mov(0, n - 1).unwrap();
            doc.commit();
            let start = Instant::now();
            assert!(undo.undo().unwrap());
            assert!(undo.redo().unwrap());
            timings.push(start.elapsed());
            assert_eq!(list.get(n - 1).unwrap().into_value().unwrap(), 0.into());
            assert_eq!(list.len(), n);
        }
        timings.sort();
        let median = timings[2];
        println!("single move undo+redo n={n}: median={median:?}");
        if n == 10_000 {
            ten_thousand = median;
        } else if !cfg!(debug_assertions) {
            assert!(median < Duration::from_millis(10));
        }
    }
    // Separate from the default suite so a loaded CI host does not make
    // correctness tests flaky. In optimized builds main takes about 0.5 ms
    // for 10k; the regression was about 4.5 ms (and grows with list length).
    let budget = if cfg!(debug_assertions) { 40 } else { 2 };
    assert!(ten_thousand < Duration::from_millis(budget));
}

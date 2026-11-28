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

#[test]
#[ignore]
fn import_into_unrelated_lists_with_many_remaps() {
    use loro::ExportMode;

    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    let big = doc.get_movable_list("big");
    for i in 0..10_000 {
        big.push(i as i64).unwrap();
    }
    for i in 0..8 {
        doc.get_movable_list(format!("u{i}")).push(0).unwrap();
    }
    doc.commit();

    let remote = LoroDoc::new();
    remote.set_peer_id(2).unwrap();
    remote
        .import(&doc.export(ExportMode::Snapshot).unwrap())
        .unwrap();

    let mut undo = UndoManager::new(&doc);
    undo.set_merge_interval(0);
    // Delete all 10k elements and undo to populate 10k entries in element_remap.
    big.delete(0, 10_000).unwrap();
    doc.commit();
    assert!(undo.undo().unwrap());

    let mut updates = Vec::new();
    for iter in 0..50 {
        let from = remote.oplog_vv();
        for i in 0..8 {
            remote
                .get_movable_list(format!("u{i}"))
                .push(iter as i64 + 1)
                .unwrap();
        }
        remote.commit();
        updates.push(remote.export(ExportMode::updates(&from)).unwrap());
    }

    let mut timings = Vec::with_capacity(updates.len());
    for update in &updates {
        let start = Instant::now();
        doc.import(update).unwrap();
        timings.push(start.elapsed());
    }
    timings.sort();
    let median = timings[timings.len() / 2];
    let p90 = timings[(timings.len() * 9) / 10];
    println!("import 8 unrelated lists with 10k remaps: median={median:?}, p90={p90:?}");
}

#[test]
#[ignore]
fn revive_deep_chain_tree_under_movable_list() {
    use loro::ExportMode;

    const N: usize = 4_000;
    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    let list = doc.get_movable_list("list");
    list.push(0).unwrap();
    let tree = list.insert_container(1, loro::LoroTree::new()).unwrap();
    let mut parent = None;
    for _ in 0..N {
        parent = Some(tree.create(parent).unwrap());
    }
    doc.commit();

    let remote = LoroDoc::new();
    remote.set_peer_id(2).unwrap();
    remote
        .import(&doc.export(ExportMode::Snapshot).unwrap())
        .unwrap();
    let from = remote.oplog_vv();
    remote.get_movable_list("list").mov(1, 0).unwrap();
    remote.commit();
    let revive_update = remote.export(ExportMode::updates(&from)).unwrap();

    let mut undo = UndoManager::new(&doc);
    undo.set_merge_interval(0);
    list.delete(1, 1).unwrap();
    doc.commit();
    assert!(undo.undo().unwrap());

    let start = Instant::now();
    doc.import(&revive_update).unwrap();
    let elapsed = start.elapsed();
    println!("revive {N}-node chain tree with UndoManager: {elapsed:?}");
    if !cfg!(debug_assertions) {
        assert!(
            elapsed < Duration::from_millis(15),
            "reviving {N}-node chain tree took {elapsed:?}"
        );
    }
}

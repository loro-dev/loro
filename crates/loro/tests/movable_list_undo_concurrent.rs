use loro::{ExportMode, LoroDoc, UndoManager};
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Barrier,
};

#[test]
fn a_writer_waiting_during_undo_cannot_invalidate_its_plan() {
    let rounds = std::env::var("LORO_UNDO_CONCURRENT_ROUNDS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(128);
    let overlapped = AtomicUsize::new(0);
    for _ in 0..rounds {
        let doc = LoroDoc::new();
        doc.set_peer_id(1).unwrap();
        let list = doc.get_movable_list("list");
        for i in 0..256 {
            list.push(i).unwrap();
        }
        doc.commit();
        let mut undo = UndoManager::new(&doc);
        undo.set_merge_interval(0);
        list.mov(0, 255).unwrap();
        doc.commit();
        let start = Barrier::new(2);
        let done = AtomicBool::new(false);
        std::thread::scope(|scope| {
            scope.spawn(|| {
                start.wait();
                while !done.load(Ordering::Acquire) {
                    // Undo temporarily checks out its source versions while
                    // holding txn. Join the waiting writer queue during that
                    // interval, rather than relying on an arbitrary sleep.
                    if doc.is_detached() {
                        overlapped.fetch_add(1, Ordering::Relaxed);
                        list.delete(0, 256).unwrap();
                        break;
                    }
                    std::hint::spin_loop();
                }
            });
            start.wait();
            undo.undo().unwrap();
            done.store(true, Ordering::Release);
        });
        // Both the document and the same manager remain usable afterwards.
        undo.undo().unwrap();
        list.push(999).unwrap();
        doc.commit();
        let replica = LoroDoc::new();
        replica
            .import(&doc.export(ExportMode::all_updates()).unwrap())
            .unwrap();
        assert_eq!(doc.get_deep_value(), replica.get_deep_value());
    }
    assert!(overlapped.load(Ordering::Relaxed) > 0);
}

use loro::{ContainerTrait, ExportMode, LoroDoc, LoroError, ToJson, UndoManager};
use serde_json::json;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

fn send(a: &LoroDoc, b: &LoroDoc) {
    b.import_with(&a.export(ExportMode::all_updates()).unwrap(), "import")
        .unwrap();
}

// 1.16's mergeable-revival history naturally produces an invalid inverse.
// Main has already repaired the revival trigger; on that base this same history
// must undo successfully. Internal fault injection covers main's failure path.
#[test]
fn undo_revival_failure_is_atomic_across_containers_and_retryable() {
    for mixed in [false, true] {
        for map_first in [false, true] {
            let a = LoroDoc::new();
            a.set_peer_id(1).unwrap();
            if map_first {
                a.get_map("other").insert("base", 0).unwrap();
                a.commit();
            }
            let root = a.get_map("root");
            let la = root.ensure_mergeable_movable_list("child").unwrap();
            la.push(0).unwrap();
            a.commit();
            if !map_first {
                a.get_map("other").insert("base", 0).unwrap();
                a.commit();
            }
            let b = LoroDoc::new();
            b.set_peer_id(2).unwrap();
            send(&a, &b);
            let lb = b.get_movable_list(la.id());
            let mut ua = UndoManager::new(&a);
            ua.add_exclude_origin_prefix("import");
            let mut ub = UndoManager::new(&b);
            ub.add_exclude_origin_prefix("import");
            ub.set_merge_interval(0);
            b.get_map("other").insert("prior", 7).unwrap();
            b.commit();
            if mixed {
                b.get_map("other").insert("edited", 42).unwrap();
            }
            lb.push(1).unwrap();
            b.commit();
            root.delete("child").unwrap();
            a.commit();
            send(&a, &b);
            send(&b, &a);
            ua.undo_one().unwrap();
            send(&a, &b);
            if lb.get_value().to_json_value() == json!([0, 1]) {
                // Main's fixed mergeable revival produces a valid inverse.
                assert!(ub.undo_one().unwrap());
                assert_eq!(lb.get_value().to_json_value(), json!([0]));
                continue;
            }
            assert_eq!(lb.get_value().to_json_value(), json!([0, 0, 1]));
            let before = b.get_deep_value();
            let vv = b.oplog_vv();
            let frontiers = b.state_frontiers();
            let stacks = (ub.undo_count(), ub.redo_count());
            let events = Arc::new(AtomicUsize::new(0));
            let captured = events.clone();
            let _sub = b.subscribe_root(Arc::new(move |_| {
                captured.fetch_add(1, Ordering::Relaxed);
            }));
            for _ in 0..2 {
                assert!(matches!(ub.undo_one(), Err(LoroError::OutOfBound { .. })));
                assert_eq!(b.get_deep_value(), before);
                assert_eq!(b.oplog_vv(), vv);
                assert_eq!(b.state_frontiers(), frontiers);
                assert_eq!(b.get_pending_txn_len(), 0);
                assert_eq!((ub.undo_count(), ub.redo_count()), stacks);
                assert_eq!(events.load(Ordering::Relaxed), 0);
            }
            b.get_text("new-text").insert(0, "usable 😀").unwrap();
            b.commit();
            assert!(ub.undo_one().unwrap());
            assert_eq!(b.get_text("new-text").to_string(), "");
            assert_eq!(b.get_deep_value(), {
                let restored = LoroDoc::new();
                restored
                    .import(&b.export(ExportMode::Snapshot).unwrap())
                    .unwrap();
                restored.get_deep_value()
            });
        }
    }
}

//! Regression for #1046: a concurrent delete and edit-then-move must converge
//! even when the edit and move arrive in separate imports.
use loro::{ContainerTrait, ExportMode, Frontiers, LoroDoc, LoroMap, ToJson};
use serde_json::{json, Value};

const ORDERS: [[usize; 3]; 6] = [
    [0, 1, 2],
    [0, 2, 1],
    [1, 0, 2],
    [1, 2, 0],
    [2, 0, 1],
    [2, 1, 0],
];

fn state(doc: &LoroDoc) -> Value {
    doc.get_deep_value().to_json_value()
}

fn map_at(doc: &LoroDoc, index: usize) -> LoroMap {
    doc.get_movable_list("list")
        .get(index)
        .unwrap()
        .into_container()
        .unwrap()
        .into_map()
        .unwrap()
}

struct Scenario {
    base: LoroDoc,
    a: LoroDoc,
    b: LoroDoc,
    // A's delete, B's edit, B's move (which depends on the edit).
    updates: [Vec<u8>; 3],
}

fn scenario() -> Scenario {
    let base = LoroDoc::new();
    base.set_peer_id(1).unwrap();
    let list = base.get_movable_list("list");
    for tag in ["a", "c"] {
        let map = list.insert_container(list.len(), LoroMap::new()).unwrap();
        map.insert("id", tag).unwrap();
    }
    base.commit();
    let snapshot = base.export(ExportMode::Snapshot).unwrap();
    let a = LoroDoc::from_snapshot(&snapshot).unwrap();
    a.set_peer_id(0xa0).unwrap();
    let b = LoroDoc::from_snapshot(&snapshot).unwrap();
    b.set_peer_id(0xa1).unwrap();
    let vv = base.oplog_vv();
    a.get_movable_list("list").delete(1, 1).unwrap();
    a.commit();
    let deleted = a.export(ExportMode::updates(&vv)).unwrap();
    map_at(&b, 1).insert("contents", "zombie").unwrap();
    b.commit();
    let edit = b.export(ExportMode::updates(&vv)).unwrap();
    let vv = b.oplog_vv();
    b.get_movable_list("list").mov(1, 0).unwrap();
    b.commit();
    let moved = b.export(ExportMode::updates(&vv)).unwrap();
    Scenario {
        base,
        a,
        b,
        updates: [deleted, edit, moved],
    }
}

fn expected() -> Value {
    json!({"list": [{"id": "c", "contents": "zombie"}, {"id": "a"}]})
}

fn check_replay(doc: &LoroDoc, base: &Frontiers) {
    let current = state(doc);
    let tip = doc.state_frontiers();
    // Fresh replay is independent of the receiver's retained tracker/cache.
    let replay = LoroDoc::new();
    replay
        .import(&doc.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    assert_eq!(state(&replay), current);
    let snapshot = LoroDoc::from_snapshot(&doc.export(ExportMode::Snapshot).unwrap()).unwrap();
    assert_eq!(state(&snapshot), current);
    for d in [doc, &snapshot] {
        for _ in 0..2 {
            d.checkout(base).unwrap();
            assert_eq!(state(d), json!({"list": [{"id":"a"}, {"id":"c"}]}));
            d.checkout(&tip).unwrap();
            assert_eq!(state(d), current);
            d.checkout(base).unwrap();
            d.try_checkout_to_latest().unwrap();
            assert_eq!(state(d), current);
        }
    }
}

fn check_revived_child(doc: &LoroDoc, base: &LoroDoc) {
    assert_eq!(state(doc), expected());
    let child = map_at(doc, 0);
    assert_eq!(child.id(), map_at(base, 1).id());
    assert!(!child.is_deleted());
    // A usable revived handle, not merely the right visible JSON.
    child.insert("after", true).unwrap();
    doc.commit();
    let restored = LoroDoc::from_snapshot(&doc.export(ExportMode::Snapshot).unwrap()).unwrap();
    assert_eq!(state(doc), state(&restored));
}

#[test]
fn issue_1046_separate_edit_and_move_imports() {
    let base = LoroDoc::new();
    base.set_peer_id(1).unwrap();
    let list = base.get_movable_list("list");
    for tag in ["a", "c"] {
        let map = list.insert_container(list.len(), LoroMap::new()).unwrap();
        map.insert("id", tag).unwrap();
    }
    base.commit();
    let snapshot = base.export(ExportMode::Snapshot).unwrap();
    let a = LoroDoc::from_snapshot(&snapshot).unwrap();
    a.set_peer_id(0xa0).unwrap();
    let b = LoroDoc::from_snapshot(&snapshot).unwrap();
    b.set_peer_id(0xa1).unwrap();
    a.get_movable_list("list").delete(1, 1).unwrap();
    a.commit();
    let before_edit = b.oplog_vv();
    let child = b
        .get_movable_list("list")
        .get(1)
        .unwrap()
        .into_container()
        .unwrap()
        .into_map()
        .unwrap();
    child.insert("contents", "zombie").unwrap();
    b.commit();
    let edit = b.export(ExportMode::updates(&before_edit)).unwrap();
    let before_move = b.oplog_vv();
    b.get_movable_list("list").mov(1, 0).unwrap();
    b.commit();
    let moved = b.export(ExportMode::updates(&before_move)).unwrap();
    a.import(&edit).unwrap();
    a.import(&moved).unwrap();
    b.import(&a.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    let expected = json!({"list": [{"id":"c", "contents":"zombie"}, {"id":"a"}]});
    assert_eq!(a.get_deep_value().to_json_value(), expected);
    assert_eq!(b.get_deep_value().to_json_value(), expected);
}

#[test]
fn all_update_orders_and_checkout_round_trips() {
    let s = scenario();
    for order in ORDERS {
        // Start from the base, the deleting peer, and the editing/moving peer.
        for source in [&s.base, &s.a, &s.b] {
            let doc =
                LoroDoc::from_snapshot(&source.export(ExportMode::Snapshot).unwrap()).unwrap();
            // Hold/query the deleted child's original handle to populate the cache.
            let child = doc.get_map(map_at(&s.base, 1).id());
            let _ = child.is_deleted();
            let mut versions = vec![];
            for i in order {
                doc.import(&s.updates[i]).unwrap();
                let _ = child.is_deleted();
                versions.push((doc.state_frontiers(), state(&doc)));
                check_replay(&doc, &s.base.state_frontiers());
            }
            assert_eq!(state(&doc), expected(), "order {order:?}");
            // Include pending-dependency versions and the deleted-only version.
            for (version, value) in versions.iter().rev().chain(versions.iter()) {
                doc.checkout(version).unwrap();
                assert_eq!(&state(&doc), value);
            }
            doc.try_checkout_to_latest().unwrap();
            check_revived_child(&doc, &s.base);
        }
    }
}

#[test]
fn all_update_orders_in_batch_and_detached_imports() {
    let s = scenario();
    for order in ORDERS {
        for source in [&s.base, &s.a, &s.b] {
            for detached in [false, true] {
                let doc =
                    LoroDoc::from_snapshot(&source.export(ExportMode::Snapshot).unwrap()).unwrap();
                if detached {
                    let before = state(&doc);
                    doc.detach();
                    for i in order {
                        doc.import(&s.updates[i]).unwrap();
                        assert_eq!(state(&doc), before);
                    }
                    doc.try_attach().unwrap();
                } else {
                    doc.import_batch(&order.map(|i| s.updates[i].clone()))
                        .unwrap();
                }
                check_replay(&doc, &s.base.state_frontiers());
                check_revived_child(&doc, &s.base);
            }
        }
    }
}

#[test]
fn all_update_orders_after_shallow_snapshot() {
    let s = scenario();
    let root = s.base.state_frontiers();
    for source in [&s.base, &s.a, &s.b] {
        let bytes = source.export(ExportMode::shallow_snapshot(&root)).unwrap();
        for order in ORDERS {
            let doc = LoroDoc::from_snapshot(&bytes).unwrap();
            for i in order {
                doc.import(&s.updates[i]).unwrap();
                let value = state(&doc);
                doc.checkout(&root).unwrap();
                doc.try_checkout_to_latest().unwrap();
                assert_eq!(state(&doc), value);
            }
            assert_eq!(state(&doc), expected());
            // Compact again at the merged tip and verify the revived child survived.
            let shallow = LoroDoc::from_snapshot(
                &doc.export(ExportMode::shallow_snapshot(&doc.state_frontiers()))
                    .unwrap(),
            )
            .unwrap();
            check_revived_child(&shallow, &s.base);
            check_revived_child(&doc, &s.base);
        }
    }
}

#[test]
fn combined_edit_and_move_and_bidirectional_sync() {
    let s = scenario();
    s.a.import(&s.b.export(ExportMode::updates(&s.base.oplog_vv())).unwrap())
        .unwrap();
    s.b.import(&s.updates[0]).unwrap();
    assert_eq!(state(&s.a), expected());
    assert_eq!(state(&s.b), expected());
    assert_eq!(s.a.oplog_vv(), s.b.oplog_vv());
    for doc in [&s.a, &s.b] {
        check_replay(doc, &s.base.state_frontiers());
    }
}

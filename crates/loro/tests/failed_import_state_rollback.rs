//! A failed import must not leave `DocState` entries, handlers, or caches pointing at arena
//! indices that its rollback handed out to other containers (loro-dev/loro#1164). See
//! `context/failed-import-arena-indices.md`.

use loro::{ContainerTrait, ExportMode, LoroDoc, LoroMap, LoroText, ToJson, TreeParentId};

fn import(bytes: &[u8]) -> LoroDoc {
    let doc = LoroDoc::new();
    doc.import(bytes).unwrap();
    doc.set_peer_id(99).unwrap();
    doc
}

/// A snapshot where `c` (created by peer 2 under `p`, which peer 1 deleted concurrently) has a
/// text in its meta, and an update that revives `c`, edits its meta, and ends with a list insert
/// forged out of bounds, which the state rejects. Also returns the update before it was forged.
fn snapshot_bad_and_good_update() -> (Vec<u8>, Vec<u8>, Vec<u8>) {
    // Peer 2 creates `c` under `p` and puts a text in its meta; peer 1 deletes `p` concurrently.
    let a = LoroDoc::new();
    a.set_peer_id(1).unwrap();
    let p = a.get_tree("tree").create(TreeParentId::Root).unwrap();
    a.commit();
    let b = LoroDoc::new();
    b.set_peer_id(2).unwrap();
    b.import(&a.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    let c = b.get_tree("tree").create(p).unwrap();
    let meta = b.get_tree("tree").get_meta(c).unwrap();
    meta.insert_container("t", LoroText::new())
        .unwrap()
        .insert(0, "z")
        .unwrap();
    b.commit();
    a.get_tree("tree").delete(p).unwrap();
    a.commit();
    a.import(&b.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    a.get_map("m").insert("k", 1).unwrap();
    a.commit();
    let snap = a.export(ExportMode::Snapshot).unwrap();
    let vv = a.oplog_vv();

    // Peer 4 revives `c` and edits its meta, then a list insert forged out of bounds, which
    // the state rejects.
    let e = LoroDoc::new();
    e.set_peer_id(4).unwrap();
    e.import(&snap).unwrap();
    e.get_tree("tree").mov(c, TreeParentId::Root).unwrap();
    e.get_tree("tree")
        .get_meta(c)
        .unwrap()
        .insert("r", 1)
        .unwrap();
    e.get_list("list").insert(0, 1).unwrap();
    e.get_list("list").insert(1, 2).unwrap();
    e.commit();
    let mut json = serde_json::to_value(e.export_json_updates(&vv, &e.oplog_vv())).unwrap();
    let last = json["changes"].as_array_mut().unwrap().last_mut().unwrap()["ops"]
        .as_array_mut()
        .unwrap()
        .last_mut()
        .unwrap();
    last["content"]["pos"] = 1000.into();
    let carrier = LoroDoc::new();
    carrier.import(&snap).unwrap();
    carrier.detach();
    carrier
        .import_json_updates(serde_json::to_string(&json).unwrap())
        .unwrap();
    let bad = carrier.export(ExportMode::updates(&vv)).unwrap();
    let good = e.export(ExportMode::updates(&vv)).unwrap();
    (snap, bad, good)
}

/// [`snapshot_bad_and_good_update`] without the update before it was forged.
fn snapshot_and_bad_update() -> (Vec<u8>, Vec<u8>) {
    let (snap, bad, _) = snapshot_bad_and_good_update();
    (snap, bad)
}

/// The repro from loro-dev/loro#1164, verbatim.
#[test]
fn failed_import_leaves_state_at_freed_indices() {
    // Peer 2 creates `c` under `p` and puts a text in its meta; peer 1 deletes `p` concurrently.
    let a = LoroDoc::new();
    a.set_peer_id(1).unwrap();
    let p = a.get_tree("tree").create(TreeParentId::Root).unwrap();
    a.commit();
    let b = LoroDoc::new();
    b.set_peer_id(2).unwrap();
    b.import(&a.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    let c = b.get_tree("tree").create(p).unwrap();
    let meta = b.get_tree("tree").get_meta(c).unwrap();
    meta.insert_container("t", LoroText::new())
        .unwrap()
        .insert(0, "z")
        .unwrap();
    b.commit();
    a.get_tree("tree").delete(p).unwrap();
    a.commit();
    a.import(&b.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    a.get_map("m").insert("k", 1).unwrap();
    a.commit();
    let snap = a.export(ExportMode::Snapshot).unwrap();
    let vv = a.oplog_vv();

    // Peer 4 revives `c` and edits its meta, then a list insert forged out of bounds, which
    // the state rejects.
    let e = LoroDoc::new();
    e.set_peer_id(4).unwrap();
    e.import(&snap).unwrap();
    e.get_tree("tree").mov(c, TreeParentId::Root).unwrap();
    e.get_tree("tree")
        .get_meta(c)
        .unwrap()
        .insert("r", 1)
        .unwrap();
    e.get_list("list").insert(0, 1).unwrap();
    e.get_list("list").insert(1, 2).unwrap();
    e.commit();
    let mut json = serde_json::to_value(e.export_json_updates(&vv, &e.oplog_vv())).unwrap();
    let last = json["changes"].as_array_mut().unwrap().last_mut().unwrap()["ops"]
        .as_array_mut()
        .unwrap()
        .last_mut()
        .unwrap();
    last["content"]["pos"] = 1000.into();
    let carrier = LoroDoc::new();
    carrier.import(&snap).unwrap();
    carrier.detach();
    carrier
        .import_json_updates(serde_json::to_string(&json).unwrap())
        .unwrap();
    let bad = carrier.export(ExportMode::updates(&vv)).unwrap();

    let doc = import(&snap);
    let reference = import(&snap);
    assert!(doc.import(&bad).is_err());
    for d in [&doc, &reference] {
        d.get_map("fresh")
            .insert_container("c0", LoroMap::new())
            .unwrap()
            .insert("v", 1)
            .unwrap();
        d.commit();
    }
    // Fails: `fresh` also shows `"t": "z"` from `c`'s meta.
    assert_eq!(
        doc.get_deep_value().to_json_value(),
        reference.get_deep_value().to_json_value()
    );
}

/// After a failed import, new containers (roots and children) get their own state, the doc
/// round-trips through a snapshot, and every later edit matches a doc that never saw the import.
#[test]
fn containers_created_after_a_failed_import_match_a_clean_doc() {
    let (snap, bad) = snapshot_and_bad_update();
    let doc = import(&snap);
    let reference = import(&snap);
    assert!(doc.import(&bad).is_err());
    assert!(doc.import(&bad).is_err());
    for d in [&doc, &reference] {
        for i in 0..10 {
            let root = d.get_map(format!("fresh{i}").as_str());
            let child = root.insert_container("c", LoroMap::new()).unwrap();
            child.insert("v", i).unwrap();
            child
                .insert_container("t", LoroText::new())
                .unwrap()
                .insert(0, "x")
                .unwrap();
            d.get_list("list").push(i).unwrap();
            d.commit();
            assert!(!child.is_deleted());
            assert!(!root.is_deleted());
        }
    }
    let expected = reference.get_deep_value().to_json_value();
    assert_eq!(doc.get_deep_value().to_json_value(), expected);
    assert_eq!(
        import(&doc.export(ExportMode::Snapshot).unwrap())
            .get_deep_value()
            .to_json_value(),
        expected
    );
    assert_eq!(
        import(&doc.export(ExportMode::all_updates()).unwrap())
            .get_deep_value()
            .to_json_value(),
        expected
    );
    doc.check_state_correctness_slow();
}

/// Retrying the same invalid update is rejected again, and leaves the doc unchanged.
#[test]
fn retrying_a_failed_import_is_rejected_again() {
    let (snap, bad) = snapshot_and_bad_update();
    let doc = import(&snap);
    let reference = import(&snap);
    for i in 0..5 {
        assert!(doc.import(&bad).is_err());
        assert_eq!(
            doc.get_deep_value().to_json_value(),
            reference.get_deep_value().to_json_value()
        );
        assert_eq!(doc.oplog_vv(), reference.oplog_vv());
        // Register a new container between the attempts.
        for d in [&doc, &reference] {
            let _ = d.get_map(format!("between{i}").as_str());
        }
    }
    // Also through `import_batch`.
    assert!(doc.import_batch(&[bad.clone()]).is_err());
    assert!(doc.import(&bad).is_err());
    assert_eq!(
        doc.get_deep_value().to_json_value(),
        reference.get_deep_value().to_json_value()
    );
    doc.check_state_correctness_slow();
}

/// The containers the failed import registered (the revived node's meta, the new list) work
/// when the valid version of the update arrives later.
#[test]
fn valid_update_after_a_failed_import_matches_a_clean_doc() {
    let (snap, bad, good) = snapshot_bad_and_good_update();
    let doc = import(&snap);
    let reference = import(&snap);
    assert!(doc.import(&bad).is_err());
    // Queries between the attempts that touch the containers of the failed import.
    let _ = doc.get_deep_value();
    assert!(doc.get_list("list").is_empty());
    doc.import(&good).unwrap();
    reference.import(&good).unwrap();
    let expected = reference.get_deep_value().to_json_value();
    assert_eq!(doc.get_deep_value().to_json_value(), expected);
    for d in [&doc, &reference] {
        d.get_map("fresh")
            .insert_container("c0", LoroMap::new())
            .unwrap()
            .insert("v", 1)
            .unwrap();
        d.commit();
    }
    assert_eq!(
        doc.get_deep_value().to_json_value(),
        reference.get_deep_value().to_json_value()
    );
    doc.check_state_correctness_slow();
}

/// Another thread registers containers (root handlers, tree metas) while an import fails. Its
/// handlers keep working after the rollback, and containers created afterwards get their own
/// state. Before loro-dev/loro#1164 the rollback freed the indices those handlers held.
///
/// Timing-dependent: each trial overlaps the queries with two failing imports. The number of
/// trials is `FAILED_IMPORT_THREAD_TRIALS` (default 8).
#[test]
fn handlers_created_during_a_failed_import_keep_their_containers() {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Barrier};

    let (snap, _) = snapshot_and_bad_update();
    let base = import(&snap);
    let c = base
        .get_tree("tree")
        .get_nodes(true)
        .iter()
        .find(|n| matches!(n.parent, TreeParentId::Node(_)))
        .map(|n| n.id)
        .unwrap();
    let vv = base.oplog_vv();
    // A bad update that registers many containers before the state rejects it.
    let e = import(&snap);
    e.set_peer_id(4).unwrap();
    e.get_tree("tree").mov(c, TreeParentId::Root).unwrap();
    e.get_tree("tree")
        .get_meta(c)
        .unwrap()
        .insert("r", 1)
        .unwrap();
    for i in 0..300 {
        let child = e
            .get_map("new")
            .insert_container(&format!("n{i}"), LoroMap::new())
            .unwrap();
        child.insert("v", i).unwrap();
        e.commit();
    }
    e.get_list("list").insert(0, 1).unwrap();
    e.get_list("list").insert(1, 2).unwrap();
    e.commit();
    let mut json = serde_json::to_value(e.export_json_updates(&vv, &e.oplog_vv())).unwrap();
    let last = json["changes"].as_array_mut().unwrap().last_mut().unwrap()["ops"]
        .as_array_mut()
        .unwrap()
        .last_mut()
        .unwrap();
    last["content"]["pos"] = 1000.into();
    let carrier = import(&snap);
    carrier.detach();
    carrier
        .import_json_updates(serde_json::to_string(&json).unwrap())
        .unwrap();
    let bad = carrier.export(ExportMode::updates(&vv)).unwrap();

    let trials: usize = std::env::var("FAILED_IMPORT_THREAD_TRIALS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(8);
    for trial in 0..trials {
        let doc = import(&snap);
        let barrier = Arc::new(Barrier::new(2));
        let done = Arc::new(AtomicBool::new(false));
        let querier = {
            let doc = doc.clone();
            let barrier = barrier.clone();
            let done = done.clone();
            std::thread::spawn(move || {
                barrier.wait();
                let mut handlers = Vec::new();
                let mut n = 0;
                while !done.load(Ordering::Acquire) || n < 4 {
                    let meta = doc.get_tree("tree").get_meta(c).unwrap();
                    assert!(meta.is_deleted());
                    handlers.push((n, doc.get_map(format!("fresh{n}").as_str())));
                    n += 1;
                    std::thread::yield_now();
                }
                handlers
            })
        };
        barrier.wait();
        assert!(doc.import(&bad).is_err());
        assert!(doc.import(&bad).is_err());
        done.store(true, Ordering::Release);
        let handlers = querier.join().unwrap();

        let reference = import(&snap);
        for (n, map) in handlers.iter() {
            map.insert("v", *n).unwrap();
            reference
                .get_map(format!("fresh{n}").as_str())
                .insert("v", *n)
                .unwrap();
        }
        for d in [&doc, &reference] {
            for i in 0..10 {
                let child = d
                    .get_map("after")
                    .insert_container(&format!("c{i}"), LoroMap::new())
                    .unwrap();
                child.insert("v", i).unwrap();
                assert!(!child.is_deleted());
            }
            d.commit();
        }
        assert_eq!(
            doc.get_deep_value().to_json_value(),
            reference.get_deep_value().to_json_value(),
            "trial {trial}"
        );
        assert!(doc.get_tree("tree").get_meta(c).unwrap().is_deleted());
        doc.check_state_correctness_slow();
    }
}

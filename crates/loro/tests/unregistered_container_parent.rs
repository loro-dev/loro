//! A document loaded from a snapshot parses its change blocks lazily, so the
//! parent of a container can be unknown until the change that created it is
//! loaded. The metadata of a tree node that was created inside an already
//! deleted subtree is never materialized, so it has no state entry either:
//! querying or editing it panicked with "Parent is not registered" and aborted
//! the process (loro-dev/loro#1158). See `context/arena-parent-links.md`.
//!
//! Every test compares against a document that imported the same history as
//! updates, where every change is parsed on import.

use loro::{
    ContainerID, ContainerTrait, ExportMode, LoroDoc, LoroError, ToJson, TreeID, TreeParentId,
    UndoManager, ID,
};
use std::sync::{Arc, Mutex};

/// Peer 2 creates `child` under `parent` while peer 1 deletes `parent`;
/// `filler` later commits push the child's `Create` into an older block.
fn concurrent_child_of_deleted_parent(filler: usize) -> (LoroDoc, TreeID) {
    let a = LoroDoc::new();
    a.set_peer_id(1).unwrap();
    let tree = a.get_tree("tree");
    let parent = tree.create(TreeParentId::Root).unwrap();
    a.commit();
    let b = LoroDoc::new();
    b.set_peer_id(2).unwrap();
    b.import(&a.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    let child = b.get_tree("tree").create(parent).unwrap();
    b.commit();
    tree.delete(parent).unwrap();
    a.commit();
    a.import(&b.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    for i in 0..filler {
        a.get_map("m")
            .insert(&format!("k{}", i % 50), i as i64)
            .unwrap();
        a.commit();
    }
    (a, child)
}

fn import(bytes: &[u8]) -> LoroDoc {
    let doc = LoroDoc::new();
    doc.import(bytes).unwrap();
    doc.set_peer_id(9).unwrap();
    doc
}

/// The ways to load `src`: all updates (the reference), a snapshot, a fork,
/// and a shallow snapshot whose root is before `child`'s creation.
fn loaded(src: &LoroDoc) -> Vec<(&'static str, LoroDoc)> {
    let first_change = LoroDoc::new();
    first_change
        .import(
            &src.export(ExportMode::updates_in_range(vec![loro::IdSpan::new(
                1, 0, 1,
            )]))
            .unwrap(),
        )
        .unwrap();
    let fork = src.fork();
    fork.set_peer_id(9).unwrap();
    vec![
        (
            "updates",
            import(&src.export(ExportMode::all_updates()).unwrap()),
        ),
        (
            "snapshot",
            import(&src.export(ExportMode::Snapshot).unwrap()),
        ),
        ("fork", fork),
        (
            "shallow",
            import(
                &src.export(ExportMode::shallow_snapshot(
                    &first_change.oplog_frontiers(),
                ))
                .unwrap(),
            ),
        ),
    ]
}

#[test]
fn deleted_meta_queries_and_edits_after_loading() {
    let (src, child) = concurrent_child_of_deleted_parent(5000);
    let meta_id = child.associated_meta_container();
    for (how, doc) in loaded(&src) {
        let tree = doc.get_tree("tree");
        assert!(tree.is_node_deleted(&child).unwrap(), "{how}");
        let meta = tree.get_meta(child).unwrap();
        assert!(meta.is_deleted(), "{how}");
        assert!(
            matches!(meta.insert("k", 1), Err(LoroError::ContainerDeleted { .. })),
            "{how}"
        );
        assert_eq!(doc.get_path_to_container(&meta_id), None, "{how}");
        assert!(doc.has_container(&meta_id), "{how}");
        assert!(
            doc.get_container(meta_id.clone()).unwrap().is_deleted(),
            "{how}"
        );
    }
}

#[test]
fn container_by_id_in_a_fresh_snapshot_doc() {
    // No handler registered the meta before the lookup.
    let (src, child) = concurrent_child_of_deleted_parent(5000);
    let meta_id = child.associated_meta_container();
    for (how, doc) in loaded(&src) {
        let meta = doc.get_container(meta_id.clone()).unwrap();
        assert!(meta.is_deleted(), "{how}");
        assert_eq!(doc.get_path_to_container(&meta_id), None, "{how}");
    }
}

#[test]
fn reviving_the_node_after_loading() {
    let (src, child) = concurrent_child_of_deleted_parent(5000);
    let meta_id = child.associated_meta_container();
    for (how, doc) in loaded(&src) {
        let tree = doc.get_tree("tree");
        let meta = tree.get_meta(child).unwrap();
        assert!(meta.is_deleted(), "{how}");
        tree.mov(child, TreeParentId::Root).unwrap();
        doc.commit();
        assert!(!meta.is_deleted(), "{how}");
        meta.insert("k", 1).unwrap();
        doc.commit();
        assert_eq!(
            doc.get_path_to_container(&meta_id)
                .unwrap()
                .last()
                .unwrap()
                .0,
            meta_id,
            "{how}"
        );
    }
}

/// An update from another peer that revives `child` and sets `x` in its meta.
fn revival_and_meta_edit_by_another_peer(src: &LoroDoc, child: TreeID) -> Vec<u8> {
    let editor = LoroDoc::new();
    editor.set_peer_id(4).unwrap();
    editor
        .import(&src.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    let before = editor.oplog_vv();
    editor
        .get_tree("tree")
        .mov(child, TreeParentId::Root)
        .unwrap();
    editor
        .get_tree("tree")
        .get_meta(child)
        .unwrap()
        .insert("x", 1)
        .unwrap();
    editor.commit();
    editor.export(ExportMode::updates(&before)).unwrap()
}

#[test]
fn importing_an_edit_of_the_meta_with_a_subscriber() {
    // Another peer revives the node and edits its meta. The loaded doc emits
    // events for the meta, which walks its ancestors and its path.
    let (src, child) = concurrent_child_of_deleted_parent(5000);
    let update = revival_and_meta_edit_by_another_peer(&src, child);

    let mut expected = None;
    for (how, doc) in loaded(&src) {
        let tree_id = doc.get_tree("tree").id();
        let events = Arc::new(Mutex::new(vec![]));
        let e2 = events.clone();
        let _root = doc.subscribe_root(Arc::new(move |e| {
            for c in e.events.iter() {
                e2.lock()
                    .unwrap()
                    .push(format!("{:?} {:?}", c.target, c.path));
            }
        }));
        let tree_events = Arc::new(Mutex::new(0));
        let t2 = tree_events.clone();
        // A subscriber on the tree is notified through the meta's ancestors.
        let _tree = doc.subscribe(
            &tree_id,
            Arc::new(move |e| {
                *t2.lock().unwrap() += e.events.len();
            }),
        );
        doc.import(&update).unwrap();
        let meta = doc.get_tree("tree").get_meta(child).unwrap();
        assert_eq!(
            meta.get_value().to_json_value(),
            serde_json::json!({"x": 1}),
            "{how}"
        );
        assert!(*tree_events.lock().unwrap() > 0, "{how}");
        let events = events.lock().unwrap().clone();
        match &expected {
            None => expected = Some(events),
            Some(expected) => assert_eq!(&events, expected, "{how}"),
        }
    }
}

/// Undo and revert after the loaded doc revives the node itself. The local
/// move registers the meta's parent, so this also passes on `main`; it checks
/// that the result matches the reference.
#[test]
fn undo_and_revert_touching_the_meta() {
    let (src, child) = concurrent_child_of_deleted_parent(5000);
    let mut expected = None;
    for (how, doc) in loaded(&src) {
        let mut undo = UndoManager::new(&doc);
        let tree = doc.get_tree("tree");
        let before_revival = doc.oplog_frontiers();
        tree.mov(child, TreeParentId::Root).unwrap();
        doc.commit();
        tree.get_meta(child).unwrap().insert("k", 1).unwrap();
        doc.commit();
        assert!(undo.undo().unwrap(), "{how}");
        assert!(undo.undo().unwrap(), "{how}");
        assert!(tree.get_meta(child).unwrap().is_deleted(), "{how}");
        assert!(undo.redo().unwrap(), "{how}");
        doc.revert_to(&before_revival).unwrap();
        doc.commit();
        let value = doc.get_deep_value().to_json_value();
        match &expected {
            None => expected = Some(value),
            Some(expected) => assert_eq!(&value, expected, "{how}"),
        }
    }
}

/// Undoing a local edit transforms it against the changes imported since,
/// here another peer's revival and edit of the meta.
#[test]
fn undo_after_importing_an_edit_of_the_meta() {
    let (src, child) = concurrent_child_of_deleted_parent(5000);
    let update = revival_and_meta_edit_by_another_peer(&src, child);
    let mut expected = None;
    for (how, doc) in loaded(&src) {
        let mut undo = UndoManager::new(&doc);
        doc.get_map("m").insert("local", 1).unwrap();
        doc.commit();
        doc.import(&update).unwrap();
        assert!(undo.undo().unwrap(), "{how}");
        assert!(undo.redo().unwrap(), "{how}");
        assert!(undo.undo().unwrap(), "{how}");
        let meta = doc.get_tree("tree").get_meta(child).unwrap();
        assert_eq!(
            meta.get_value().to_json_value(),
            serde_json::json!({"x": 1}),
            "{how}"
        );
        assert!(doc.get_map("m").get("local").is_none(), "{how}");
        let value = doc.get_deep_value().to_json_value();
        match &expected {
            None => expected = Some(value),
            Some(expected) => assert_eq!(&value, expected, "{how}"),
        }
    }
}

#[test]
fn ids_that_no_op_creates_are_not_containers() {
    let (src, _) = concurrent_child_of_deleted_parent(5000);
    // 10@1 is a map insert, and 900000@1 is not in the history.
    for id in [ID::new(1, 10), ID::new(1, 900_000)] {
        let cid = ContainerID::new_normal(id, loro::ContainerType::Map);
        for (how, doc) in loaded(&src) {
            assert!(!doc.has_container(&cid), "{how} {id}");
            assert!(doc.get_container(cid.clone()).is_none(), "{how} {id}");
            assert_eq!(doc.get_path_to_container(&cid), None, "{how} {id}");
        }
    }
}

/// The state found by `loro.js`'s review harness (seed 5, step 103): node
/// `7@40881` sits under the deleted `0@2`, and its meta has no state entry.
#[test]
fn review_harness_dump() {
    let snapshot = include_bytes!("./unregistered_meta_parent.snapshot.bin");
    let updates = include_bytes!("./unregistered_meta_parent.updates.bin");
    let node = TreeID::new(40881, 7);
    let from_snapshot = import(snapshot);
    for (how, doc) in [
        ("updates", import(updates)),
        ("fork", from_snapshot.fork()),
        ("snapshot", from_snapshot),
    ] {
        let tree = doc.get_tree("tree");
        assert!(tree.is_node_deleted(&node).unwrap(), "{how}");
        let meta = tree.get_meta(node).unwrap();
        assert!(meta.is_deleted(), "{how}");
        assert!(
            matches!(meta.delete("c0"), Err(LoroError::ContainerDeleted { .. })),
            "{how}"
        );
        assert!(
            doc.get_container(node.associated_meta_container())
                .unwrap()
                .is_deleted(),
            "{how}"
        );
    }
}

/// A failed import may load old changes while computing its diff, e.g. to find the parent of
/// a container that only the history knows. Loading a change registers the containers its
/// ops use, and the rollback of the failed import drops registrations made during it, so the
/// loaded change must not keep indices into them.
#[test]
fn failed_import_that_loads_an_old_change() {
    let a = LoroDoc::new();
    a.set_peer_id(1).unwrap();
    let tree = a.get_tree("tree");
    let parent = tree.create(TreeParentId::Root).unwrap();
    a.commit();
    let b = LoroDoc::new();
    b.set_peer_id(2).unwrap();
    b.import(&a.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    let child = b.get_tree("tree").create(parent).unwrap();
    // A container only this change mentions: it has no state entry and is only
    // registered once the change is loaded.
    b.get_map("b")
        .insert_container("inner", loro::LoroText::new())
        .unwrap()
        .insert(0, "inner")
        .unwrap();
    b.get_map("b").delete("inner").unwrap();
    b.commit();
    let b_updates = b.export(ExportMode::all_updates()).unwrap();
    let b_vv = b.oplog_vv();
    tree.delete(parent).unwrap();
    a.commit();
    a.import(&b_updates).unwrap();
    for i in 0..5000 {
        a.get_map("m")
            .insert(&format!("k{}", i % 50), i as i64)
            .unwrap();
        a.commit();
    }
    let snapshot = a.export(ExportMode::Snapshot).unwrap();

    // Peer 4 (who has not seen the delete) edits `child`'s meta, then inserts a list
    // element far out of bounds, which the state rejects.
    let e = LoroDoc::new();
    e.set_peer_id(4).unwrap();
    e.import(&b_updates).unwrap();
    e.get_tree("tree")
        .get_meta(child)
        .unwrap()
        .insert("x", 1)
        .unwrap();
    let list = e.get_list("list");
    list.insert(0, "seed").unwrap();
    list.insert(1, "tail").unwrap();
    e.commit();
    let mut json = serde_json::to_value(e.export_json_updates(&b_vv, &e.oplog_vv())).unwrap();
    let last_op = json["changes"].as_array_mut().unwrap().last_mut().unwrap()["ops"]
        .as_array_mut()
        .unwrap()
        .last_mut()
        .unwrap();
    assert_eq!(last_op["content"]["type"], "insert");
    last_op["content"]["pos"] = 1000.into();
    let carrier = LoroDoc::new();
    carrier.import(&b_updates).unwrap();
    carrier.detach();
    carrier
        .import_json_updates(serde_json::to_string(&json).unwrap())
        .unwrap();
    let bad = carrier.export(ExportMode::updates(&b_vv)).unwrap();

    let dst = import(&snapshot);
    dst.import(&bad)
        .expect_err("the out-of-bounds list insert must fail the import");

    // New registrations may reuse the indices the rollback dropped.
    for i in 0..8 {
        dst.get_map("fresh")
            .insert_container(&format!("c{i}"), loro::LoroMap::new())
            .unwrap();
    }
    dst.commit();
    // The history, including the change the failed import loaded, must still name the
    // right containers.
    let history = |doc: &LoroDoc| {
        serde_json::to_string(&doc.export_json_updates(&Default::default(), &a.oplog_vv())).unwrap()
    };
    assert_eq!(history(&dst), history(&a));
    assert!(dst.get_tree("tree").get_meta(child).unwrap().is_deleted());
}

/// loro-dev/loro#1161: parsing a change also allocates its ops' values in the arena, and
/// the rollback of a failed import truncates them. A change the failed import loaded kept
/// value slices past the end, and a later checkout that read them panicked.
#[test]
fn failed_import_that_loads_old_movable_list_values() {
    let a = LoroDoc::new();
    a.set_peer_id(1).unwrap();
    let list = a.get_movable_list("ml");
    for i in 0..20 {
        list.insert(i, format!("v{i}")).unwrap();
        a.commit();
    }
    let inserted = a.oplog_frontiers();
    for i in 0..20 {
        list.set(i, format!("s{i}")).unwrap();
        a.commit();
    }
    for i in 0..3000 {
        a.get_map("m").insert("k", i).unwrap();
        a.commit();
    }
    let snapshot = a.export(ExportMode::Snapshot).unwrap();
    let vv = a.oplog_vv();

    for edit in ["set", "move"] {
        // Peer 4 edits the movable list, whose history the import's diff loads, then
        // inserts a list element far out of bounds, which the state rejects.
        let e = import(&snapshot);
        e.set_peer_id(4).unwrap();
        match edit {
            "set" => e.get_movable_list("ml").set(3, "x").unwrap(),
            _ => e.get_movable_list("ml").mov(2, 7).unwrap(),
        }
        e.get_list("list").insert(0, 1).unwrap();
        e.get_list("list").insert(1, 2).unwrap();
        e.commit();
        let mut json = serde_json::to_value(e.export_json_updates(&vv, &e.oplog_vv())).unwrap();
        let last_op = json["changes"].as_array_mut().unwrap().last_mut().unwrap()["ops"]
            .as_array_mut()
            .unwrap()
            .last_mut()
            .unwrap();
        last_op["content"]["pos"] = 1000.into();
        let carrier = import(&snapshot);
        carrier.detach();
        carrier
            .import_json_updates(serde_json::to_string(&json).unwrap())
            .unwrap();
        let bad = carrier.export(ExportMode::updates(&vv)).unwrap();

        let dst = import(&snapshot);
        let reference = import(&snapshot);
        for doc in [&dst, &reference] {
            // Registered before the import, so only the values are rolled back.
            doc.get_movable_list("ml");
            doc.get_map("m");
        }
        dst.import(&bad)
            .expect_err("the out-of-bounds list insert must fail the import");
        for doc in [&dst, &reference] {
            doc.checkout(&inserted).unwrap();
        }
        let value = |doc: &LoroDoc| doc.get_deep_value().to_json_value();
        assert_eq!(value(&dst), value(&reference), "{edit}");
        for doc in [&dst, &reference] {
            doc.checkout_to_latest();
        }
        assert_eq!(value(&dst), value(&reference), "{edit}");
    }
}

/// Querying these metas resolves their parents through the change store without the op
/// log lock, while other threads read the store under it. A lock-order inversion inside
/// the store deadlocked (about every other run of 3000 trials hit it);
/// `multi_thread_test.rs` has the loom model. Trials: `UNREGISTERED_PARENT_THREAD_TRIALS`
/// (default 500).
#[test]
fn queries_race_with_history_readers() {
    use std::sync::{mpsc, Barrier};
    use std::time::Duration;

    // Peer 2 creates 32 children, spread over change blocks, under parents that peer 1
    // deletes concurrently.
    let a = LoroDoc::new();
    a.set_peer_id(1).unwrap();
    let tree = a.get_tree("tree");
    let parents: Vec<_> = (0..4)
        .map(|_| tree.create(TreeParentId::Root).unwrap())
        .collect();
    a.commit();
    let b = LoroDoc::new();
    b.set_peer_id(2).unwrap();
    b.import(&a.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    let mut children = vec![];
    for p in parents.iter() {
        for _ in 0..8 {
            children.push(b.get_tree("tree").create(*p).unwrap());
            b.commit();
            for k in 0..30 {
                b.get_map("filler").insert("k", k).unwrap();
                b.commit();
            }
        }
    }
    for p in parents.iter() {
        tree.delete(*p).unwrap();
    }
    a.commit();
    a.import(&b.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    for i in 0..1000 {
        a.get_map("m").insert("k", i).unwrap();
        a.commit();
    }
    let snapshot = a.export(ExportMode::Snapshot).unwrap();

    let trials: usize = std::env::var("UNREGISTERED_PARENT_THREAD_TRIALS")
        .map(|s| s.parse().unwrap())
        .unwrap_or(500);
    let (progress, rx) = mpsc::channel();
    let runner = std::thread::spawn(move || {
        for _ in 0..trials {
            let doc = import(&snapshot);
            let barrier = Arc::new(Barrier::new(3));
            let spawn = |f: Box<dyn FnOnce(&LoroDoc) + Send>| {
                let (doc, barrier) = (doc.clone(), barrier.clone());
                std::thread::spawn(move || {
                    barrier.wait();
                    f(&doc)
                })
            };
            let metas = children.clone();
            let ids = children.clone();
            let threads = [
                spawn(Box::new(move |doc| {
                    let tree = doc.get_tree("tree");
                    for c in metas {
                        assert!(tree.get_meta(c).unwrap().is_deleted());
                    }
                })),
                spawn(Box::new(move |doc| {
                    for c in ids {
                        let id = c.associated_meta_container();
                        assert!(doc.has_container(&id));
                        assert_eq!(doc.get_path_to_container(&id), None);
                    }
                })),
                spawn(Box::new(|doc| {
                    for _ in 0..3 {
                        assert!(doc.len_changes() > 0);
                        doc.export(ExportMode::all_updates()).unwrap();
                        doc.frontiers_to_vv(&doc.oplog_frontiers()).unwrap();
                    }
                })),
            ];
            for t in threads {
                t.join().unwrap();
            }
            progress.send(()).unwrap();
        }
    });
    loop {
        match rx.recv_timeout(Duration::from_secs(30)) {
            Ok(()) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => panic!("no progress for 30 s: deadlock"),
        }
    }
    runner.join().unwrap();
}

/// An import that fails before its rollback scope begins (decoding, or depending on history
/// before a shallow root) rolls the arena back to where it started, while another thread
/// resolves parents through the change store. Neither the parsed blocks nor the resolver's
/// caller may keep indices the rollback frees (`multi_thread_test.rs` has the loom model).
/// Before the fix, a failed JSON import hit it within the default trials in most runs.
/// Trials: `UNREGISTERED_PARENT_THREAD_TRIALS` (default 120).
#[test]
fn queries_race_with_failing_imports() {
    use loro::{LoroMap, VersionVector};
    use std::sync::Barrier;
    use std::time::Instant;

    // Peer 2 creates children under parents that peer 1 deletes concurrently, and map
    // children that it deletes again: none of them is in the state, except that a shallow
    // snapshot keeps the metas of deleted nodes.
    let a = LoroDoc::new();
    a.set_peer_id(1).unwrap();
    let tree = a.get_tree("tree");
    let parents: Vec<_> = (0..4)
        .map(|_| tree.create(TreeParentId::Root).unwrap())
        .collect();
    a.commit();
    let before_root = a.export(ExportMode::all_updates()).unwrap();
    a.get_map("m").insert("k", -1).unwrap();
    a.commit();
    let root = a.oplog_frontiers();
    let root_vv = a.oplog_vv();
    let shallow_at_root = a.export(ExportMode::shallow_snapshot(&root)).unwrap();
    let b = LoroDoc::new();
    b.set_peer_id(2).unwrap();
    b.import(&a.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    let mut ids = vec![];
    for (i, p) in parents.iter().enumerate() {
        for j in 0..8 {
            let child = b.get_tree("tree").create(*p).unwrap();
            ids.push(child.associated_meta_container());
            let key = format!("c{i}_{j}");
            let gone = b
                .get_map("gone")
                .insert_container(&key, LoroMap::new())
                .unwrap();
            gone.insert("v", 1).unwrap();
            ids.push(gone.id());
            b.get_map("gone").delete(&key).unwrap();
            b.commit();
            for k in 0..30 {
                b.get_map("filler").insert("k", k).unwrap();
                b.commit();
            }
        }
    }
    for p in parents.iter() {
        tree.delete(*p).unwrap();
    }
    a.commit();
    a.import(&b.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    for i in 0..1000 {
        a.get_map("m").insert("k", i).unwrap();
        a.commit();
    }
    let vv = a.oplog_vv();
    let snapshot = a.export(ExportMode::Snapshot).unwrap();
    // A snapshot of a shallow document whose history starts at `root`. It received the
    // rest as updates, so its state lacks the same containers, and it loads lazily.
    let shallow_doc = import(&shallow_at_root);
    shallow_doc
        .import(&a.export(ExportMode::updates(&root_vv)).unwrap())
        .unwrap();
    let shallow = shallow_doc.export(ExportMode::Snapshot).unwrap();

    // Many changes that create containers, so the failing import registers many and takes a
    // while. Decoding the JSON fails at the empty change at its end.
    let e = import(&snapshot);
    e.set_peer_id(8).unwrap();
    for i in 0..1000 {
        e.get_map("new")
            .insert_container(&format!("n{i}"), LoroMap::new())
            .unwrap();
        e.commit();
    }
    let mut json = serde_json::to_value(e.export_json_updates(&vv, &e.oplog_vv())).unwrap();
    let changes = json["changes"].as_array_mut().unwrap();
    let mut empty = changes.last().unwrap().clone();
    empty["ops"] = serde_json::json!([]);
    changes.push(empty);
    let bad_json = serde_json::to_string(&json).unwrap();
    // These depend on history before the shallow root.
    let o = LoroDoc::new();
    o.set_peer_id(10).unwrap();
    o.import(&before_root).unwrap();
    for i in 0..1000 {
        o.get_map("old")
            .insert_container(&format!("n{i}"), LoroMap::new())
            .unwrap();
        o.commit();
    }
    let old_deps = o.export(ExportMode::all_updates()).unwrap();

    let history = |doc: &LoroDoc, from: &VersionVector| {
        serde_json::to_string(&doc.export_json_updates(from, &vv)).unwrap()
    };
    let reference = import(&a.export(ExportMode::all_updates()).unwrap());
    let empty_vv = VersionVector::new();
    let expected = [
        history(&reference, &empty_vv),
        history(&reference, &root_vv),
    ];
    // Also IDs of peer 2's other ops, which create no container. In the shallow document the
    // containers above are in the state, and only these make the resolver load a block.
    for counter in (0..vv.get(&2).copied().unwrap()).step_by(13) {
        ids.push(ContainerID::new_normal(
            ID::new(2, counter),
            loro::ContainerType::Map,
        ));
    }
    let exists: Vec<bool> = ids.iter().map(|id| reference.has_container(id)).collect();

    let fail_import = |doc: &LoroDoc, is_shallow: bool| {
        if is_shallow {
            assert!(matches!(
                doc.import(&old_deps),
                Err(LoroError::ImportUpdatesThatDependsOnOutdatedVersion)
            ));
        } else {
            assert!(doc.import_json_updates(bad_json.as_str()).is_err());
        }
    };
    // How long each failing import takes here, to sweep the queries across it.
    let window = [false, true].map(|is_shallow| {
        let doc = import(if is_shallow { &shallow } else { &snapshot });
        let start = Instant::now();
        fail_import(&doc, is_shallow);
        start.elapsed()
    });

    let trials: usize = std::env::var("UNREGISTERED_PARENT_THREAD_TRIALS")
        .map(|s| s.parse().unwrap())
        .unwrap_or(120);
    for trial in 0..trials {
        let is_shallow = trial % 2 == 1;
        let doc = import(if is_shallow { &shallow } else { &snapshot });
        let barrier = Arc::new(Barrier::new(2));
        let query = {
            let (doc, barrier, ids) = (doc.clone(), barrier.clone(), ids.clone());
            let delay = window[is_shallow as usize] * (trial as u32 / 2 % 20) / 20;
            std::thread::spawn(move || {
                barrier.wait();
                let start = Instant::now();
                while start.elapsed() < delay {
                    std::hint::spin_loop();
                }
                for id in ids.iter() {
                    doc.has_container(id);
                }
            })
        };
        std::thread::scope(|scope| {
            scope.spawn(|| {
                barrier.wait();
                fail_import(&doc, is_shallow);
            });
        });
        query.join().unwrap();
        for (id, exists) in ids.iter().zip(exists.iter()) {
            assert_eq!(doc.has_container(id), *exists, "trial {trial}: {id}");
        }
        let from = if is_shallow { &root_vv } else { &empty_vv };
        assert_eq!(
            history(&doc, from),
            expected[is_shallow as usize],
            "trial {trial}"
        );
    }
}

mod random {
    use super::*;
    use loro::{LoroText, TreeID};
    use rand::{rngs::StdRng, seq::SliceRandom, Rng, SeedableRng};

    fn random_history(seed: u64) -> LoroDoc {
        let mut rng = StdRng::seed_from_u64(seed);
        let peers: Vec<LoroDoc> = (1..=3)
            .map(|p| {
                let d = LoroDoc::new();
                d.set_peer_id(p).unwrap();
                d
            })
            .collect();
        for _ in 0..60 {
            let i = rng.gen_range(0..peers.len());
            let doc = &peers[i];
            let tree = doc.get_tree("tree");
            let nodes = tree.nodes();
            let pick = |rng: &mut StdRng| nodes.choose(rng).copied();
            let _ = match rng.gen_range(0..10) {
                0..=2 => {
                    let parent = match pick(&mut rng) {
                        Some(n) if rng.gen_bool(0.7) => TreeParentId::Node(n),
                        _ => TreeParentId::Root,
                    };
                    tree.create(parent).map(|_| ())
                }
                3 => match (pick(&mut rng), pick(&mut rng)) {
                    (Some(t), Some(p)) => tree.mov(t, p),
                    _ => Ok(()),
                },
                4 => match pick(&mut rng) {
                    Some(t) => tree.delete(t),
                    None => Ok(()),
                },
                5 => match pick(&mut rng) {
                    Some(t) => tree
                        .get_meta(t)
                        .and_then(|m| m.insert("k", rng.gen::<i32>())),
                    None => Ok(()),
                },
                6 => match pick(&mut rng) {
                    Some(t) => tree
                        .get_meta(t)
                        .and_then(|m| m.insert_container("text", LoroText::new())?.insert(0, "t")),
                    None => Ok(()),
                },
                7 => {
                    // Later history, so earlier changes end up in older blocks.
                    for k in 0..rng.gen_range(50..300) {
                        doc.get_map("filler")
                            .insert(&format!("k{}", k % 20), k)
                            .unwrap();
                        doc.commit();
                    }
                    Ok(())
                }
                _ => {
                    let j = rng.gen_range(0..peers.len());
                    if i != j {
                        let u = peers[j]
                            .export(ExportMode::updates(&doc.oplog_vv()))
                            .unwrap();
                        doc.import(&u).unwrap();
                    }
                    Ok(())
                }
            };
            doc.commit();
        }
        let all = LoroDoc::new();
        for p in peers.iter() {
            all.import(&p.export(ExportMode::all_updates()).unwrap())
                .unwrap();
        }
        all
    }

    /// What a document says about the metadata of `node`.
    fn observe(doc: &LoroDoc, node: TreeID) -> String {
        let tree = doc.get_tree("tree");
        let meta_id = node.associated_meta_container();
        let meta = tree.get_meta(node).unwrap();
        format!(
            "node_deleted={:?} meta_deleted={} has={} by_id={:?} path={:?} value={:?}",
            tree.is_node_deleted(&node),
            meta.is_deleted(),
            doc.has_container(&meta_id),
            doc.get_container(meta_id.clone()).map(|c| c.is_deleted()),
            doc.get_path_to_container(&meta_id),
            meta.get_deep_value(),
        )
    }

    fn check(seed: u64) {
        let src = random_history(seed);
        let mut rng = StdRng::seed_from_u64(seed ^ 0x5eed);
        let reference = import(&src.export(ExportMode::all_updates()).unwrap());
        let nodes = reference.get_tree("tree").nodes();
        if nodes.is_empty() {
            return;
        }
        let vv = src.oplog_vv();
        let peers: Vec<_> = vv.iter().filter(|(_, n)| **n > 0).collect();
        let (peer, end) = peers.choose(&mut rng).unwrap();
        let root = src.vv_to_frontiers(
            &src.frontiers_to_vv(&ID::new(**peer, rng.gen_range(0..**end)).into())
                .unwrap(),
        );
        let fork = src.fork();
        fork.set_peer_id(9).unwrap();
        let loaded = [
            (
                "snapshot",
                import(&src.export(ExportMode::Snapshot).unwrap()),
            ),
            ("fork", fork),
            (
                "shallow",
                import(&src.export(ExportMode::shallow_snapshot(&root)).unwrap()),
            ),
            (
                "state_only",
                import(&src.export(ExportMode::state_only(None)).unwrap()),
            ),
        ];
        // Visit the nodes in a random order: resolving one meta must not change the
        // answer for another.
        let mut order = nodes.clone();
        order.shuffle(&mut rng);
        for (how, doc) in loaded.iter() {
            for &node in order.iter() {
                // Every source has the latest version, where every node exists.
                assert!(
                    doc.get_tree("tree").contains(node),
                    "seed {seed} {how} {node}"
                );
                assert_eq!(
                    observe(doc, node),
                    observe(&reference, node),
                    "seed {seed} {how} node {node}"
                );
            }
        }

        // Edit every meta, then revive some nodes and edit again.
        let mut revive = nodes.clone();
        revive.shuffle(&mut rng);
        revive.truncate(3);
        for doc in std::iter::once(&reference).chain(loaded.iter().map(|(_, d)| d)) {
            for &node in order.iter() {
                let r = doc
                    .get_tree("tree")
                    .get_meta(node)
                    .unwrap()
                    .insert("probe", 1);
                assert!(
                    r.is_ok() || matches!(r, Err(LoroError::ContainerDeleted { .. })),
                    "seed {seed}: {r:?}"
                );
            }
            doc.commit();
            for &node in revive.iter() {
                let _ = doc.get_tree("tree").mov(node, TreeParentId::Root);
            }
            doc.commit();
            for &node in order.iter() {
                let _ = doc
                    .get_tree("tree")
                    .get_meta(node)
                    .unwrap()
                    .insert("after", 2);
            }
            doc.commit();
        }
        for (how, doc) in loaded.iter() {
            for &node in order.iter() {
                assert_eq!(
                    observe(doc, node),
                    observe(&reference, node),
                    "seed {seed} {how} node {node} after edits"
                );
            }
        }
    }

    #[test]
    fn metas_after_loading_match_a_full_history_import() {
        let seeds: Vec<u64> = match std::env::var("UNREGISTERED_PARENT_SEEDS") {
            Ok(r) => {
                let (a, b) = r.split_once("..").unwrap();
                (a.parse().unwrap()..b.parse().unwrap()).collect()
            }
            Err(_) => (0..20).collect(),
        };
        for seed in seeds {
            check(seed);
        }
    }
}

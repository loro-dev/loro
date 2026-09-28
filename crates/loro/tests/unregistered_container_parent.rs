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

#[test]
fn importing_an_edit_of_the_meta_with_a_subscriber() {
    // Another peer revives the node and edits its meta. The loaded doc emits
    // events for the meta, which walks its ancestors and its path.
    let (src, child) = concurrent_child_of_deleted_parent(5000);
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
    let update = editor.export(ExportMode::updates(&before)).unwrap();

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

#[test]
fn undo_and_revert_touching_the_meta() {
    let (src, child) = concurrent_child_of_deleted_parent(5000);
    let mut expected = None;
    for (how, doc) in loaded(&src) {
        let mut undo = UndoManager::new(&doc);
        let tree = doc.get_tree("tree");
        let alive_before = doc.oplog_frontiers();
        tree.mov(child, TreeParentId::Root).unwrap();
        doc.commit();
        tree.get_meta(child).unwrap().insert("k", 1).unwrap();
        doc.commit();
        assert!(undo.undo().unwrap(), "{how}");
        assert!(undo.undo().unwrap(), "{how}");
        assert!(tree.get_meta(child).unwrap().is_deleted(), "{how}");
        assert!(undo.redo().unwrap(), "{how}");
        doc.revert_to(&alive_before).unwrap();
        doc.commit();
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

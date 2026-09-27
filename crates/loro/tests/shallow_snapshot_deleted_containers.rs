//! Containers deleted before a shallow root must not be carried in the exported
//! state, however many ops the snapshot retains after the root — unless a
//! retained op can bring them back. Tree nodes are the case that can: a node
//! that is only dead because an ancestor was deleted can be moved out locally,
//! and any peer's `Move` op revives even a directly deleted node (the local
//! handler refuses that, but the tree CRDT applies it). Either way the node's
//! old meta map is live again, so it must stay in the shallow root state.

use loro::{
    ContainerID, ContainerTrait, ExportMode, Frontiers, JsonSchema, LoroDoc, LoroMap, LoroText,
    TreeID, TreeParentId,
};

/// `rows` child maps, all deleted, then the cut, then `edits_after_cut` unrelated ops.
fn doc_with_rows_deleted_before_cut(
    rows: usize,
    edits_after_cut: usize,
) -> (LoroDoc, Frontiers, Vec<ContainerID>) {
    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    let map = doc.get_map("rows");
    let mut ids = Vec::new();
    for i in 0..rows {
        let row = map
            .insert_container(&format!("r{i}"), LoroMap::new())
            .unwrap();
        row.insert("title", format!("row {i}")).unwrap();
        ids.push(row.id());
    }
    doc.commit();
    for i in 0..rows {
        map.delete(&format!("r{i}")).unwrap();
    }
    doc.commit();
    let cut = doc.oplog_frontiers();
    let other = doc.get_map("other");
    for i in 0..edits_after_cut {
        other.insert(&format!("k{i}"), i as i64).unwrap();
        doc.commit();
    }
    (doc, cut, ids)
}

#[test]
fn shallow_snapshot_drops_containers_deleted_before_the_root() {
    // 300 retained ops is above the threshold that adds the latest-state overlay.
    for edits_after_cut in [0, 300] {
        let (doc, cut, ids) = doc_with_rows_deleted_before_cut(50, edits_after_cut);
        let bytes = doc.export(ExportMode::shallow_snapshot(&cut)).unwrap();
        let loaded = LoroDoc::new();
        loaded.import(&bytes).unwrap();
        assert_eq!(loaded.get_deep_value(), doc.get_deep_value());
        for id in &ids {
            assert!(
                !loaded.has_container(id),
                "{edits_after_cut} ops after the root: {id} was deleted before the root but is still stored"
            );
        }
    }
}

#[test]
fn state_only_snapshot_drops_containers_deleted_before_the_target() {
    let (doc, _, ids) = doc_with_rows_deleted_before_cut(50, 300);
    let bytes = doc
        .export(ExportMode::state_only(Some(&doc.oplog_frontiers())))
        .unwrap();
    let loaded = LoroDoc::new();
    loaded.import(&bytes).unwrap();
    assert_eq!(loaded.get_deep_value(), doc.get_deep_value());
    for id in &ids {
        assert!(
            !loaded.has_container(id),
            "{id} was deleted before the target but is still stored"
        );
    }
}

#[test]
fn shallow_snapshot_keeps_containers_created_after_the_root() {
    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    doc.get_map("seed").insert("a", 1).unwrap();
    doc.commit();
    let cut = doc.oplog_frontiers();
    let rows = doc.get_map("rows");
    let mut ids = Vec::new();
    for i in 0..300 {
        let row = rows
            .insert_container(&format!("r{i}"), LoroMap::new())
            .unwrap();
        row.insert("title", format!("row {i}")).unwrap();
        ids.push(row.id());
        doc.commit();
    }
    let bytes = doc.export(ExportMode::shallow_snapshot(&cut)).unwrap();
    let loaded = LoroDoc::new();
    loaded.import(&bytes).unwrap();
    assert_eq!(loaded.get_deep_value(), doc.get_deep_value());
    for id in &ids {
        assert!(
            loaded.has_container(id),
            "{id} was created after the root and must survive"
        );
    }
}

/// Below and above the op count that adds the encoded latest-state overlay.
const RETAINED_FILLER: [usize; 2] = [0, 300];

fn filler(doc: &LoroDoc, n: usize) {
    let other = doc.get_map("other");
    for i in 0..n {
        other.insert(&format!("k{i}"), i as i64).unwrap();
        doc.commit();
    }
}

fn import_fresh(bytes: &[u8]) -> LoroDoc {
    let doc = LoroDoc::new();
    doc.import(bytes).unwrap();
    doc
}

fn shallow_roundtrip(doc: &LoroDoc, cut: &Frontiers) -> LoroDoc {
    import_fresh(&doc.export(ExportMode::shallow_snapshot(cut)).unwrap())
}

/// Fills a node's meta with a scalar, a Text and a nested Map so revival has to
/// bring back a meta subtree, not only the meta map itself.
fn fill_meta(doc: &LoroDoc, node: TreeID, title: &str) {
    let meta = doc.get_tree("tree").get_meta(node).unwrap();
    meta.insert("title", title).unwrap();
    let body = meta.insert_container("body", LoroText::new()).unwrap();
    body.insert(0, &format!("{title} body")).unwrap();
    let props = meta.insert_container("props", LoroMap::new()).unwrap();
    props.insert("owner", title).unwrap();
}

/// Imports a peer-2 op that moves `target` (created by peer 1) under Root on top
/// of the doc's current single-head history, which must be peer 1's only.
fn import_remote_move_to_root(doc: &LoroDoc, target: TreeID) {
    assert_eq!(target.peer, 1);
    let head = doc.oplog_frontiers().as_single().unwrap();
    assert_eq!(head.peer, 1);
    let change = doc.get_change(head).unwrap();
    let lamport = change.lamport + (head.counter - change.id.counter) as u32 + 1;
    let json = serde_json::json!({
        "schema_version": 1,
        "start_version": {},
        "peers": ["1", "2"],
        "changes": [{
            "id": "0@1",
            "timestamp": 0,
            "deps": [format!("{}@0", head.counter)],
            "lamport": lamport,
            "msg": null,
            "ops": [{
                "container": "cid:root-tree:Tree",
                "content": {
                    "type": "move",
                    "target": format!("{}@0", target.counter),
                    "parent": null,
                    "fractional_index": "80",
                },
                "counter": 0,
            }],
        }],
    });
    let updates: JsonSchema = serde_json::from_value(json).unwrap();
    doc.import_json_updates(updates).unwrap();
    assert!(!doc.get_tree("tree").is_node_deleted(&target).unwrap());
}

/// `p -> c`, both with meta. `p` is deleted before the cut, so `c` is dead only
/// because of its ancestor; after the cut `c` is moved back under Root.
fn doc_with_child_revived_from_deleted_parent(filler_ops: usize) -> (LoroDoc, Frontiers, TreeID) {
    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    let tree = doc.get_tree("tree");
    let p = tree.create(TreeParentId::Root).unwrap();
    let c = tree.create(p).unwrap();
    fill_meta(&doc, p, "parent");
    fill_meta(&doc, c, "child");
    doc.commit();
    tree.delete(p).unwrap();
    doc.commit();
    let cut = doc.oplog_frontiers();
    tree.mov(c, TreeParentId::Root).unwrap();
    doc.commit();
    filler(&doc, filler_ops);
    (doc, cut, c)
}

#[test]
fn shallow_snapshot_keeps_meta_of_tree_node_revived_from_deleted_ancestor() {
    for filler_ops in RETAINED_FILLER {
        let (doc, cut, c) = doc_with_child_revived_from_deleted_parent(filler_ops);
        let loaded = shallow_roundtrip(&doc, &cut);
        assert_eq!(
            loaded.get_deep_value(),
            doc.get_deep_value(),
            "{filler_ops} filler ops"
        );
        let meta = loaded.get_tree("tree").get_meta(c).unwrap();
        assert_eq!(
            meta.get_deep_value(),
            doc.get_tree("tree").get_meta(c).unwrap().get_deep_value()
        );
        // The revived replica keeps working as a normal document.
        meta.insert("after", 1).unwrap();
        loaded.commit();
        let peer = doc.fork();
        peer.import(&loaded.export(ExportMode::all_updates()).unwrap())
            .unwrap();
        assert_eq!(peer.get_deep_value(), loaded.get_deep_value());
    }
}

#[test]
fn shallow_reexport_keeps_meta_of_revived_tree_node() {
    // Re-exporting at the existing shallow root reuses the cached root state.
    for filler_ops in RETAINED_FILLER {
        let (doc, cut, _) = doc_with_child_revived_from_deleted_parent(filler_ops);
        let shallow = shallow_roundtrip(&doc, &cut);
        let again = shallow_roundtrip(&shallow, &shallow.shallow_since_frontiers());
        assert_eq!(
            again.get_deep_value(),
            doc.get_deep_value(),
            "{filler_ops} filler ops"
        );
    }
}

#[test]
fn state_only_snapshot_keeps_meta_of_revived_tree_node() {
    for filler_ops in RETAINED_FILLER {
        let (doc, _, _) = doc_with_child_revived_from_deleted_parent(filler_ops);
        let loaded = import_fresh(
            &doc.export(ExportMode::state_only(Some(&doc.oplog_frontiers())))
                .unwrap(),
        );
        assert_eq!(
            loaded.get_deep_value(),
            doc.get_deep_value(),
            "{filler_ops} filler ops"
        );
    }
}

#[test]
fn state_only_snapshot_at_multi_head_target_keeps_revived_tree_node() {
    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    let tree = doc.get_tree("tree");
    let p = tree.create(TreeParentId::Root).unwrap();
    let c = tree.create(p).unwrap();
    fill_meta(&doc, c, "child");
    doc.commit();
    tree.delete(p).unwrap();
    doc.commit();
    let other = doc.fork();
    other.set_peer_id(2).unwrap();
    other.get_map("x").insert("a", 1).unwrap();
    other.commit();
    tree.mov(c, TreeParentId::Root).unwrap();
    doc.commit();
    doc.import(&other.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    let target = doc.oplog_frontiers();
    assert_eq!(target.len(), 2);
    let loaded = import_fresh(&doc.export(ExportMode::state_only(Some(&target))).unwrap());
    assert_eq!(loaded.get_deep_value(), doc.get_deep_value());
}

#[test]
fn shallow_snapshot_keeps_meta_of_directly_deleted_node_revived_by_remote_move() {
    for filler_ops in RETAINED_FILLER {
        let doc = LoroDoc::new();
        doc.set_peer_id(1).unwrap();
        let tree = doc.get_tree("tree");
        let n = tree.create(TreeParentId::Root).unwrap();
        fill_meta(&doc, n, "node");
        doc.commit();
        tree.delete(n).unwrap();
        doc.commit();
        let cut = doc.oplog_frontiers();
        import_remote_move_to_root(&doc, n);
        filler(&doc, filler_ops);
        let loaded = shallow_roundtrip(&doc, &cut);
        assert_eq!(
            loaded.get_deep_value(),
            doc.get_deep_value(),
            "{filler_ops} filler ops"
        );
    }
}

#[test]
fn shallow_snapshot_checkout_matches_full_history_across_tree_revivals() {
    for filler_ops in RETAINED_FILLER {
        let doc = LoroDoc::new();
        doc.set_peer_id(1).unwrap();
        let tree = doc.get_tree("tree");
        let p = tree.create(TreeParentId::Root).unwrap();
        let c = tree.create(p).unwrap();
        let g = tree.create(c).unwrap();
        let d = tree.create(TreeParentId::Root).unwrap();
        fill_meta(&doc, p, "parent");
        fill_meta(&doc, c, "child");
        fill_meta(&doc, g, "grandchild");
        fill_meta(&doc, d, "direct");
        doc.commit();
        tree.delete(p).unwrap();
        doc.commit();
        tree.delete(d).unwrap();
        doc.commit();
        let cut = doc.oplog_frontiers();

        let mut versions = vec![cut.clone()];
        // A peer revives the directly deleted d with its old TreeID.
        import_remote_move_to_root(&doc, d);
        versions.push(doc.oplog_frontiers());
        let mut step = |f: &dyn Fn()| {
            f();
            doc.commit();
            versions.push(doc.oplog_frontiers());
        };
        step(&|| {
            tree.get_meta(d)
                .unwrap()
                .insert("title", "direct v2")
                .unwrap()
        });
        // Revive c (and g under it) out of the deleted parent, then edit it.
        step(&|| tree.mov(c, TreeParentId::Root).unwrap());
        step(&|| {
            let meta = tree.get_meta(c).unwrap();
            meta.insert("title", "child v2").unwrap();
            meta.get("body")
                .unwrap()
                .into_container()
                .unwrap()
                .into_text()
                .unwrap()
                .insert(0, "edited ")
                .unwrap();
        });
        // Delete c again: g is dead through its ancestor a second time.
        step(&|| tree.delete(c).unwrap());
        step(&|| tree.mov(g, TreeParentId::Root).unwrap());
        step(&|| tree.get_meta(g).unwrap().insert("title", "g v2").unwrap());
        step(&|| tree.delete(d).unwrap());
        step(&|| tree.delete(g).unwrap());
        filler(&doc, filler_ops);
        versions.push(doc.oplog_frontiers());

        let loaded = shallow_roundtrip(&doc, &cut);
        assert_eq!(loaded.get_deep_value(), doc.get_deep_value());
        let full = import_fresh(&doc.export(ExportMode::Snapshot).unwrap());
        for version in &versions {
            full.checkout(version).unwrap();
            loaded.checkout(version).unwrap();
            assert_eq!(
                loaded.get_deep_value(),
                full.get_deep_value(),
                "{filler_ops} filler ops, checkout {version:?}"
            );
        }
    }
}

#[test]
fn legacy_shallow_snapshot_without_revived_node_meta_imports() {
    // Exported by the pre-fix exporter from: `p -> c`, meta(c).title = "child",
    // delete(p), cut, mov(c, Root). The root state lacks c's meta, which made
    // import panic ("Parent is not registered") when replaying the move. The
    // meta content is not in the blob, so only the structure can come back.
    let bytes = include_bytes!("./legacy_shallow_revived_tree_node.bin");
    let doc = import_fresh(bytes);
    let c = TreeID::new(1, 1);
    let tree = doc.get_tree("tree");
    assert_eq!(tree.roots(), vec![c]);
    tree.get_meta(c).unwrap().insert("title", "again").unwrap();
    doc.commit();
    let again = import_fresh(&doc.export(ExportMode::Snapshot).unwrap());
    assert_eq!(again.get_deep_value(), doc.get_deep_value());
}

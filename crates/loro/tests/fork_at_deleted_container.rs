use std::borrow::Cow;

use loro::{
    ContainerTrait, ExportMode, Frontiers, LoroCounter, LoroDoc, LoroList, LoroMap,
    LoroMovableList, LoroText, LoroTree, UndoManager,
};

/// Checks that `fork_at(target)` (and a nested `fork_at` of that fork) can
/// travel to every version in `versions` and matches the source document.
fn assert_fork_at_matches_history(doc: &LoroDoc, target: &Frontiers, versions: &[Frontiers]) {
    let reference = LoroDoc::new();
    reference
        .import(&doc.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    let fork = doc.fork_at(target).unwrap();
    let nested = fork.fork_at(target).unwrap();
    let round_trip = LoroDoc::new();
    round_trip
        .import(
            &nested
                .export(ExportMode::SnapshotAt {
                    version: Cow::Borrowed(target),
                })
                .unwrap(),
        )
        .unwrap();
    for candidate in [&fork, &nested, &round_trip] {
        for from in versions.iter().chain([&Frontiers::default()]) {
            for to in versions.iter().chain([&Frontiers::default()]) {
                candidate.diff(from, to).unwrap();
            }
        }
        for v in versions.iter().chain([&Frontiers::default(), target]) {
            reference.checkout(v).unwrap();
            candidate.checkout(v).unwrap();
            assert_eq!(
                candidate.get_deep_value(),
                reference.get_deep_value(),
                "version {v:?}"
            );
        }
        // And back down again, after visiting target.
        for v in versions.iter().rev() {
            reference.checkout(v).unwrap();
            candidate.checkout(v).unwrap();
            assert_eq!(candidate.get_deep_value(), reference.get_deep_value());
        }
    }
}

#[test]
fn issue_1106_fork_at_head_historical_diff_after_parent_deletion() {
    let document = LoroDoc::new();
    document.set_peer_id(1).unwrap();
    let agents = document
        .get_map("root")
        .insert_container("agents", LoroMap::new())
        .unwrap();
    let observer = agents.insert_container("observer", LoroMap::new()).unwrap();
    let permissions = observer
        .insert_container("permissions", LoroMap::new())
        .unwrap();
    let read = permissions
        .insert_container("read", LoroMovableList::new())
        .unwrap();
    read.insert(0, "/resources").unwrap();
    document.commit();
    let seeded = document.oplog_frontiers();

    document.set_peer_id(2).unwrap();
    agents.delete("observer").unwrap();
    document.commit();

    let fork = document.fork_at(&document.oplog_frontiers()).unwrap();
    fork.diff(&Frontiers::default(), &seeded).unwrap();
}

#[test]
fn all_container_types_deep_nesting() {
    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    let root = doc.get_map("root");
    let a = root.insert_container("a", LoroMap::new()).unwrap();
    let b = a.insert_container("b", LoroList::new()).unwrap();
    let c = b.insert_container(0, LoroMap::new()).unwrap();
    let list = c.insert_container("list", LoroList::new()).unwrap();
    list.push(1).unwrap();
    list.push(2).unwrap();
    let mlist = c.insert_container("mlist", LoroMovableList::new()).unwrap();
    mlist.push("x").unwrap();
    mlist.push("y").unwrap();
    let text = c.insert_container("text", LoroText::new()).unwrap();
    text.insert(0, "hello").unwrap();
    text.mark(0..3, "bold", true).unwrap();
    let tree = c.insert_container("tree", LoroTree::new()).unwrap();
    let n = tree.create(None).unwrap();
    let n2 = tree.create(n).unwrap();
    tree.get_meta(n2).unwrap().insert("k", "v").unwrap();
    let counter = c.insert_container("counter", LoroCounter::new()).unwrap();
    counter.increment(3.0).unwrap();
    doc.commit();
    let v1 = doc.oplog_frontiers();
    mlist.mov(0, 1).unwrap();
    mlist.set(0, "z").unwrap();
    text.delete(1, 2).unwrap();
    list.delete(0, 1).unwrap();
    tree.mov(n2, None).unwrap();
    doc.commit();
    let v2 = doc.oplog_frontiers();
    root.delete("a").unwrap();
    doc.commit();
    let v3 = doc.oplog_frontiers();
    assert_fork_at_matches_history(&doc, &v3, &[v1.clone(), v2.clone(), v3.clone()]);
    // Also a target after further unrelated edits.
    doc.get_map("other").insert("x", 1).unwrap();
    doc.commit();
    let v4 = doc.oplog_frontiers();
    assert_fork_at_matches_history(&doc, &v4, &[v1.clone(), v2.clone(), v3.clone(), v4.clone()]);
    assert_fork_at_matches_history(&doc, &v3, &[v1, v2, v3.clone()]);
}

#[test]
fn deleted_tree_node_meta_children() {
    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    let tree = doc.get_tree("tree");
    let n = tree.create(None).unwrap();
    let child = tree.create(n).unwrap();
    let meta = tree.get_meta(child).unwrap();
    let l = meta.insert_container("l", LoroMovableList::new()).unwrap();
    l.push(1).unwrap();
    l.push(2).unwrap();
    let t = meta.insert_container("t", LoroText::new()).unwrap();
    t.insert(0, "abc").unwrap();
    doc.commit();
    let v1 = doc.oplog_frontiers();
    l.mov(0, 1).unwrap();
    doc.commit();
    let v2 = doc.oplog_frontiers();
    tree.delete(n).unwrap();
    doc.commit();
    let v3 = doc.oplog_frontiers();
    assert_fork_at_matches_history(&doc, &v3, &[v1, v2, v3.clone()]);
}

#[test]
fn edit_deleted_container_through_stale_handler() {
    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    let root = doc.get_map("root");
    let p = root.insert_container("p", LoroMap::new()).unwrap();
    let l = p.insert_container("l", LoroMovableList::new()).unwrap();
    l.push("a").unwrap();
    doc.commit();
    let v1 = doc.oplog_frontiers();
    root.delete("p").unwrap();
    doc.commit();
    let v2 = doc.oplog_frontiers();
    // Edits on a deleted container are still recorded in history.
    if l.push("b").is_err() {
        return;
    }
    l.mov(0, 1).unwrap();
    doc.commit();
    let v3 = doc.oplog_frontiers();
    assert_fork_at_matches_history(&doc, &v3, &[v1.clone(), v2.clone(), v3.clone()]);
    assert_fork_at_matches_history(&doc, &v2, &[v1, v2.clone()]);
}

#[test]
fn concurrent_edit_to_deleted_container() {
    let a = LoroDoc::new();
    a.set_peer_id(1).unwrap();
    let p = a
        .get_map("root")
        .insert_container("p", LoroMap::new())
        .unwrap();
    let l = p.insert_container("l", LoroMovableList::new()).unwrap();
    l.push("a").unwrap();
    let t = p.insert_container("t", LoroText::new()).unwrap();
    t.insert(0, "abc").unwrap();
    a.commit();
    let v1 = a.oplog_frontiers();
    let b = a.fork();
    b.set_peer_id(2).unwrap();
    a.get_map("root").delete("p").unwrap();
    a.commit();
    let va = a.oplog_frontiers();
    let bl = b.get_movable_list(l.id());
    bl.push("b").unwrap();
    bl.mov(0, 1).unwrap();
    b.get_text(t.id()).insert(1, "XY").unwrap();
    b.commit();
    let vb = b.oplog_frontiers();
    a.import(&b.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    let head = a.oplog_frontiers();
    assert_fork_at_matches_history(
        &a,
        &head,
        &[v1.clone(), va.clone(), vb.clone(), head.clone()],
    );
    assert_fork_at_matches_history(&a, &va, &[v1.clone(), va.clone()]);
    assert_fork_at_matches_history(&a, &vb, &[v1, vb.clone()]);
}

#[test]
fn deleted_before_target_and_edited_after_target() {
    // Container is dead at the target, but the source doc (at head) has
    // later edits on it: the retained state must be the one at target.
    let a = LoroDoc::new();
    a.set_peer_id(1).unwrap();
    let p = a
        .get_map("root")
        .insert_container("p", LoroMap::new())
        .unwrap();
    let l = p.insert_container("l", LoroMovableList::new()).unwrap();
    l.push("a").unwrap();
    let t = p.insert_container("t", LoroText::new()).unwrap();
    t.insert(0, "abc").unwrap();
    a.commit();
    let v1 = a.oplog_frontiers();
    let b = a.fork();
    b.set_peer_id(2).unwrap();
    a.get_map("root").delete("p").unwrap();
    a.commit();
    let va = a.oplog_frontiers();
    let bl = b.get_movable_list(l.id());
    bl.push("b").unwrap();
    bl.mov(0, 1).unwrap();
    b.get_text(t.id()).delete(0, 2).unwrap();
    b.commit();
    a.import(&b.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    // Source at head; export at va (list dead there, but later edited).
    assert_fork_at_matches_history(&a, &va, &[v1.clone(), va.clone()]);
    // Also: undo/revive-like case, the other peer resurrects p.
    let head = a.oplog_frontiers();
    assert_fork_at_matches_history(&a, &head, &[v1, va, head.clone()]);
}

#[test]
fn undo_revives_deleted_container() {
    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    let mut undo = UndoManager::new(&doc);
    let root = doc.get_map("root");
    let p = root.insert_container("p", LoroMap::new()).unwrap();
    let l = p.insert_container("l", LoroMovableList::new()).unwrap();
    l.push("a").unwrap();
    l.push("b").unwrap();
    doc.commit();
    let v1 = doc.oplog_frontiers();
    root.delete("p").unwrap();
    doc.commit();
    let v2 = doc.oplog_frontiers();
    undo.undo().unwrap();
    doc.commit();
    let v3 = doc.oplog_frontiers();
    let revived = root
        .get("p")
        .unwrap()
        .into_container()
        .unwrap()
        .into_map()
        .unwrap();
    revived
        .get("l")
        .unwrap()
        .into_container()
        .unwrap()
        .into_movable_list()
        .unwrap()
        .mov(0, 1)
        .unwrap();
    doc.commit();
    let v4 = doc.oplog_frontiers();
    undo.redo().unwrap();
    doc.commit();
    let v5 = doc.oplog_frontiers();
    let all = [v1, v2, v3, v4, v5.clone()];
    assert_fork_at_matches_history(&doc, &v5, &all);
    assert_fork_at_matches_history(&doc, &all[2], &all[..3]);
}

#[test]
fn overwritten_container_and_detached_source() {
    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    let root = doc.get_map("root");
    let old = root.insert_container("x", LoroList::new()).unwrap();
    old.push(1).unwrap();
    doc.commit();
    let v1 = doc.oplog_frontiers();
    let new = root.insert_container("x", LoroText::new()).unwrap();
    new.insert(0, "t").unwrap();
    doc.commit();
    let v2 = doc.oplog_frontiers();
    root.insert("x", 5).unwrap();
    doc.commit();
    let v3 = doc.oplog_frontiers();
    doc.checkout(&v1).unwrap();
    assert_fork_at_matches_history(&doc, &v3, &[v1.clone(), v2.clone(), v3.clone()]);
    assert!(doc.is_detached());
    assert_eq!(doc.state_frontiers(), v1);
    assert_fork_at_matches_history(&doc, &v2, &[v1, v2.clone()]);
}

#[test]
fn future_containers_are_not_retained() {
    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    let root = doc.get_map("root");
    root.insert("a", 1).unwrap();
    doc.commit();
    let v1 = doc.oplog_frontiers();
    let later = root.insert_container("later", LoroList::new()).unwrap();
    later.push(1).unwrap();
    root.delete("later").unwrap();
    doc.commit();
    let fork = doc.fork_at(&v1).unwrap();
    assert!(fork.try_get_list(later.id()).is_none());
}

#[test]
fn mergeable_child_of_deleted_parent() {
    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    let root = doc.get_map("root");
    let p = root.insert_container("p", LoroMap::new()).unwrap();
    let l = p.ensure_mergeable_movable_list("l").unwrap();
    l.push("a").unwrap();
    l.push("b").unwrap();
    let t = p.ensure_mergeable_text("t").unwrap();
    t.insert(0, "abc").unwrap();
    doc.commit();
    let v1 = doc.oplog_frontiers();
    root.delete("p").unwrap();
    doc.commit();
    let v2 = doc.oplog_frontiers();
    assert_fork_at_matches_history(&doc, &v2, &[v1, v2.clone()]);
}

#[test]
fn mergeable_child_hidden_by_key_delete() {
    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    let root = doc.get_map("root");
    let l = root.ensure_mergeable_movable_list("l").unwrap();
    l.push("a").unwrap();
    l.push("b").unwrap();
    let t = root.ensure_mergeable_text("t").unwrap();
    t.insert(0, "abc").unwrap();
    doc.commit();
    let v1 = doc.oplog_frontiers();
    root.delete("l").unwrap();
    root.delete("t").unwrap();
    doc.commit();
    let v2 = doc.oplog_frontiers();
    assert_fork_at_matches_history(&doc, &v2, &[v1.clone(), v2.clone()]);
    // Re-ensure resurfaces the preserved state.
    let fork = doc.fork_at(&v2).unwrap();
    let l2 = fork
        .get_map("root")
        .ensure_mergeable_movable_list("l")
        .unwrap();
    let full = doc.fork();
    let l3 = full
        .get_map("root")
        .ensure_mergeable_movable_list("l")
        .unwrap();
    assert_eq!(l2.get_value(), l3.get_value());
}

#[test]
fn mergeable_child_under_deleted_tree_node() {
    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    let tree = doc.get_tree("tree");
    let n = tree.create(None).unwrap();
    let meta = tree.get_meta(n).unwrap();
    let l = meta.ensure_mergeable_list("l").unwrap();
    l.push(1).unwrap();
    doc.commit();
    let v1 = doc.oplog_frontiers();
    tree.delete(n).unwrap();
    doc.commit();
    let v2 = doc.oplog_frontiers();
    assert_fork_at_matches_history(&doc, &v2, &[v1, v2.clone()]);
}

#[test]
fn lazily_loaded_source() {
    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    let root = doc.get_map("root");
    let p = root.insert_container("p", LoroMap::new()).unwrap();
    let l = p.insert_container("l", LoroMovableList::new()).unwrap();
    l.push("a").unwrap();
    let t = p.insert_container("t", LoroText::new()).unwrap();
    t.insert(0, "abc").unwrap();
    doc.commit();
    let v1 = doc.oplog_frontiers();
    root.delete("p").unwrap();
    doc.commit();
    let v2 = doc.oplog_frontiers();
    let lazy = LoroDoc::new();
    lazy.import(&doc.export(ExportMode::Snapshot).unwrap())
        .unwrap();
    assert_fork_at_matches_history(&lazy, &v2, &[v1.clone(), v2.clone()]);
    let lazy = LoroDoc::new();
    lazy.import(&doc.export(ExportMode::Snapshot).unwrap())
        .unwrap();
    assert_fork_at_matches_history(&lazy, &v1, &[v1.clone()]);
}

#[test]
fn shallow_snapshot_after_parent_deletion() {
    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    let root = doc.get_map("root");
    root.insert("x", 0).unwrap();
    doc.commit();
    let p = root.insert_container("p", LoroMap::new()).unwrap();
    let l = p.insert_container("l", LoroMovableList::new()).unwrap();
    l.push("a").unwrap();
    let t = p.insert_container("t", LoroText::new()).unwrap();
    t.insert(0, "abc").unwrap();
    doc.commit();
    let v1 = doc.oplog_frontiers();
    l.push("b").unwrap();
    t.insert(0, "z").unwrap();
    doc.commit();
    let v2 = doc.oplog_frontiers();
    root.delete("p").unwrap();
    doc.commit();
    let v3 = doc.oplog_frontiers();
    let shallow = LoroDoc::new();
    shallow
        .import(
            &doc.export(ExportMode::ShallowSnapshot(Cow::Borrowed(&v1)))
                .unwrap(),
        )
        .unwrap();
    let reference = doc.fork();
    for v in [&v2, &v1, &v3, &v1, &v2] {
        reference.checkout(v).unwrap();
        shallow.checkout(v).unwrap();
        assert_eq!(shallow.get_deep_value(), reference.get_deep_value());
    }
    shallow.diff(&v3, &v1).unwrap();
}

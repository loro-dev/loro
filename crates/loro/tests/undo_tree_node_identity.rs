//! Undo and redo bring a deleted tree node back with the same id, metadata and
//! children instead of making a new node (loro-dev/loro#1212, #938 and #1311).

use std::{
    borrow::Cow,
    sync::{Arc, Mutex},
};

use loro::{
    event::{Diff, DiffBatch, MapDelta},
    ExportMode, LoroDoc, LoroText, LoroValue, TreeID, TreeParentId, UndoManager,
};

fn name_of(doc: &LoroDoc, id: TreeID) -> Option<String> {
    let meta = doc.get_tree("t").get_meta(id).ok()?;
    match meta.get("name")?.into_value().ok()? {
        LoroValue::String(s) => Some(s.to_string()),
        _ => None,
    }
}

/// Every node called `name`, live or deleted, with whether it's deleted.
fn nodes_named(doc: &LoroDoc, name: &str) -> Vec<(TreeID, bool)> {
    let tree = doc.get_tree("t");
    tree.get_nodes(true)
        .into_iter()
        .filter(|n| name_of(doc, n.id).as_deref() == Some(name))
        .map(|n| (n.id, tree.is_node_deleted(&n.id).unwrap()))
        .collect()
}

/// loro-dev/loro#1212: undo of a move into or out of a deleted parent.
#[test]
fn undo_move_with_deleted_parent_keeps_node_id() {
    for into in [true, false] {
        let doc = LoroDoc::new();
        doc.set_peer_id(1).unwrap();
        let tree = doc.get_tree("t");
        let p = tree.create(TreeParentId::Root).unwrap();
        tree.get_meta(p).unwrap().insert("name", "P").unwrap();
        tree.get_meta(tree.create(TreeParentId::Root).unwrap())
            .unwrap()
            .insert("name", "O")
            .unwrap();
        tree.delete(p).unwrap();
        doc.set_next_commit_origin("setup");
        doc.commit();
        let n = tree.create(TreeParentId::Root).unwrap();
        let meta = tree.get_meta(n).unwrap();
        meta.insert("name", "N").unwrap();
        meta.insert_container("text", LoroText::new())
            .unwrap()
            .insert(0, "words")
            .unwrap();
        doc.set_next_commit_origin("setup");
        doc.commit();
        if !into {
            tree.mov(n, p).unwrap();
            doc.set_next_commit_origin("setup");
            doc.commit();
        }

        let mut undo = UndoManager::new(&doc);
        undo.set_merge_interval(0);
        undo.add_exclude_origin_prefix("setup");
        let target = if into {
            TreeParentId::Node(p)
        } else {
            TreeParentId::Root
        };
        tree.mov(n, target).unwrap();
        doc.commit();
        let moved = nodes_named(&doc, "N");
        assert_eq!(moved, vec![(n, into)], "into={into}");

        for _ in 0..2 {
            assert!(undo.undo().unwrap());
            assert_eq!(nodes_named(&doc, "N"), vec![(n, !into)], "into={into}");
            assert!(undo.redo().unwrap());
            assert_eq!(nodes_named(&doc, "N"), moved, "into={into}");
        }
        assert!(undo.undo().unwrap());
        let text = tree.get_meta(n).unwrap().get("text").unwrap();
        assert_eq!(
            text.into_container()
                .unwrap()
                .into_text()
                .unwrap()
                .to_string(),
            "words"
        );
    }
}

/// loro-dev/loro#938: redo of a node creation, then redo of an edit to text
/// keyed by the node's id.
#[test]
fn redo_create_then_edit_keeps_node_id() {
    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    let mut undo = UndoManager::new(&doc);
    undo.set_merge_interval(0);
    undo.add_exclude_origin_prefix("setup");
    let tree = doc.get_tree("tree");
    let root = tree.create(TreeParentId::Root).unwrap();
    let a = tree.create(root).unwrap();
    let text_a = doc.get_text(format!("text_{a}"));
    text_a.insert(0, "Hello World").unwrap();
    doc.set_next_commit_origin("setup");
    doc.commit();

    text_a.delete(5, 6).unwrap();
    let b = tree.create(root).unwrap();
    let text_b = doc.get_text(format!("text_{b}"));
    text_b.insert(0, " World").unwrap();
    doc.commit();
    text_b.insert(0, "你好").unwrap();
    doc.commit();

    assert!(undo.undo().unwrap());
    assert!(undo.undo().unwrap());
    assert_eq!(text_a.to_string(), "Hello World");
    assert!(tree.is_node_deleted(&b).unwrap());

    assert!(undo.redo().unwrap());
    assert_eq!(tree.children(root).unwrap(), vec![a, b]);
    assert!(undo.redo().unwrap());
    assert_eq!(text_b.to_string(), "你好 World");
    assert_eq!(text_a.to_string(), "Hello");
}

/// loro-dev/loro#1311: undoing a move of a node that a remote undo deleted.
#[test]
fn undo_move_of_remotely_deleted_node_makes_no_empty_node() {
    let r0 = LoroDoc::new();
    r0.set_peer_id(1).unwrap();
    let r1 = LoroDoc::new();
    r1.set_peer_id(2).unwrap();
    let mut u0 = UndoManager::new(&r0);
    u0.set_merge_interval(0);
    let mut u1 = UndoManager::new(&r1);
    u1.set_merge_interval(0);
    let sync = |from: &LoroDoc, to: &LoroDoc| {
        to.import(&from.export(ExportMode::updates(&to.oplog_vv())).unwrap())
            .unwrap();
    };
    let t0 = r0.get_tree("t");
    let t1 = r1.get_tree("t");

    let a = t0.create(TreeParentId::Root).unwrap();
    t0.get_meta(a).unwrap().insert("name", "A").unwrap();
    r0.commit();
    let b = t1.create(TreeParentId::Root).unwrap();
    t1.get_meta(b).unwrap().insert("name", "B").unwrap();
    r1.commit();
    sync(&r0, &r1);
    t1.mov(a, b).unwrap();
    r1.commit();
    t1.delete(b).unwrap();
    r1.commit();
    assert!(u1.undo().unwrap());
    assert!(u0.undo().unwrap());
    sync(&r0, &r1);
    u1.undo().unwrap();

    for id in t1.nodes() {
        if t1.is_node_deleted(&id).unwrap() {
            continue;
        }
        let name = name_of(&r1, id);
        assert!(name.is_some(), "live node {id} has no data");
    }
    assert_eq!(
        t1.get_nodes(true)
            .iter()
            .filter(|n| name_of(&r1, n.id).as_deref() == Some("A"))
            .count(),
        1,
        "A was copied"
    );
}

/// A revived node keeps its data, so undo's event carries it, as an import's
/// does: a doc that applies the events ends up with the same state.
#[test]
fn undo_event_of_revived_node_carries_its_data() {
    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    let tree = doc.get_tree("t");
    let n = tree.create(TreeParentId::Root).unwrap();
    let meta = tree.get_meta(n).unwrap();
    meta.insert("name", "N").unwrap();
    meta.insert_container("text", LoroText::new())
        .unwrap()
        .insert(0, "words")
        .unwrap();
    let child = tree.create(n).unwrap();
    tree.get_meta(child).unwrap().insert("name", "C").unwrap();
    doc.commit();
    let mirror = LoroDoc::new();
    mirror.set_peer_id(2).unwrap();
    mirror
        .import(&doc.export(ExportMode::all_updates()).unwrap())
        .unwrap();

    let mut undo = UndoManager::new(&doc);
    undo.set_merge_interval(0);
    tree.delete(n).unwrap();
    doc.commit();
    mirror
        .import(&doc.export(ExportMode::updates(&mirror.oplog_vv())).unwrap())
        .unwrap();

    let batches: Arc<Mutex<Vec<DiffBatch>>> = Default::default();
    let sink = batches.clone();
    let sub = doc.subscribe_root(Arc::new(move |e| {
        let mut batch = DiffBatch::default();
        for d in e.events {
            batch.push(d.target.clone(), owned_diff(d.diff)).unwrap();
        }
        sink.lock().unwrap().push(batch);
    }));
    assert!(undo.undo().unwrap());
    doc.commit();
    drop(sub);
    assert_eq!(nodes_named(&doc, "N"), vec![(n, false)]);

    for batch in batches.lock().unwrap().drain(..) {
        mirror.apply_diff(batch).unwrap();
    }
    mirror.commit();
    assert_eq!(mirror.get_deep_value(), doc.get_deep_value());
}

fn owned_diff(diff: Diff<'_>) -> Diff<'static> {
    match diff {
        Diff::List(l) => Diff::List(l),
        Diff::Text(t) => Diff::Text(t),
        Diff::Map(m) => Diff::Map(MapDelta {
            updated: m
                .updated
                .into_iter()
                .map(|(k, v)| (Cow::Owned(k.into_owned()), v))
                .collect(),
        }),
        Diff::Tree(t) => Diff::Tree(Cow::Owned(t.into_owned())),
        _ => Diff::Unknown,
    }
}

//! A value written again after the shallow root, equal to its value at the
//! root, must not make the root value disappear from a shallow replica.
//!
//! Checkout used to skip map keys whose value is the same at both versions even
//! when a different op wins, so the state kept the later op's lamport/peer. The
//! shallow root state is built by such a checkout, and on import its entry then
//! collided with the retained later op in the map history cache and was dropped:
//! `checkout(root)` lost the key. The root commit holds more than one op so the
//! root-time writer of the key is trimmed from the shallow history.
//!
//! Every test compares a shallow replica against a full-history replica over the
//! whole retained range, with 0 and 300 padding ops (the latter is above the
//! 256-op threshold that ships a latest-state overlay).

use loro::{ExportMode, Frontiers, LoroDoc, LoroMap, LoroValue, ToJson, TreeParentId, UndoManager};

const PADS: [usize; 2] = [0, 300];

struct Fixture {
    doc: LoroDoc,
    root: Frontiers,
    versions: Vec<Frontiers>,
}

fn commit(doc: &LoroDoc, versions: &mut Vec<Frontiers>) {
    doc.commit();
    versions.push(doc.oplog_frontiers());
}

/// Every container kind gets a value at the root, a different value after it,
/// and then its root-time value again.
fn fixture(pad: usize, revert: bool) -> Fixture {
    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    let map = doc.get_map("map");
    map.insert("x", "root").unwrap();
    map.insert("untouched", "root").unwrap();
    let child = map.insert_container("child", LoroMap::new()).unwrap();
    child.insert("x", 1).unwrap();
    let sub = map.insert_container("sub", LoroMap::new()).unwrap();
    sub.insert("k", "v").unwrap();
    let mergeable = map.ensure_mergeable_map("merge").unwrap();
    mergeable.insert("x", "root").unwrap();
    let mlist = doc.get_movable_list("mlist");
    mlist.push("a").unwrap();
    mlist.push("b").unwrap();
    let list = doc.get_list("list");
    list.push("a").unwrap();
    let text = doc.get_text("text");
    text.insert(0, "ab").unwrap();
    let tree = doc.get_tree("tree");
    let n1 = tree.create(TreeParentId::Root).unwrap();
    let n2 = tree.create(TreeParentId::Root).unwrap();
    tree.get_meta(n1).unwrap().insert("x", "root").unwrap();
    let counter = doc.get_counter("counter");
    counter.increment(1.0).unwrap();
    doc.commit();
    let root = doc.oplog_frontiers();
    let mut versions = vec![root.clone()];

    map.insert("x", "later").unwrap();
    map.insert("sub", "not a map").unwrap();
    child.insert("x", 2).unwrap();
    mergeable.insert("x", "later").unwrap();
    mlist.set(0, "later").unwrap();
    mlist.mov(0, 1).unwrap();
    list.delete(0, 1).unwrap();
    list.push("a").unwrap();
    text.delete(0, 1).unwrap();
    text.insert(0, "a").unwrap();
    tree.mov(n1, n2).unwrap();
    tree.get_meta(n1).unwrap().insert("x", "later").unwrap();
    counter.increment(2.0).unwrap();
    commit(&doc, &mut versions);

    let pad_map = doc.get_map("pad");
    for i in 0..pad {
        pad_map.insert("k", i as i64).unwrap();
    }
    commit(&doc, &mut versions);

    if revert {
        doc.revert_to(&root).unwrap();
    } else {
        map.insert("x", "root").unwrap();
        child.insert("x", 1).unwrap();
        mergeable.insert("x", "root").unwrap();
        mlist.mov(1, 0).unwrap();
        mlist.set(0, "a").unwrap();
        tree.mov(n1, TreeParentId::Root).unwrap();
        tree.get_meta(n1).unwrap().insert("x", "root").unwrap();
        counter.decrement(2.0).unwrap();
    }
    commit(&doc, &mut versions);
    map.insert("tail", 1).unwrap();
    commit(&doc, &mut versions);

    Fixture {
        doc,
        root,
        versions,
    }
}

fn import(bytes: &[u8]) -> LoroDoc {
    let doc = LoroDoc::new();
    doc.import(bytes).unwrap();
    doc
}

fn value_at(doc: &LoroDoc, version: &Frontiers) -> LoroValue {
    doc.checkout(version).unwrap();
    let value = doc.get_deep_value();
    doc.checkout_to_latest();
    value
}

fn assert_same_history(full: &LoroDoc, shallow: &LoroDoc, versions: &[Frontiers], ctx: &str) {
    assert_eq!(
        shallow.get_deep_value(),
        full.get_deep_value(),
        "{ctx}: latest"
    );
    for version in versions {
        assert_eq!(
            value_at(shallow, version),
            value_at(full, version),
            "{ctx}: checkout {version:?}"
        );
    }
    for from in versions {
        for to in versions {
            let applied = |doc: &LoroDoc| {
                let diff = doc.diff(from, to).unwrap();
                let fork = full.fork_at(from).unwrap();
                fork.set_detached_editing(true);
                fork.apply_diff(diff).unwrap();
                fork.get_deep_value()
            };
            assert_eq!(
                applied(shallow),
                applied(full),
                "{ctx}: diff {from:?} -> {to:?}"
            );
        }
    }
}

#[test]
fn shallow_checkout_to_root_keeps_values_rewritten_after_root() {
    for pad in PADS {
        for revert in [false, true] {
            let f = fixture(pad, revert);
            let shallow = import(&f.doc.export(ExportMode::shallow_snapshot(&f.root)).unwrap());
            let ctx = format!("pad={pad} revert={revert}");
            assert_eq!(
                value_at(&shallow, &f.root).to_json_value()["map"]["x"],
                "root",
                "{ctx}"
            );
            assert_same_history(&f.doc, &shallow, &f.versions, &ctx);
        }
    }
}

#[test]
fn shallow_reexport_keeps_values_rewritten_after_root() {
    for pad in PADS {
        let f = fixture(pad, true);
        let mut shallow = import(&f.doc.export(ExportMode::shallow_snapshot(&f.root)).unwrap());
        for hop in 0..2 {
            // Checking out first leaves the root-time state in the store, as
            // an export that goes through a checkout would.
            shallow.checkout(&f.root).unwrap();
            shallow.checkout_to_latest();
            shallow = import(
                &shallow
                    .export(ExportMode::shallow_snapshot(&f.root))
                    .unwrap(),
            );
            assert_same_history(
                &f.doc,
                &shallow,
                &f.versions,
                &format!("pad={pad} hop={hop}"),
            );
        }
        // Re-export at a later root from a replica that is already shallow.
        let later = &f.versions[1];
        let reexported = import(&shallow.export(ExportMode::shallow_snapshot(later)).unwrap());
        assert_same_history(
            &f.doc,
            &reexported,
            &f.versions[1..],
            &format!("pad={pad} later"),
        );
    }
}

#[test]
fn state_only_at_root_keeps_values_rewritten_after_root() {
    for pad in PADS {
        let f = fixture(pad, true);
        let state_only = import(&f.doc.export(ExportMode::state_only(Some(&f.root))).unwrap());
        assert_eq!(
            state_only.get_deep_value(),
            value_at(&f.doc, &f.root),
            "pad={pad}"
        );
        // The state-only replica must carry the root-time winners: with the
        // later writer's lamport, LWW would reject the next retained update.
        let next = f.doc.fork_at(&f.versions[1]).unwrap();
        let root_vv = f.doc.frontiers_to_vv(&f.root).unwrap();
        state_only
            .import(&next.export(ExportMode::updates(&root_vv)).unwrap())
            .unwrap();
        assert_eq!(
            state_only.get_deep_value(),
            next.get_deep_value(),
            "pad={pad}"
        );
    }
}

#[test]
fn shallow_undo_matches_full_history() {
    for pad in PADS {
        let f = fixture(pad, true);
        let full = import(&f.doc.export(ExportMode::Snapshot).unwrap());
        let shallow = import(&f.doc.export(ExportMode::shallow_snapshot(&f.root)).unwrap());
        let mut values = Vec::new();
        for doc in [&full, &shallow] {
            doc.set_peer_id(2).unwrap();
            let mut undo = UndoManager::new(doc);
            let map = doc.get_map("map");
            map.insert("x", "local").unwrap();
            doc.get_movable_list("mlist").set(0, "local").unwrap();
            doc.commit();
            undo.record_new_checkpoint().unwrap();
            undo.undo().unwrap();
            let after_undo = doc.get_deep_value();
            undo.redo().unwrap();
            values.push((after_undo, doc.get_deep_value()));
        }
        assert_eq!(values[1], values[0], "pad={pad}");
        assert_eq!(values[0].0.to_json_value()["map"]["x"], "root", "pad={pad}");
    }
}

/// Peer 2 writes back the value peer 1 had at the root. Every replica that
/// materializes the root must report peer 1's op as the winner, not only its
/// value.
#[test]
fn checkout_reports_root_time_winner_of_equal_value() {
    for pad in PADS {
        let doc = LoroDoc::new();
        doc.set_peer_id(1).unwrap();
        let map = doc.get_map("map");
        let list = doc.get_movable_list("mlist");
        map.insert("x", "root").unwrap();
        map.insert("y", "root").unwrap();
        list.push("a").unwrap();
        list.push("b").unwrap();
        doc.commit();
        let root = doc.oplog_frontiers();
        map.insert("x", "later").unwrap();
        list.set(0, "later").unwrap();
        for i in 0..pad {
            doc.get_map("pad").insert("k", i as i64).unwrap();
        }
        doc.commit();
        doc.set_peer_id(2).unwrap();
        map.insert("x", "root").unwrap();
        list.set(0, "a").unwrap();
        doc.commit();

        let full = import(&doc.export(ExportMode::Snapshot).unwrap());
        let shallow = import(&doc.export(ExportMode::shallow_snapshot(&root)).unwrap());
        let state_only = import(&doc.export(ExportMode::state_only(Some(&root))).unwrap());
        let expected = value_at(&full, &root);
        for (name, replica) in [
            ("full", &full),
            ("shallow", &shallow),
            ("state-only", &state_only),
        ] {
            replica.checkout(&root).unwrap();
            let ctx = format!("pad={pad} {name}");
            assert_eq!(
                replica.get_map("map").get_last_editor("x"),
                Some(1),
                "{ctx}"
            );
            assert_eq!(
                replica.get_movable_list("mlist").get_last_editor_at(0),
                Some(1),
                "{ctx}"
            );
            assert_eq!(replica.get_deep_value(), expected, "{ctx}");
            replica.checkout_to_latest();
            // A state-only replica's latest version is the root itself.
            let latest_writer = if name == "state-only" { 1 } else { 2 };
            assert_eq!(
                replica.get_map("map").get_last_editor("x"),
                Some(latest_writer),
                "{ctx} latest"
            );
        }
    }
}

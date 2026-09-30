//! Tree events must turn the tree before an import into the tree after it:
//! every node that becomes alive gets a `Create`, including the descendants of
//! a node revived from a deleted subtree (loro-dev/loro#1157).

use base64::Engine;
use loro::{
    event::Diff, ContainerID, ContainerTrait, ExportMode, LoroDoc, LoroTree, LoroValue,
    TreeExternalDiff, TreeID, TreeParentId, ValueOrContainer,
};
use rand::{rngs::StdRng, seq::SliceRandom, Rng, SeedableRng};
use rustc_hash::FxHashMap;
use std::sync::{Arc, Mutex};

/// The alive part of a tree (children lists and scalar metadata), rebuilt
/// only from events.
#[derive(Default, Debug, Clone, PartialEq)]
struct Mirror {
    children: FxHashMap<TreeParentId, Vec<TreeID>>,
    meta: FxHashMap<TreeID, FxHashMap<String, LoroValue>>,
}

impl Mirror {
    fn from_tree(tree: &LoroTree) -> Self {
        let mut m = Mirror::default();
        let mut stack = vec![TreeParentId::Root];
        while let Some(p) = stack.pop() {
            let children = tree.children(p).unwrap_or_default();
            for c in children.iter() {
                stack.push(TreeParentId::Node(*c));
                let meta = tree.get_meta(*c).unwrap().get_value();
                let meta = meta.into_map().unwrap();
                m.meta.insert(
                    *c,
                    meta.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
                );
            }
            if !children.is_empty() {
                m.children.insert(p, children);
            }
        }
        m
    }

    fn apply_meta(&mut self, container: &ContainerID, updated: &[(String, Option<LoroValue>)]) {
        let Some((_, meta)) = self
            .meta
            .iter_mut()
            .find(|(id, _)| &id.associated_meta_container() == container)
        else {
            // Not an alive node's metadata.
            return;
        };
        for (k, v) in updated {
            match v {
                Some(v) => meta.insert(k.clone(), v.clone()),
                None => meta.remove(k),
            };
        }
    }

    fn contains(&self, id: &TreeID) -> bool {
        self.children.values().any(|c| c.contains(id))
    }

    fn remove(&mut self, target: TreeID, parent: TreeParentId, index: usize) {
        let list = self
            .children
            .get_mut(&parent)
            .unwrap_or_else(|| panic!("{target}: parent {parent:?} has no children in the mirror"));
        assert_eq!(list.get(index), Some(&target), "{target}: wrong old_index");
        list.remove(index);
        if list.is_empty() {
            self.children.remove(&parent);
        }
    }

    fn insert(&mut self, target: TreeID, parent: TreeParentId, index: usize) {
        if let TreeParentId::Node(p) = parent {
            assert!(
                self.contains(&p),
                "{target}: parent {p} is not alive in the mirror"
            );
        }
        let list = self.children.entry(parent).or_default();
        assert!(index <= list.len(), "{target}: index {index} out of bounds");
        list.insert(index, target);
    }

    fn apply(&mut self, target: TreeID, action: &TreeExternalDiff) {
        match action {
            TreeExternalDiff::Create { parent, index, .. } => {
                assert!(!self.contains(&target), "{target}: created twice");
                self.insert(target, *parent, *index);
                // The metadata arrives as map events.
                self.meta.insert(target, FxHashMap::default());
            }
            TreeExternalDiff::Move {
                parent,
                index,
                old_parent,
                old_index,
                ..
            } => {
                self.remove(target, *old_parent, *old_index);
                self.insert(target, *parent, *index);
            }
            TreeExternalDiff::Delete {
                old_parent,
                old_index,
            } => {
                self.remove(target, *old_parent, *old_index);
                // The whole subtree goes with it.
                let mut stack = vec![target];
                while let Some(n) = stack.pop() {
                    self.meta.remove(&n);
                    if let Some(c) = self.children.remove(&TreeParentId::Node(n)) {
                        stack.extend(c);
                    }
                }
            }
        }
    }
}

#[derive(Debug)]
enum Recorded {
    Tree(TreeID, TreeExternalDiff),
    Map(ContainerID, Vec<(String, Option<LoroValue>)>),
}

/// Replays the tree events of a document into a [`Mirror`].
///
/// The subscriber only records the events; they are applied on the test
/// thread, so a bad event fails the test instead of panicking under the
/// document's lock.
struct EventMirror {
    mirror: Mirror,
    events: Arc<Mutex<Vec<Recorded>>>,
    _sub: loro::Subscription,
}

/// Subscribes to `doc`; the mirror starts from the current state of `tree`.
fn mirror_events(doc: &LoroDoc, tree: &LoroTree) -> EventMirror {
    let events = Arc::new(Mutex::new(Vec::new()));
    let recorded = events.clone();
    let tid = tree.id();
    let sub = doc.subscribe_root(Arc::new(move |e| {
        let mut recorded = recorded.lock().unwrap();
        for c in e.events.iter() {
            match &c.diff {
                Diff::Tree(t) if c.target == &tid => {
                    recorded.extend(
                        t.diff
                            .iter()
                            .map(|i| Recorded::Tree(i.target, i.action.clone())),
                    );
                }
                Diff::Map(m) => {
                    let updated = m
                        .updated
                        .iter()
                        .map(|(k, v)| {
                            let v = v.as_ref().map(|v| match v {
                                ValueOrContainer::Value(v) => v.clone(),
                                ValueOrContainer::Container(c) => LoroValue::Container(c.id()),
                            });
                            (k.to_string(), v)
                        })
                        .collect();
                    recorded.push(Recorded::Map(c.target.clone(), updated));
                }
                _ => {}
            }
        }
    }));
    EventMirror {
        mirror: Mirror::from_tree(tree),
        events,
        _sub: sub,
    }
}

fn assert_mirror_matches(m: &mut EventMirror, tree: &LoroTree) {
    let events = std::mem::take(&mut *m.events.lock().unwrap());
    for e in events.iter() {
        match e {
            Recorded::Tree(target, action) => m.mirror.apply(*target, action),
            Recorded::Map(container, updated) => m.mirror.apply_meta(container, updated),
        }
    }
    let expected = Mirror::from_tree(tree);
    for (p, children) in expected.children.iter() {
        assert_eq!(
            m.mirror.children.get(p),
            Some(children),
            "children of {p:?} differ between the events and the state; events: {events:?}"
        );
    }
    assert_eq!(m.mirror, expected, "events: {events:?}");
}

const BASE: &str = "bG9ybwAAAAAAAAAAAAAAAACHcG0ABEEAAQABARABAgAAAAAAAAABAQAAAAAABQEAAAEABgEEAQAAAggBawVidXJzdAAOAQQCAQACAQACAQsCAQEAAwPRB0wAAQEBARsC6QMAAAAAAAACAAAAAAAAAAEBAQEBAQAAAAAFAQAAAQAGAQQBAAACCAFrBWJ1cnN0AA4BBAIBAAIBAAIBCwIBAQADA9oG";
const REST: &str = "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";

/// The repro from loro-dev/loro#1157: `0@1008` is created under a parent that
/// another peer already deleted, gets a child `3@1008`, and is then moved to
/// the root. Importing the rest of the history emitted `Create` for `0@1008`
/// but not for `3@1008`.
#[test]
fn imported_revival_emits_create_for_every_child() {
    let b64 = base64::engine::general_purpose::STANDARD;
    let base = b64.decode(BASE).unwrap();
    let rest = b64.decode(REST).unwrap();

    let doc = LoroDoc::new();
    doc.import(&base).unwrap();
    let tree = doc.get_tree("tree");
    let mut mirror = mirror_events(&doc, &tree);
    doc.import(&rest).unwrap();
    assert_mirror_matches(&mut mirror, &tree);

    // A fresh document importing everything at once.
    let fresh = LoroDoc::new();
    let tree = fresh.get_tree("tree");
    let mut mirror = mirror_events(&fresh, &tree);
    fresh.import(&base).unwrap();
    fresh.import(&rest).unwrap();
    assert_mirror_matches(&mut mirror, &tree);

    // Checking out every version of the history keeps the mirror in sync.
    let all = fresh.export(ExportMode::all_updates()).unwrap();
    let doc = LoroDoc::new();
    doc.import(&all).unwrap();
    let tree = doc.get_tree("tree");
    let mut mirror = mirror_events(&doc, &tree);
    let vv = doc.oplog_vv();
    let mut versions = vec![loro::Frontiers::default()];
    for (peer, end) in vv.iter() {
        for counter in 0..*end {
            versions.push(loro::Frontiers::from_id(loro::ID::new(*peer, counter)));
        }
    }
    for f in versions.iter() {
        doc.checkout(f).unwrap();
        assert_mirror_matches(&mut mirror, &tree);
    }
    doc.checkout_to_latest();
    assert_mirror_matches(&mut mirror, &tree);
}

fn doc_with_peer(peer: u64) -> LoroDoc {
    let doc = LoroDoc::new();
    doc.set_peer_id(peer).unwrap();
    doc
}

/// `p > x > (y1 > z, y2)`, committed by peer 1.
fn doc_with_subtree() -> (LoroDoc, [TreeID; 5]) {
    let doc = doc_with_peer(1);
    let tree = doc.get_tree("tree");
    let p = tree.create(TreeParentId::Root).unwrap();
    let x = tree.create(p).unwrap();
    let y1 = tree.create(x).unwrap();
    let y2 = tree.create(x).unwrap();
    let z = tree.create(y1).unwrap();
    doc.commit();
    (doc, [p, x, y1, y2, z])
}

/// Imports `updates` into a copy of `base` one by one and checks that the
/// events bring a mirror of the tree to the imported state.
fn check_import_events(base: &LoroDoc, updates: &[Vec<u8>]) {
    let doc = LoroDoc::new();
    doc.import(&base.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    let tree = doc.get_tree("tree");
    let mut mirror = mirror_events(&doc, &tree);
    for u in updates {
        doc.import(u).unwrap();
        assert_mirror_matches(&mut mirror, &tree);
    }
}

/// A linear update (`DiffMode::Linear`) that moves `x` out of the deleted `p`
/// revives `x` and its whole subtree.
#[test]
fn linear_import_of_move_out_of_deleted_subtree_creates_descendants() {
    let (doc, [p, x, ..]) = doc_with_subtree();
    let base = doc.fork();
    let tree = doc.get_tree("tree");
    let v0 = doc.oplog_vv();
    tree.delete(p).unwrap();
    doc.commit();
    let v1 = doc.oplog_vv();
    tree.mov(x, TreeParentId::Root).unwrap();
    doc.commit();
    // Both in one import, and one change at a time.
    check_import_events(&base, &[doc.export(ExportMode::updates(&v0)).unwrap()]);
    check_import_events(
        &base,
        &[
            doc.export(ExportMode::updates_till(&v1)).unwrap(),
            doc.export(ExportMode::updates(&v1)).unwrap(),
        ],
    );
}

/// Moving an alive node under a deleted one deletes it; a linear import used to
/// emit nothing for it.
#[test]
fn linear_import_of_move_into_deleted_subtree_deletes_node() {
    let (doc, [p, _, y1, ..]) = doc_with_subtree();
    let tree = doc.get_tree("tree");
    let q = tree.create(TreeParentId::Root).unwrap();
    let r = tree.create(q).unwrap();
    doc.commit();
    let base = doc.fork();
    let v0 = doc.oplog_vv();
    tree.delete(p).unwrap();
    tree.mov(q, y1).unwrap();
    // Moves inside the deleted subtree emit nothing, then `r` comes back.
    tree.mov(q, p).unwrap();
    tree.mov(r, TreeParentId::Root).unwrap();
    doc.commit();
    check_import_events(&base, &[doc.export(ExportMode::updates(&v0)).unwrap()]);
}

/// `DiffMode::ImportGreaterUpdates`: one peer deletes `p` while another moves
/// `x` out of it with a greater lamport. Applied in lamport order, the move
/// revives `x` and its subtree.
#[test]
fn concurrent_delete_and_move_out_creates_descendants() {
    let (doc, [p, x, ..]) = doc_with_subtree();
    let v0 = doc.oplog_vv();

    let a = doc.fork();
    a.set_peer_id(2).unwrap();
    a.get_tree("tree").delete(p).unwrap();
    a.commit();

    let b = doc.fork();
    b.set_peer_id(3).unwrap();
    // Raise the lamport of the move above the delete's.
    b.get_map("m").insert("k", 1).unwrap();
    b.get_tree("tree").mov(x, TreeParentId::Root).unwrap();
    b.commit();

    let merged = doc.fork();
    merged
        .import(&a.export(ExportMode::updates(&v0)).unwrap())
        .unwrap();
    merged
        .import(&b.export(ExportMode::updates(&v0)).unwrap())
        .unwrap();
    let tree = merged.get_tree("tree");
    assert!(!tree.is_node_deleted(&x).unwrap());
    check_import_events(&doc, &[merged.export(ExportMode::updates(&v0)).unwrap()]);
}

/// Undoing a delete re-creates the node and then each of its descendants, one
/// op each. Importing that must create every node exactly once.
#[test]
fn import_of_undone_delete_creates_each_node_once() {
    let (doc, [p, ..]) = doc_with_subtree();
    let tree = doc.get_tree("tree");
    tree.get_meta(p).unwrap().insert("k", 1).unwrap();
    doc.commit();
    let mut undo = loro::UndoManager::new(&doc);
    let base = doc.fork();
    let v0 = doc.oplog_vv();
    tree.delete(p).unwrap();
    doc.commit();
    let v1 = doc.oplog_vv();
    assert!(undo.undo().unwrap());
    doc.commit();
    // The undo brings the subtree back, possibly under new ids.
    assert_eq!(tree.get_nodes(false).len(), 5);
    check_import_events(&base, &[doc.export(ExportMode::updates(&v0)).unwrap()]);
    check_import_events(
        &base,
        &[
            doc.export(ExportMode::updates_till(&v1)).unwrap(),
            doc.export(ExportMode::updates(&v1)).unwrap(),
        ],
    );
}

fn random_parent(nodes: &[TreeID], rng: &mut StdRng) -> TreeParentId {
    match nodes.choose(rng) {
        Some(n) if rng.gen_bool(0.7) => TreeParentId::Node(*n),
        _ => TreeParentId::Root,
    }
}

/// Three peers make random tree edits (including creates under, moves into
/// and moves out of deleted subtrees, and metadata edits) and sync at random
/// with two observers that only import. The observers' imports have no local
/// concurrency, so they take the `Linear` and `ImportGreaterUpdates` modes.
/// After every import the events must rebuild the document's tree.
fn random_sync(seed: u64) {
    const EDITORS: usize = 3;
    const DOCS: usize = 5;
    let mut rng = StdRng::seed_from_u64(seed);
    let docs: Vec<LoroDoc> = (1..=DOCS as u64).map(doc_with_peer).collect();
    let mut mirrors: Vec<EventMirror> = docs
        .iter()
        .map(|d| mirror_events(d, &d.get_tree("tree")))
        .collect();
    for _ in 0..150 {
        if rng.gen_bool(0.4) {
            let i = rng.gen_range(0..DOCS);
            let j = rng.gen_range(0..DOCS);
            if i == j {
                continue;
            }
            let update = docs[j]
                .export(ExportMode::updates(&docs[i].oplog_vv()))
                .unwrap();
            docs[i].import(&update).unwrap();
            assert_mirror_matches(&mut mirrors[i], &docs[i].get_tree("tree"));
        } else {
            let i = rng.gen_range(0..EDITORS);
            let tree = docs[i].get_tree("tree");
            // Includes deleted nodes.
            let nodes = tree.nodes();
            let target = nodes.choose(&mut rng).copied();
            // Errors are expected, e.g. moving a node below itself.
            let _ = match (rng.gen_range(0..10), target) {
                (0..=2, _) | (_, None) => tree.create(random_parent(&nodes, &mut rng)).map(drop),
                (3..=5, Some(t)) => tree.mov(t, random_parent(&nodes, &mut rng)),
                (6, Some(t)) => tree.delete(t),
                (_, Some(t)) => tree.get_meta(t).and_then(|m| {
                    m.insert(
                        &format!("k{}", rng.gen_range(0..3)),
                        rng.gen_range(0..100i64),
                    )
                }),
            };
            docs[i].commit();
            // Local events on deleted subtrees still use the shapes of live
            // ones (a create under a deleted parent emits `Create`, a move out
            // of one emits `Move`), so start the mirror over from the state.
            mirrors[i].events.lock().unwrap().clear();
            mirrors[i].mirror = Mirror::from_tree(&tree);
        }
    }
}

#[test]
fn random_sync_events_rebuild_the_tree() {
    let seeds: Vec<u64> = match std::env::var("TREE_EVENT_SEEDS") {
        Ok(range) => {
            let (start, end) = range.split_once("..").expect("TREE_EVENT_SEEDS=start..end");
            (start.parse().unwrap()..end.parse().unwrap()).collect()
        }
        Err(_) => (0..100).collect(),
    };
    let failed: Vec<u64> = seeds
        .into_iter()
        .filter(|&seed| std::panic::catch_unwind(|| random_sync(seed)).is_err())
        .collect();
    assert!(failed.is_empty(), "failed seeds: {failed:?}");
}

//! `apply_diff` and `revert_to` are all-or-nothing: a batch that fails leaves
//! the document, its history and its subscribers as they were
//! (loro-dev/loro#1154). See `context/apply-diff-atomicity.md`.

use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

use loro::{
    event::{Diff, DiffBatch, ListDiffItem, MapDelta},
    ContainerID, ContainerTrait, ContainerType, ExportMode, Frontiers, LoroCounter, LoroDoc,
    LoroList, LoroMap, LoroResult, LoroText, LoroValue, TextDelta, ToJson, TreeParentId,
    UndoManager, ValueOrContainer, VersionVector,
};
use pretty_assertions::assert_eq;
use rand::{rngs::StdRng, Rng, SeedableRng};
use serde_json::Value;

/// Everything a failed `apply_diff` must leave untouched.
#[derive(Debug, PartialEq)]
struct Observed {
    value: Value,
    vv: VersionVector,
    oplog_frontiers: Frontiers,
    state_frontiers: Frontiers,
    len_ops: usize,
    len_changes: usize,
}

fn observe(doc: &LoroDoc) -> Observed {
    Observed {
        value: doc.get_deep_value().to_json_value(),
        vv: doc.oplog_vv(),
        oplog_frontiers: doc.oplog_frontiers(),
        state_frontiers: doc.state_frontiers(),
        len_ops: doc.len_ops(),
        len_changes: doc.len_changes(),
    }
}

/// The deep value without empty root containers: reading a root through a
/// handler materializes it, a replay only shows roots with content.
fn value_without_empty_roots(doc: &LoroDoc) -> Value {
    let mut value = doc.get_deep_value().to_json_value();
    value.as_object_mut().unwrap().retain(|_, v| match v {
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
        Value::Number(n) => n.as_f64() != Some(0.0),
        _ => true,
    });
    value
}

/// The doc agrees with a fresh replay of its own history and of its snapshot.
fn assert_consistent_with_replay(doc: &LoroDoc) {
    let value = value_without_empty_roots(doc);
    let from_updates = LoroDoc::new();
    from_updates
        .import(&doc.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    assert_eq!(value_without_empty_roots(&from_updates), value);
    let from_snapshot = LoroDoc::new();
    from_snapshot
        .import(&doc.export(ExportMode::Snapshot).unwrap())
        .unwrap();
    assert_eq!(value_without_empty_roots(&from_snapshot), value);
}

fn text_insert(s: &str) -> Diff<'static> {
    Diff::Text(vec![TextDelta::Insert {
        insert: s.into(),
        attributes: None,
    }])
}

fn map_diff(key: &'static str, value: ValueOrContainer) -> Diff<'static> {
    let mut updated = rustc_hash::FxHashMap::default();
    updated.insert(key.into(), Some(value));
    Diff::Map(MapDelta { updated })
}

fn root(name: &str, kind: ContainerType) -> ContainerID {
    ContainerID::new_root(name, kind)
}

/// A batch whose first entries apply and whose last entry is out of bounds.
fn batch_failing_at_the_end() -> DiffBatch {
    let mut batch = DiffBatch::default();
    batch
        .push(root("text", ContainerType::Text), text_insert("partial"))
        .unwrap();
    batch
        .push(
            root("list", ContainerType::List),
            Diff::List(vec![ListDiffItem::Delete { delete: 5 }]),
        )
        .unwrap();
    batch
}

#[test]
fn issue_1154_out_of_bound_entry_rolls_back_the_batch() {
    let doc = LoroDoc::new();
    doc.get_list("list").push(1).unwrap();
    doc.commit();
    let before = observe(&doc);

    let r = doc.apply_diff(batch_failing_at_the_end());
    assert!(
        matches!(r, Err(loro::LoroError::OutOfBound { .. })),
        "{r:?}"
    );
    doc.commit();
    assert_eq!(observe(&doc), before);

    // The doc is still usable, and reuses the op ids of the rolled back batch.
    doc.get_text("text").insert(0, "ok").unwrap();
    doc.commit();
    assert_eq!(
        doc.get_deep_value().to_json_value(),
        serde_json::json!({"list": [1], "text": "ok"})
    );
    assert_consistent_with_replay(&doc);
}

#[test]
fn diff_type_mismatch_rolls_back_the_batch() {
    let doc = LoroDoc::new();
    doc.get_list("list").push(1).unwrap();
    doc.get_map("map").insert("k", 1).unwrap();
    doc.commit();
    let before = observe(&doc);

    let mut batch = DiffBatch::default();
    batch
        .push(
            root("map", ContainerType::Map),
            map_diff("k", ValueOrContainer::Value(2.into())),
        )
        .unwrap();
    batch
        .push(root("text", ContainerType::Text), text_insert("x"))
        .unwrap();
    // A text diff for a list container.
    batch
        .push(root("list", ContainerType::List), text_insert("y"))
        .unwrap();
    assert!(doc.apply_diff(batch).is_err());
    doc.commit();
    assert_eq!(observe(&doc), before);
}

#[test]
fn missing_container_rolls_back_the_batch() {
    let doc = LoroDoc::new();
    doc.get_list("list").push(1).unwrap();
    doc.commit();
    let before = observe(&doc);

    let mut batch = DiffBatch::default();
    batch
        .push(root("text", ContainerType::Text), text_insert("x"))
        .unwrap();
    batch
        .push(
            ContainerID::new_normal(loro::ID::new(42, 7), ContainerType::Text),
            text_insert("y"),
        )
        .unwrap();
    let r = doc.apply_diff(batch);
    assert!(
        matches!(r, Err(loro::LoroError::ContainersNotFound { .. })),
        "{r:?}"
    );
    doc.commit();
    assert_eq!(observe(&doc), before);
}

/// Children created by the failed batch (map/list/movable-list containers,
/// tree nodes and their metadata) must not leave state, parent links or
/// liveness answers behind at the op ids the next edits reuse.
#[test]
fn created_containers_are_discarded_and_their_ids_reused_cleanly() {
    let source = LoroDoc::new();
    source.set_peer_id(1).unwrap();
    let map = source.get_map("map");
    let child = map.insert_container("child", LoroMap::new()).unwrap();
    child.insert("a", 1).unwrap();
    let text = map.insert_container("text", LoroText::new()).unwrap();
    text.insert(0, "hello").unwrap();
    let list = source.get_list("list");
    let inner = list.insert_container(0, LoroList::new()).unwrap();
    inner.push("x").unwrap();
    let mlist = source.get_movable_list("mlist");
    let counter = mlist.insert_container(0, LoroCounter::new()).unwrap();
    counter.increment(2.5).unwrap();
    let tree = source.get_tree("tree");
    let node = tree.create(TreeParentId::Root).unwrap();
    tree.get_meta(node).unwrap().insert("name", "n").unwrap();
    let node2 = tree.create(node).unwrap();
    tree.get_meta(node2).unwrap().insert("name", "m").unwrap();
    source.get_counter("counter").increment(0.2).unwrap();
    source.commit();
    let full = source
        .diff(&Frontiers::default(), &source.oplog_frontiers())
        .unwrap();

    let doc = LoroDoc::new();
    doc.set_peer_id(2).unwrap();
    doc.get_list("list").push(1).unwrap();
    doc.get_counter("counter").increment(0.1).unwrap();
    doc.commit();
    let before = observe(&doc);

    let mut failing = full.clone();
    failing
        .push(
            root("zzz", ContainerType::List),
            Diff::List(vec![ListDiffItem::Delete { delete: 1 }]),
        )
        .unwrap();
    assert!(doc.apply_diff(failing).is_err());
    doc.commit();
    assert_eq!(observe(&doc), before);
    // Restored exactly, not by subtracting the increment again.
    assert_eq!(doc.get_counter("counter").get_value(), 0.1);

    // The same batch again, without the failing entry: it creates containers
    // and tree nodes at the op ids the rolled back batch used.
    doc.apply_diff(full).unwrap();
    doc.commit();
    let value = doc.get_deep_value().to_json_value();
    let expected = source.get_deep_value().to_json_value();
    assert_eq!(value["map"], expected["map"]);
    assert_eq!(value["mlist"], expected["mlist"]);
    assert_eq!(doc.get_tree("tree").nodes().len(), 2);
    for n in doc.get_tree("tree").nodes() {
        assert!(!doc.get_tree("tree").is_node_deleted(&n).unwrap());
        let meta = doc.get_tree("tree").get_meta(n).unwrap();
        assert!(!meta.is_deleted());
        meta.insert("edited", true).unwrap();
    }
    let ValueOrContainer::Container(loro::Container::Map(child)) =
        doc.get_map("map").get("child").unwrap()
    else {
        panic!()
    };
    assert!(!child.is_deleted());
    child.insert("b", 2).unwrap();
    doc.commit();
    assert_consistent_with_replay(&doc);
}

#[test]
fn uncommitted_edits_survive_a_failed_apply_diff() {
    let doc = LoroDoc::new();
    doc.get_list("list").push(1).unwrap();
    doc.commit();
    doc.set_next_commit_message("mine");
    doc.get_text("mine").insert(0, "abc").unwrap();
    doc.get_list("list").push(2).unwrap();

    assert!(doc.apply_diff(batch_failing_at_the_end()).is_err());
    doc.commit();
    assert_eq!(
        doc.get_deep_value().to_json_value(),
        serde_json::json!({"list": [1, 2], "mine": "abc"})
    );
    let last = doc
        .get_change(doc.oplog_frontiers().as_single().unwrap())
        .unwrap();
    assert_eq!(last.message(), "mine");
    assert_consistent_with_replay(&doc);
}

#[test]
fn uncommitted_edits_and_a_successful_apply_diff_keep_the_commit_options() {
    let doc = LoroDoc::new();
    doc.get_list("list").push(1).unwrap();
    doc.commit();
    doc.set_next_commit_message("mine");
    doc.get_text("mine").insert(0, "abc").unwrap();
    let mut batch = DiffBatch::default();
    batch
        .push(root("text", ContainerType::Text), text_insert("diff"))
        .unwrap();
    doc.apply_diff(batch).unwrap();
    doc.commit();
    assert_eq!(
        doc.get_deep_value().to_json_value(),
        serde_json::json!({"list": [1], "mine": "abc", "text": "diff"})
    );
    let last = doc
        .get_change(doc.oplog_frontiers().as_single().unwrap())
        .unwrap();
    assert_eq!(last.message(), "mine");
}

#[test]
fn failed_apply_diff_emits_no_event() {
    let doc = LoroDoc::new();
    doc.get_list("list").push(1).unwrap();
    doc.commit();
    let events = Arc::new(AtomicUsize::new(0));
    let events_in_cb = events.clone();
    let _sub = doc.subscribe_root(Arc::new(move |_| {
        events_in_cb.fetch_add(1, Ordering::SeqCst);
    }));
    let updates = Arc::new(AtomicUsize::new(0));
    let updates_in_cb = updates.clone();
    let _sub2 = doc.subscribe_local_update(Box::new(move |_| {
        updates_in_cb.fetch_add(1, Ordering::SeqCst);
        true
    }));

    assert!(doc.apply_diff(batch_failing_at_the_end()).is_err());
    doc.commit();
    assert_eq!(events.load(Ordering::SeqCst), 0);
    assert_eq!(updates.load(Ordering::SeqCst), 0);

    // A later edit is reported as usual.
    doc.get_text("text").insert(0, "x").unwrap();
    doc.commit();
    assert_eq!(events.load(Ordering::SeqCst), 1);
    assert_eq!(updates.load(Ordering::SeqCst), 1);
}

#[test]
fn undo_manager_does_not_see_a_failed_apply_diff() {
    let doc = LoroDoc::new();
    let mut undo = UndoManager::new(&doc);
    doc.get_list("list").push(1).unwrap();
    doc.commit();
    assert_eq!(undo.undo_count(), 1);

    assert!(doc.apply_diff(batch_failing_at_the_end()).is_err());
    doc.commit();
    assert_eq!(undo.undo_count(), 1);
    assert!(undo.undo().unwrap());
    assert_eq!(
        doc.get_deep_value().to_json_value(),
        serde_json::json!({"list": []})
    );
}

#[test]
fn failed_apply_diff_in_detached_editing_mode_keeps_the_checked_out_version() {
    let doc = LoroDoc::new();
    doc.get_list("list").push(1).unwrap();
    doc.commit();
    let v1 = doc.oplog_frontiers();
    doc.get_list("list").push(2).unwrap();
    doc.commit();
    doc.set_detached_editing(true);
    doc.checkout(&v1).unwrap();
    doc.get_text("text").insert(0, "draft").unwrap();
    doc.commit();
    let before = observe(&doc);

    let mut batch = DiffBatch::default();
    batch
        .push(root("text", ContainerType::Text), text_insert("partial"))
        .unwrap();
    batch
        .push(
            root("list", ContainerType::List),
            Diff::List(vec![ListDiffItem::Delete { delete: 5 }]),
        )
        .unwrap();
    assert!(doc.apply_diff(batch).is_err());
    doc.commit();
    assert!(doc.is_detached());
    assert_eq!(observe(&doc), before);
    doc.get_text("text").insert(0, ">").unwrap();
    doc.commit();
    assert_eq!(
        doc.get_deep_value().to_json_value(),
        serde_json::json!({"list": [1], "text": ">draft"})
    );
}

#[test]
fn failed_apply_diff_on_a_shallow_doc() {
    let source = LoroDoc::new();
    source.get_list("list").push(1).unwrap();
    source.get_text("text").insert(0, "abc").unwrap();
    source.commit();
    source.get_list("list").push(2).unwrap();
    source.commit();
    let doc = LoroDoc::new();
    doc.import(
        &source
            .export(ExportMode::shallow_snapshot(&source.oplog_frontiers()))
            .unwrap(),
    )
    .unwrap();
    let before = observe(&doc);

    let mut batch = DiffBatch::default();
    batch
        .push(
            root("text", ContainerType::Text),
            Diff::Text(vec![
                TextDelta::Retain {
                    retain: 1,
                    attributes: None,
                },
                TextDelta::Delete { delete: 1 },
            ]),
        )
        .unwrap();
    batch
        .push(
            root("list", ContainerType::List),
            Diff::List(vec![ListDiffItem::Delete { delete: 5 }]),
        )
        .unwrap();
    assert!(doc.apply_diff(batch).is_err());
    doc.commit();
    assert_eq!(observe(&doc), before);
    doc.get_text("text").insert(0, "x").unwrap();
    doc.commit();
    assert_eq!(doc.get_text("text").to_string(), "xabc");
}

// ---------------------------------------------------------------------------
// Random stale diffs: `doc.diff(a, b)` applied to a doc that is not at `a`.
// ---------------------------------------------------------------------------

/// Sorted, so that the same random choices pick the same node in two docs
/// whose tree states were built in a different order.
fn alive_nodes(tree: &loro::LoroTree) -> Vec<loro::TreeID> {
    let mut alive: Vec<_> = tree
        .nodes()
        .into_iter()
        .filter(|n| !tree.is_node_deleted(n).unwrap())
        .collect();
    alive.sort();
    alive
}

fn random_edit(doc: &LoroDoc, rng: &mut StdRng) -> LoroResult<()> {
    let text = doc.get_text("text");
    let list = doc.get_list("list");
    let map = doc.get_map("map");
    let mlist = doc.get_movable_list("mlist");
    let tree = doc.get_tree("tree");
    match rng.gen_range(0..14) {
        0 | 1 => {
            let pos = rng.gen_range(0..=text.len_unicode());
            text.insert(pos, ["a", "bc", "def", "🙂"][rng.gen_range(0..4)])?;
        }
        2 => {
            let len = text.len_unicode();
            if len > 0 {
                let pos = rng.gen_range(0..len);
                let n = rng.gen_range(1..=(len - pos).min(3));
                text.delete(pos, n)?;
            }
        }
        3 => {
            let pos = rng.gen_range(0..=list.len());
            if rng.gen_bool(0.3) {
                let child = list.insert_container(pos, LoroText::new())?;
                child.insert(0, "c")?;
            } else {
                list.insert(pos, rng.gen_range(0..100))?;
            }
        }
        4 => {
            if !list.is_empty() {
                list.delete(rng.gen_range(0..list.len()), 1)?;
            }
        }
        5 => {
            let key = ["a", "b", "c", "d"][rng.gen_range(0..4)];
            if rng.gen_bool(0.3) {
                let child = map.insert_container(key, LoroMap::new())?;
                child.insert("x", rng.gen_range(0..10))?;
            } else if rng.gen_bool(0.2) {
                map.delete(key)?;
            } else {
                map.insert(key, rng.gen_range(0..100))?;
            }
        }
        6 => {
            let pos = rng.gen_range(0..=mlist.len());
            if rng.gen_bool(0.3) {
                let child = mlist.insert_container(pos, LoroCounter::new())?;
                child.increment(1.0)?;
            } else {
                mlist.insert(pos, rng.gen_range(0..100))?;
            }
        }
        7 => {
            let len = mlist.len();
            if len > 1 {
                mlist.mov(rng.gen_range(0..len), rng.gen_range(0..len))?;
            } else if len == 1 && rng.gen_bool(0.5) {
                mlist.delete(0, 1)?;
            }
        }
        8 => {
            let len = mlist.len();
            if len > 0 {
                mlist.set(rng.gen_range(0..len), rng.gen_range(0..100))?;
            }
        }
        9 => {
            let alive = alive_nodes(&tree);
            let parent = if alive.is_empty() || rng.gen_bool(0.3) {
                TreeParentId::Root
            } else {
                TreeParentId::Node(alive[rng.gen_range(0..alive.len())])
            };
            let node = tree.create(parent)?;
            tree.get_meta(node)?.insert("v", rng.gen_range(0..10))?;
        }
        10 => {
            let alive = alive_nodes(&tree);
            if !alive.is_empty() {
                let target = alive[rng.gen_range(0..alive.len())];
                if rng.gen_bool(0.4) {
                    tree.delete(target)?;
                } else {
                    let parent = alive[rng.gen_range(0..alive.len())];
                    // Cyclic moves are rejected; that is fine here.
                    let _ = tree.mov(target, parent);
                }
            }
        }
        12 => {
            // Mergeable children: deleting the key hides the child's state, and
            // ensuring it again (here or through a diff) brings the state back.
            let key = ["m1", "m2"][rng.gen_range(0..2)];
            match rng.gen_range(0..4) {
                0 => map.delete(key)?,
                1 => {
                    let t = map.ensure_mergeable_text(key)?;
                    t.insert(0, "t")?;
                }
                2 => {
                    let m = map.ensure_mergeable_map(key)?;
                    m.insert("k", rng.gen_range(0..10))?;
                }
                _ => {
                    let c = map.ensure_mergeable_counter(key)?;
                    c.increment(1.0)?;
                }
            }
        }
        _ => {
            // Integers: float sums depend on the order they are added in.
            doc.get_counter("counter")
                .increment(rng.gen_range(0..10) as f64)?;
        }
    }
    Ok(())
}

/// Builds a doc with a random history of `n` commits from two peers and
/// returns it with the frontiers after each commit.
fn random_history(rng: &mut StdRng, n: usize) -> (LoroDoc, Vec<Frontiers>) {
    let a = LoroDoc::new();
    a.set_peer_id(1).unwrap();
    let b = LoroDoc::new();
    b.set_peer_id(2).unwrap();
    let mut versions = vec![Frontiers::default()];
    for _ in 0..n {
        let doc = if rng.gen_bool(0.5) { &a } else { &b };
        for _ in 0..rng.gen_range(1..4) {
            random_edit(doc, rng).unwrap();
        }
        doc.commit();
        if rng.gen_bool(0.3) {
            a.import(&b.export(ExportMode::all_updates()).unwrap())
                .unwrap();
            b.import(&a.export(ExportMode::all_updates()).unwrap())
                .unwrap();
        }
        versions.push(a.oplog_frontiers());
    }
    a.import(&b.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    versions.push(a.oplog_frontiers());
    (a, versions)
}

fn seeds() -> std::ops::Range<u64> {
    let n = std::env::var("LORO_APPLY_DIFF_ATOMICITY_SEEDS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(120);
    0..n
}

/// The doc's value after each of peer 3's ops. (The ops' storage may be split
/// differently: a rolled back batch keeps its values in the arena, so the next
/// op's values are not adjacent to the previous op's and don't merge with it.)
fn history_of_peer_3(doc: &LoroDoc) -> Vec<Value> {
    let end = doc.oplog_vv().get(&3).copied().unwrap_or(0);
    (0..end)
        .map(|counter| {
            let at = doc.fork_at(&loro::ID::new(3, counter).into()).unwrap();
            value_without_empty_roots(&at)
        })
        .collect()
}

/// A failed `apply_diff` is indistinguishable from not calling it: a twin that
/// makes the same edits without the call ends up with the same state and the
/// same history, op ids included, although the edits after the call reuse the
/// ids of the rolled back batch.
#[test]
fn stale_diffs_are_applied_whole_or_not_at_all() {
    let mut failures = 0;
    for seed in seeds() {
        let mut rng = StdRng::seed_from_u64(seed);
        let (source, versions) = random_history(&mut rng, 12);
        let pick = |rng: &mut StdRng| versions[rng.gen_range(0..versions.len())].clone();
        let (va, vb, vc) = (pick(&mut rng), pick(&mut rng), pick(&mut rng));
        let diff = source.diff(&va, &vb).unwrap();
        let target = source.fork_at(&vc).unwrap();
        target.set_peer_id(3).unwrap();
        let twin = source.fork_at(&vc).unwrap();
        twin.set_peer_id(3).unwrap();
        // Uncommitted edits before the call must survive it.
        let mut edits = StdRng::seed_from_u64(seed ^ 0xed17);
        random_edit(&target, &mut edits.clone()).unwrap();
        random_edit(&twin, &mut edits).unwrap();

        match target.apply_diff(diff) {
            Ok(()) => {
                target.commit();
                for _ in 0..3 {
                    random_edit(&target, &mut rng).unwrap();
                }
                target.commit();
                assert_consistent_with_replay(&target);
            }
            Err(_) => {
                failures += 1;
                target.commit();
                twin.commit();
                assert_eq!(observe(&target), observe(&twin), "seed {seed}");
                for doc in [&target, &twin] {
                    let mut rng = edits.clone();
                    for _ in 0..3 {
                        random_edit(doc, &mut rng).unwrap();
                    }
                    // Creates many containers and tree nodes (or fails the same way).
                    let full = source.diff(&Frontiers::default(), &vb).unwrap();
                    let _ = doc.apply_diff(full);
                    doc.commit();
                }
                assert_eq!(observe(&target), observe(&twin), "seed {seed}");
                assert_eq!(
                    history_of_peer_3(&target),
                    history_of_peer_3(&twin),
                    "seed {seed}"
                );
                assert_consistent_with_replay(&target);
            }
        }
    }
    // The random history must actually exercise the failure path.
    assert!(
        failures > seeds().end / 10,
        "only {failures} failing batches"
    );
}

/// The tree as a set of nodes with metadata: `revert_to` recreates deleted
/// nodes under new ids, and it does not always restore sibling order
/// (a separate issue).
fn without_tree_ids(mut value: Value) -> Value {
    fn strip(v: &mut Value) {
        match v {
            Value::Object(o) => {
                o.remove("id");
                o.remove("parent");
                o.remove("fractional_index");
                o.remove("index");
                o.values_mut().for_each(strip);
            }
            Value::Array(a) => {
                a.iter_mut().for_each(strip);
                a.sort_by_key(|v| v.to_string());
            }
            _ => {}
        }
    }
    if let Some(tree) = value.get_mut("tree") {
        strip(tree);
    }
    value
}

#[test]
fn revert_to_is_whole_or_not_at_all() {
    for seed in seeds() {
        let mut rng = StdRng::seed_from_u64(seed ^ 0x5eed);
        let (doc, versions) = random_history(&mut rng, 10);
        let target = versions[rng.gen_range(0..versions.len())].clone();
        let expected = without_tree_ids(value_without_empty_roots(&doc.fork_at(&target).unwrap()));
        let before = observe(&doc);
        match doc.revert_to(&target) {
            Ok(()) => {
                doc.commit();
                assert_eq!(
                    without_tree_ids(value_without_empty_roots(&doc)),
                    expected,
                    "seed {seed}"
                );
            }
            Err(_) => {
                doc.commit();
                assert_eq!(observe(&doc), before, "seed {seed}");
            }
        }
        assert_consistent_with_replay(&doc);
    }
}

#[test]
fn failed_batch_is_retried_successfully() {
    // The typical recovery: fix the batch and apply it again.
    let doc = LoroDoc::new();
    let list = doc.get_list("list");
    list.push(1).unwrap();
    doc.commit();
    let mut batch = DiffBatch::default();
    batch
        .push(
            root("map", ContainerType::Map),
            map_diff(
                "child",
                ValueOrContainer::Value(LoroValue::Container(ContainerID::new_normal(
                    loro::ID::new(99, 0),
                    ContainerType::Text,
                ))),
            ),
        )
        .unwrap();
    batch
        .push(
            ContainerID::new_normal(loro::ID::new(99, 0), ContainerType::Text),
            text_insert("child text"),
        )
        .unwrap();
    let mut failing = batch.clone();
    failing
        .push(
            root("list", ContainerType::List),
            Diff::List(vec![ListDiffItem::Delete { delete: 3 }]),
        )
        .unwrap();
    assert!(doc.apply_diff(failing).is_err());
    doc.apply_diff(batch).unwrap();
    doc.commit();
    assert_eq!(
        doc.get_deep_value().to_json_value(),
        serde_json::json!({"list": [1], "map": {"child": "child text"}})
    );
    assert_consistent_with_replay(&doc);
}

/// Edits another thread makes while a batch runs can land in the batch's
/// transaction, or commit it. Such a batch is not rolled back, so that those
/// edits are kept.
#[test]
fn edits_of_another_thread_during_a_failing_apply_diff_are_kept() {
    let doc = LoroDoc::new();
    doc.get_list("list").push(1).unwrap();
    doc.commit();
    let n = 2000;
    std::thread::scope(|s| {
        s.spawn(|| {
            for i in 0..n {
                doc.get_list("b").push(i as i64).unwrap();
                if i % 7 == 0 {
                    doc.commit();
                }
            }
            doc.commit();
        });
        s.spawn(|| {
            for _ in 0..n {
                assert!(doc.apply_diff(batch_failing_at_the_end()).is_err());
            }
        });
    });
    doc.commit();
    assert_eq!(doc.get_list("b").len(), n);
    assert_eq!(doc.get_list("list").len(), 1);
    assert_consistent_with_replay(&doc);
}

/// Edits to containers that existed before the rolled back batch (movable-list
/// elements, tree nodes, style anchors and counters keyed by the op ids the
/// next edits reuse) give the same doc as on a twin that never made the call.
#[test]
fn edits_to_existing_containers_after_a_rollback_match_a_twin() {
    let base = LoroDoc::new();
    base.set_peer_id(1).unwrap();
    let ml = base.get_movable_list("ml");
    ml.push("a").unwrap();
    ml.push("b").unwrap();
    let node = base.get_tree("tree").create(TreeParentId::Root).unwrap();
    base.get_tree("tree")
        .get_meta(node)
        .unwrap()
        .insert("v", 1)
        .unwrap();
    base.get_text("text").insert(0, "hello").unwrap();
    base.get_counter("c").increment(0.1).unwrap();
    base.commit();
    let snapshot = base.export(ExportMode::Snapshot).unwrap();

    let source = base.fork();
    source.set_peer_id(2).unwrap();
    let ml = source.get_movable_list("ml");
    ml.insert(1, "x").unwrap();
    ml.mov(0, 2).unwrap();
    ml.set(0, "y").unwrap();
    let tree = source.get_tree("tree");
    let child = tree.create(node).unwrap();
    tree.get_meta(child).unwrap().insert("w", 2).unwrap();
    tree.mov(child, TreeParentId::Root).unwrap();
    let text = source.get_text("text");
    text.mark(0..3, "bold", true).unwrap();
    text.insert(1, "XY").unwrap();
    source.get_counter("c").increment(0.2).unwrap();
    source.commit();
    let diff = source
        .diff(&base.oplog_frontiers(), &source.oplog_frontiers())
        .unwrap();

    let open = |peer| {
        let doc = LoroDoc::new();
        doc.import(&snapshot).unwrap();
        doc.set_peer_id(peer).unwrap();
        doc
    };
    let target = open(3);
    let twin = open(3);

    let mut failing = diff.clone();
    failing
        .push(
            root("zzz", ContainerType::List),
            Diff::List(vec![ListDiffItem::Delete { delete: 1 }]),
        )
        .unwrap();
    assert!(target.apply_diff(failing).is_err());
    target.commit();
    assert_eq!(observe(&target), observe(&twin));
    assert_eq!(target.get_counter("c").get_value(), 0.1);

    for doc in [&target, &twin] {
        let ml = doc.get_movable_list("ml");
        ml.insert(0, "p").unwrap();
        ml.set(1, "q").unwrap();
        ml.mov(0, 2).unwrap();
        let tree = doc.get_tree("tree");
        let n = tree.create(node).unwrap();
        tree.get_meta(n).unwrap().insert("z", 3).unwrap();
        let text = doc.get_text("text");
        text.mark(1..4, "bold", true).unwrap();
        text.insert(2, "Z").unwrap();
        doc.get_counter("c").increment(0.2).unwrap();
        doc.commit();
        doc.apply_diff(diff.clone()).unwrap();
        doc.commit();
    }
    assert_eq!(observe(&target), observe(&twin));
    assert_eq!(
        target.get_text("text").get_richtext_value(),
        twin.get_text("text").get_richtext_value()
    );
    assert_eq!(history_of_peer_3(&target), history_of_peer_3(&twin));
    assert_consistent_with_replay(&target);
}

/// Pending edits stay in the transaction, uncommitted, even when the batch's
/// first ops were merged into theirs (the same text insert run, the same
/// counter): the commit afterwards reports exactly the pending edits.
#[test]
fn pending_edits_merged_with_a_failed_batch_keep_their_ops_and_events() {
    fn events_of(doc: &LoroDoc) -> Arc<std::sync::Mutex<Vec<String>>> {
        let events = Arc::new(std::sync::Mutex::new(Vec::new()));
        let events_in_cb = events.clone();
        doc.subscribe_root(Arc::new(move |e| {
            let diffs: Vec<String> = e
                .events
                .iter()
                .map(|d| format!("{:?} {:?}", d.target, d.diff))
                .collect();
            events_in_cb.lock().unwrap().push(format!("{:?}", diffs));
        }))
        .detach();
        events
    }
    let open = || {
        let doc = LoroDoc::new();
        doc.set_peer_id(3).unwrap();
        doc.get_list("list").push(1).unwrap();
        doc.get_text("text").insert(0, "xy").unwrap();
        doc.commit();
        doc
    };
    let pending = |doc: &LoroDoc| {
        doc.get_text("text").insert(2, "abc").unwrap();
        doc.get_counter("c").increment(1.0).unwrap();
        doc.get_list("list").push(2).unwrap();
    };
    let target = open();
    let twin = open();
    let target_events = events_of(&target);
    let twin_events = events_of(&twin);
    pending(&target);
    pending(&twin);
    let changes = target.len_changes();

    let mut batch = DiffBatch::default();
    batch
        .push(
            root("text", ContainerType::Text),
            Diff::Text(vec![
                TextDelta::Retain {
                    retain: 5,
                    attributes: None,
                },
                TextDelta::Insert {
                    insert: "partial".into(),
                    attributes: None,
                },
            ]),
        )
        .unwrap();
    batch
        .push(root("c", ContainerType::Counter), Diff::Counter(2.0))
        .unwrap();
    batch
        .push(
            root("list", ContainerType::List),
            Diff::List(vec![
                ListDiffItem::Retain { retain: 2 },
                ListDiffItem::Insert {
                    insert: vec![ValueOrContainer::Value(3.into())],
                    is_move: false,
                },
            ]),
        )
        .unwrap();
    batch
        .push(
            root("zzz", ContainerType::List),
            Diff::List(vec![ListDiffItem::Delete { delete: 1 }]),
        )
        .unwrap();
    assert!(target.apply_diff(batch).is_err());
    // Still pending: nothing was committed or reported.
    assert_eq!(target.len_changes(), changes);
    assert!(target_events.lock().unwrap().is_empty());
    assert_eq!(
        target.get_deep_value().to_json_value(),
        twin.get_deep_value().to_json_value()
    );

    target.commit();
    twin.commit();
    assert_eq!(observe(&target), observe(&twin));
    assert_eq!(*target_events.lock().unwrap(), *twin_events.lock().unwrap());
    assert_eq!(target_events.lock().unwrap().len(), 1);
    assert_eq!(history_of_peer_3(&target), history_of_peer_3(&twin));
    assert_consistent_with_replay(&target);
}

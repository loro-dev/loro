//! `apply_diff`, `revert_to` and undo must either succeed or return `Err`
//! without changing the doc when they meet containers of an unknown type
//! (written by a newer Loro). The pre-check in `_apply_diff` has to predict
//! what the apply loop does to recreated, revived and removed containers.

use loro::{
    event::{Diff, DiffBatch, ListDiffItem, MapDelta},
    ContainerID, ContainerTrait, ContainerType, Frontiers, JsonFutureOp, JsonListOp, JsonMapOp,
    JsonMovableListOp, JsonOpContent, LoroCounter, LoroDoc, LoroError, LoroList, LoroMap,
    LoroMovableList, LoroResult, LoroValue, ToJson, TreeParentId, UndoManager, ValueOrContainer,
    ID,
};
use rand::{rngs::StdRng, Rng, SeedableRng};
use rustc_hash::FxHashMap;
use std::borrow::Cow;

const UNKNOWN: ContainerType = ContainerType::Unknown(9);

fn forge_value(v: &mut LoroValue) {
    match v {
        LoroValue::Container(c) => forge_id(c),
        LoroValue::List(l) => l.make_mut().iter_mut().for_each(forge_value),
        LoroValue::Map(m) => m.make_mut().values_mut().for_each(forge_value),
        _ => {}
    }
}

fn forge_id(id: &mut ContainerID) {
    if id.container_type() == ContainerType::Counter {
        *id = match id {
            ContainerID::Root { name, .. } => ContainerID::new_root(name, UNKNOWN),
            ContainerID::Normal { peer, counter, .. } => {
                ContainerID::new_normal(ID::new(*peer, *counter), UNKNOWN)
            }
        };
    }
}

/// Replays `build`'s history into a new doc with every Counter turned into an
/// `Unknown(9)` container, as if a newer Loro had written it. Mergeable
/// markers survive because the typed JSON is edited in memory.
fn forge(build: impl FnOnce(&LoroDoc)) -> LoroDoc {
    forge_as(build, true)
}

fn forge_as(build: impl FnOnce(&LoroDoc), unknown: bool) -> LoroDoc {
    let src = LoroDoc::new();
    src.set_peer_id(1).unwrap();
    build(&src);
    src.commit();
    let mut json =
        src.export_json_updates_without_peer_compression(&Default::default(), &src.oplog_vv());
    for change in json.changes.iter_mut().filter(|_| unknown) {
        for op in change.ops.iter_mut() {
            forge_id(&mut op.container);
            match &mut op.content {
                JsonOpContent::Future(f) => {
                    if let JsonFutureOp::Counter(v) = &f.value {
                        f.value = JsonFutureOp::Unknown(v.clone());
                    }
                }
                JsonOpContent::Map(JsonMapOp::Insert { value, .. }) => forge_value(value),
                JsonOpContent::List(JsonListOp::Insert { value, .. })
                | JsonOpContent::MovableList(JsonMovableListOp::Insert { value, .. }) => {
                    value.iter_mut().for_each(forge_value)
                }
                JsonOpContent::MovableList(JsonMovableListOp::Set { value, .. }) => {
                    forge_value(value)
                }
                _ => {}
            }
        }
    }
    let doc = LoroDoc::new();
    doc.set_peer_id(2).unwrap();
    doc.import_json_updates(json).unwrap();
    doc
}

fn counter() -> LoroCounter {
    LoroCounter::new()
}

fn edited_counter(c: LoroCounter) {
    c.increment(1.0).unwrap();
}

fn state(doc: &LoroDoc) -> (serde_json::Value, loro::VersionVector) {
    doc.commit();
    (doc.get_deep_value().to_json_value(), doc.oplog_vv())
}

fn is_unknown_err(e: &LoroError) -> bool {
    matches!(e, LoroError::ArgErr(msg) if msg.contains("Unknown(9)") && msg.contains("unknown to this version"))
}

/// Runs `f` and checks it either succeeds or fails without changing `doc`.
/// Returns whether it failed on an unknown container.
fn atomic<T>(doc: &LoroDoc, f: impl FnOnce(&LoroDoc) -> LoroResult<T>) -> bool {
    let before = state(doc);
    match f(doc) {
        Ok(_) => false,
        Err(e) => {
            assert_eq!(state(doc), before, "partially applied before {e:?}");
            is_unknown_err(&e)
        }
    }
}

/// `m.k = List[U, 1]`, then `m.k` deleted.
fn deleted_list_holding_unknown() -> (LoroDoc, Frontiers, Frontiers) {
    let doc = forge(|doc| {
        let list = doc
            .get_map("m")
            .insert_container("k", LoroList::new())
            .unwrap();
        edited_counter(list.insert_container(0, counter()).unwrap());
        list.push(1).unwrap();
    });
    let v0 = doc.state_frontiers();
    assert_eq!(state(&doc).0, serde_json::json!({"m": {"k": [null, 1]}}));
    doc.get_map("m").delete("k").unwrap();
    doc.commit();
    (doc.clone(), v0, doc.state_frontiers())
}

#[test]
fn recreating_a_deleted_list_holding_unknown_is_rejected_atomically() {
    let (doc, v0, v1) = deleted_list_holding_unknown();
    let text = doc.get_text("t");
    text.insert(0, "x").unwrap();
    doc.commit();

    let diff = doc.diff(&v1, &v0).unwrap();
    let err = doc.apply_diff(diff).unwrap_err();
    assert!(is_unknown_err(&err), "{err:?}");
    assert!(atomic(&doc, |d| d.apply_diff(d.diff(&v1, &v0)?)));
    assert!(atomic(&doc, |d| d.revert_to(&v0)));
    assert_eq!(state(&doc).0, serde_json::json!({"m": {}, "t": "x"}));
}

#[test]
fn undo_of_deleting_a_list_holding_unknown_fails_without_changes() {
    let doc = forge(|doc| {
        let list = doc
            .get_map("m")
            .insert_container("k", LoroList::new())
            .unwrap();
        edited_counter(list.insert_container(0, counter()).unwrap());
        list.push(1).unwrap();
    });
    let mut undo = UndoManager::new(&doc);
    doc.get_text("t").insert(0, "a").unwrap();
    doc.commit();
    // One step mixing a text edit with the deletion
    doc.get_text("t").insert(1, "b").unwrap();
    doc.get_map("m").delete("k").unwrap();
    doc.commit();
    doc.get_text("t").insert(2, "c").unwrap();
    doc.commit();
    assert_eq!(undo.undo_count(), 3);

    assert!(undo.undo().unwrap());
    assert_eq!(state(&doc).0, serde_json::json!({"m": {}, "t": "ab"}));
    // The mixed step would recreate U: rejected as a whole and dropped
    let before = state(&doc);
    let err = undo.undo().unwrap_err();
    assert!(is_unknown_err(&err), "{err:?}");
    assert_eq!(state(&doc), before);
    assert_eq!(undo.undo_count(), 1);
    // The next undo undoes the step before it, not two steps at once; the
    // text edit of the dropped step stays
    assert!(undo.undo().unwrap());
    assert_eq!(state(&doc).0, serde_json::json!({"m": {}, "t": "b"}));
    assert!(!undo.undo().unwrap());
    // Redo goes forward through the steps that were undone
    assert!(undo.redo().unwrap());
    assert_eq!(state(&doc).0, serde_json::json!({"m": {}, "t": "ab"}));
}

#[test]
fn recreating_a_deleted_tree_node_with_unknown_meta_is_rejected_atomically() {
    let doc = forge(|doc| {
        let tree = doc.get_tree("tree");
        let node = tree.create(None).unwrap();
        let meta = tree.get_meta(node).unwrap();
        meta.insert("a", 1).unwrap();
        edited_counter(meta.insert_container("u", counter()).unwrap());
    });
    let mut undo = UndoManager::new(&doc);
    let v0 = doc.state_frontiers();
    let tree = doc.get_tree("tree");
    let node = tree.roots()[0];
    doc.get_text("t").insert(0, "x").unwrap();
    tree.delete(node).unwrap();
    doc.commit();
    let v1 = doc.state_frontiers();

    assert!(atomic(&doc, |d| d.apply_diff(d.diff(&v1, &v0)?)));
    assert!(atomic(&doc, |d| d.revert_to(&v0)));
    assert!(atomic(&doc, |_| undo.undo()));
}

#[test]
fn recreating_a_mergeable_list_holding_unknown_is_rejected_atomically() {
    let doc = forge(|doc| {
        let list = doc.get_map("m").ensure_mergeable_list("k").unwrap();
        list.push(1).unwrap();
        edited_counter(list.insert_container(0, counter()).unwrap());
    });
    assert_eq!(state(&doc).0, serde_json::json!({"m": {"k": [null, 1]}}));
    let mut undo = UndoManager::new(&doc);
    let v0 = doc.state_frontiers();
    let list = doc.get_map("m").ensure_mergeable_list("k").unwrap();
    list.delete(0, 1).unwrap();
    doc.get_map("m").delete("k").unwrap();
    doc.commit();
    let v1 = doc.state_frontiers();

    assert!(atomic(&doc, |d| d.apply_diff(d.diff(&v1, &v0)?)));
    assert!(atomic(&doc, |d| d.revert_to(&v0)));
    assert!(atomic(&doc, |_| undo.undo()));
}

fn movable_list_with_unknown() -> (LoroDoc, ContainerID) {
    let doc = forge(|doc| {
        let list = doc.get_movable_list("ml");
        list.push("a").unwrap();
        edited_counter(list.insert_container(1, counter()).unwrap());
        list.push("b").unwrap();
    });
    let u = doc
        .get_movable_list("ml")
        .get(1)
        .unwrap()
        .into_container()
        .unwrap()
        .id();
    assert!(u.is_unknown());
    (doc, u)
}

fn value(id: &ContainerID) -> ValueOrContainer {
    ValueOrContainer::Value(LoroValue::Container(id.clone()))
}

fn batch(items: Vec<(ContainerID, Diff<'static>)>) -> DiffBatch {
    let mut batch = DiffBatch::default();
    for (id, diff) in items {
        batch.push(id, diff).unwrap();
    }
    batch
}

#[test]
fn duplicated_unknown_in_one_movable_list_delta_is_rejected_atomically() {
    let (doc, u) = movable_list_with_unknown();
    let ml = ContainerID::new_root("ml", ContainerType::MovableList);
    // Deletes U once but re-inserts it twice
    let diff = batch(vec![
        (
            ContainerID::new_root("t", ContainerType::Text),
            Diff::Text(vec![loro::TextDelta::Insert {
                insert: "x".into(),
                attributes: None,
            }]),
        ),
        (
            ml,
            Diff::List(vec![
                ListDiffItem::Retain { retain: 1 },
                ListDiffItem::Delete { delete: 1 },
                ListDiffItem::Insert {
                    insert: vec![value(&u), value(&u)],
                    is_move: false,
                },
            ]),
        ),
    ]);
    assert!(atomic(&doc, |d| d.apply_diff(diff)));

    // A plain move of U is still accepted
    let diff = batch(vec![(
        ContainerID::new_root("ml", ContainerType::MovableList),
        Diff::List(vec![
            ListDiffItem::Delete { delete: 2 },
            ListDiffItem::Retain { retain: 1 },
            ListDiffItem::Insert {
                insert: vec![value(&u), ValueOrContainer::Value("a".into())],
                is_move: false,
            },
        ]),
    )]);
    doc.apply_diff(diff).unwrap();
    assert_eq!(state(&doc).0, serde_json::json!({"ml": ["b", null, "a"]}));
}

#[test]
fn diffs_skipped_by_apply_are_not_rejected() {
    // A diff for a child list that the same batch deletes first is skipped
    // by `apply_diff`, so its unknown value doesn't matter
    let doc = forge(|doc| {
        doc.get_map("m")
            .insert_container("k", LoroList::new())
            .unwrap()
            .push(1)
            .unwrap();
        doc.get_map("m").insert_container("c", counter()).unwrap();
    });
    let k = doc
        .get_map("m")
        .get("k")
        .unwrap()
        .into_container()
        .unwrap()
        .id();
    let u = doc
        .get_map("m")
        .get("c")
        .unwrap()
        .into_container()
        .unwrap()
        .id();
    assert!(u.is_unknown());
    let diff = batch(vec![
        (
            ContainerID::new_root("m", ContainerType::Map),
            Diff::Map(MapDelta {
                updated: FxHashMap::from_iter([(Cow::Borrowed("k"), None)]),
            }),
        ),
        (
            k,
            Diff::List(vec![ListDiffItem::Insert {
                insert: vec![value(&u)],
                is_move: false,
            }]),
        ),
    ]);
    doc.apply_diff(diff).unwrap();
    assert_eq!(state(&doc).0, serde_json::json!({"m": {"c": null}}));

    // A diff targeting an unknown container is a no-op, whatever it holds
    let diff = batch(vec![(
        u.clone(),
        Diff::List(vec![ListDiffItem::Insert {
            insert: vec![value(&u)],
            is_move: false,
        }]),
    )]);
    doc.apply_diff(diff).unwrap();
    assert_eq!(state(&doc).0, serde_json::json!({"m": {"c": null}}));
}

/// Random edits on a doc holding unknown containers in lists, movable lists,
/// maps, nested containers, tree metas, a mergeable list and a mergeable map. Every
/// `apply_diff(diff(a, b))`, `revert_to` and undo/redo must be atomic.
#[test]
fn random_edits_never_partially_apply() {
    let mut rejected = 0;
    let mut accepted = 0;
    for seed in 0..200u64 {
        let (r, a) = random_case(seed);
        rejected += r;
        accepted += a;
    }
    // Both outcomes must be exercised
    assert!(rejected > 100, "rejected {rejected}");
    assert!(accepted > 100, "accepted {accepted}");
}

fn random_case(seed: u64) -> (usize, usize) {
    random_case_as(seed, true)
}

fn random_case_as(seed: u64, unknown: bool) -> (usize, usize) {
    let doc = forge_as(
        |doc| {
            let m = doc.get_map("m");
            let l = m.insert_container("l", LoroList::new()).unwrap();
            edited_counter(l.insert_container(0, counter()).unwrap());
            l.push(1).unwrap();
            l.insert_container(2, counter()).unwrap();
            let ml = m.insert_container("ml", LoroMovableList::new()).unwrap();
            ml.push("a").unwrap();
            edited_counter(ml.insert_container(1, counter()).unwrap());
            ml.push("b").unwrap();
            ml.insert_container(3, counter()).unwrap();
            let mm = m.insert_container("mm", LoroMap::new()).unwrap();
            edited_counter(mm.insert_container("u", counter()).unwrap());
            mm.insert("x", 1).unwrap();
            let nested = m.insert_container("nested", LoroList::new()).unwrap();
            let inner = nested.insert_container(0, LoroMap::new()).unwrap();
            inner.insert_container("u", counter()).unwrap();
            let merge = m.ensure_mergeable_list("merge").unwrap();
            merge.push(2).unwrap();
            edited_counter(merge.insert_container(0, counter()).unwrap());
            let smap = m.ensure_mergeable_map("smap").unwrap();
            edited_counter(smap.insert_container("u", counter()).unwrap());
            smap.insert("x", 1).unwrap();
            // A regular child of `smap`, kept when `smap` is re-activated
            let sl = smap.insert_container("l", LoroList::new()).unwrap();
            edited_counter(sl.insert_container(0, counter()).unwrap());
            sl.push(1).unwrap();
            let tree = doc.get_tree("tree");
            let n1 = tree.create(None).unwrap();
            let n2 = tree.create(n1).unwrap();
            for n in [n1, n2] {
                let meta = tree.get_meta(n).unwrap();
                meta.insert_container("u", counter()).unwrap();
                meta.insert("n", 1).unwrap();
            }
            doc.get_list("l2").insert_container(0, counter()).unwrap();
        },
        unknown,
    );

    let mut rng = StdRng::seed_from_u64(seed);
    let mut undo = UndoManager::new(&doc);
    let mut versions = vec![doc.state_frontiers()];
    for _ in 0..rng.gen_range(3..12) {
        random_edit(&doc, &mut rng);
        if rng.gen_bool(0.6) {
            doc.commit();
            versions.push(doc.state_frontiers());
        }
    }
    doc.commit();
    versions.push(doc.state_frontiers());

    let mut rejected = 0;
    let mut accepted = 0;
    let mut count = |failed: bool| {
        if failed {
            rejected += 1
        } else {
            accepted += 1
        }
    };
    // A diff only applies to the doc at its start version, so diff from the
    // latest version to an earlier one
    let latest = doc.state_frontiers();
    for _ in 0..3 {
        let target = &versions[rng.gen_range(0..versions.len())];
        // `diff` batches are full states (aligned with the hidden state of
        // re-activated mergeable children); also apply them incrementally
        for full_state in [true, false] {
            let d = doc.fork();
            count(atomic(&d, |d| {
                let mut batch = d.diff(&latest, target)?;
                assert!(batch.is_full_state());
                batch.set_full_state(full_state);
                d.apply_diff(batch)
            }));
        }
        let d = doc.fork();
        count(atomic(&d, |d| d.revert_to(target)));
    }
    for _ in 0..versions.len() {
        count(atomic(&doc, |_| undo.undo()));
    }
    for _ in 0..versions.len() {
        count(atomic(&doc, |_| undo.redo()));
    }
    (rejected, accepted)
}

fn random_edit(doc: &LoroDoc, rng: &mut StdRng) {
    let m = doc.get_map("m");
    let child = |key: &str| m.get(key).and_then(|v| v.into_container().ok());
    match rng.gen_range(0..10) {
        0 => {
            let lists = ["l", "nested"]
                .into_iter()
                .filter_map(|k| child(k).and_then(|c| c.into_list().ok()))
                .chain([doc.get_list("l2")])
                .collect::<Vec<_>>();
            let l = &lists[rng.gen_range(0..lists.len())];
            if !l.is_empty() {
                l.delete(rng.gen_range(0..l.len()), 1).unwrap();
            } else {
                l.push(rng.gen_range(0..10)).unwrap();
            }
        }
        1 | 2 => {
            if let Some(ml) = child("ml").and_then(|c| c.into_movable_list().ok()) {
                let len = ml.len();
                match (len, rng.gen_range(0..3)) {
                    (0, _) => ml.push("z").unwrap(),
                    (_, 0) => ml.delete(rng.gen_range(0..len), 1).unwrap(),
                    (_, 1) => ml.set(rng.gen_range(0..len), rng.gen_range(0..10)).unwrap(),
                    _ => ml
                        .mov(rng.gen_range(0..len), rng.gen_range(0..len))
                        .unwrap(),
                }
            }
        }
        3 => {
            let keys = ["l", "ml", "mm", "nested"];
            m.delete(keys[rng.gen_range(0..keys.len())]).unwrap();
        }
        4 => {
            if let Some(mm) = child("mm").and_then(|c| c.into_map().ok()) {
                if rng.gen_bool(0.5) {
                    mm.delete("u").unwrap();
                } else {
                    mm.insert("x", rng.gen_range(0..10)).unwrap();
                }
            }
        }
        5 => {
            let tree = doc.get_tree("tree");
            let nodes = tree.nodes();
            let alive: Vec<_> = nodes
                .into_iter()
                .filter(|n| !tree.is_node_deleted(n).unwrap())
                .collect();
            match rng.gen_range(0..3) {
                0 if !alive.is_empty() => {
                    tree.delete(alive[rng.gen_range(0..alive.len())]).unwrap()
                }
                1 if alive.len() >= 2 => {
                    let _ = tree.mov(alive[0], TreeParentId::Root);
                }
                _ => {
                    let n = tree.create(None).unwrap();
                    tree.get_meta(n).unwrap().insert("n", 2).unwrap();
                }
            }
        }
        6 => {
            let merge = m.ensure_mergeable_list("merge").unwrap();
            if !merge.is_empty() && rng.gen_bool(0.6) {
                merge.delete(rng.gen_range(0..merge.len()), 1).unwrap();
            } else if rng.gen_bool(0.5) {
                m.delete("merge").unwrap();
            } else {
                merge.push(rng.gen_range(0..10)).unwrap();
            }
        }
        7 => {
            if let Some(nested) = child("nested").and_then(|c| c.into_list().ok()) {
                if let Some(inner) = nested.get(0).and_then(|v| v.into_container().ok()) {
                    if let Ok(inner) = inner.into_map() {
                        inner.delete("u").unwrap();
                    }
                }
            }
        }
        8 => {
            // Deleting and re-ensuring re-activates the hidden `smap`
            let smap = m.ensure_mergeable_map("smap").unwrap();
            match rng.gen_range(0..4) {
                0 => m.delete("smap").unwrap(),
                1 => smap.delete("u").unwrap(),
                2 => {
                    if let Some(Ok(sl)) = smap
                        .get("l")
                        .and_then(|v| v.into_container().ok())
                        .map(|c| c.into_list())
                    {
                        if !sl.is_empty() {
                            sl.delete(0, 1).unwrap();
                        }
                    }
                }
                _ => smap.insert("x", rng.gen_range(0..10)).unwrap(),
            }
        }
        _ => {
            let t = doc.get_text("t");
            t.insert(t.len_unicode(), "x").unwrap();
        }
    }
}

/// The same edits with the placeholders left as Counters: nothing is
/// rejected, so the rejections above come from the unknown containers.
#[test]
fn random_edits_without_unknown_containers_are_accepted() {
    for seed in 0..200u64 {
        assert_eq!(random_case_as(seed, false).0, 0, "seed {seed}");
    }
}

// Full-state batches (`LoroDoc::diff`) align a re-activated mergeable child
// with the state this doc kept for it. Keeping an unknown container there
// creates nothing, so it must be accepted; only the edits that alignment
// actually applies may be rejected.

/// The four places an unknown container can sit in a mergeable child `m.s`.
#[derive(Clone, Copy, Debug)]
enum Holder {
    Map,
    List,
    MovableList,
    TreeMeta,
}

const HOLDERS: [Holder; 4] = [
    Holder::Map,
    Holder::List,
    Holder::MovableList,
    Holder::TreeMeta,
];

/// Builds `m.s` of the given kind holding an edited unknown container U and
/// a plain value.
fn build_holder(doc: &LoroDoc, holder: Holder) {
    let m = doc.get_map("m");
    match holder {
        Holder::Map => {
            let s = m.ensure_mergeable_map("s").unwrap();
            edited_counter(s.insert_container("u", counter()).unwrap());
            s.insert("x", 1).unwrap();
        }
        Holder::List => {
            let s = m.ensure_mergeable_list("s").unwrap();
            edited_counter(s.insert_container(0, counter()).unwrap());
            s.push(1).unwrap();
        }
        Holder::MovableList => {
            let s = m.ensure_mergeable_movable_list("s").unwrap();
            s.push("a").unwrap();
            edited_counter(s.insert_container(1, counter()).unwrap());
        }
        Holder::TreeMeta => {
            let s = m.ensure_mergeable_tree("s").unwrap();
            let node = s.create(None).unwrap();
            let meta = s.get_meta(node).unwrap();
            edited_counter(meta.insert_container("u", counter()).unwrap());
            meta.insert("x", 1).unwrap();
        }
    }
}

/// Adds a plain value to the (visible) `m.s`.
fn add_plain_value(doc: &LoroDoc, holder: Holder) {
    let m = doc.get_map("m");
    match holder {
        Holder::Map => m.ensure_mergeable_map("s").unwrap().insert("y", 2).unwrap(),
        Holder::List => m.ensure_mergeable_list("s").unwrap().push(2).unwrap(),
        Holder::MovableList => m
            .ensure_mergeable_movable_list("s")
            .unwrap()
            .push("b")
            .unwrap(),
        Holder::TreeMeta => {
            m.ensure_mergeable_tree("s").unwrap().create(None).unwrap();
        }
    }
}

/// The id of the unknown container U in the visible `m.s`.
fn unknown_in_holder(doc: &LoroDoc, holder: Holder) -> ContainerID {
    let m = doc.get_map("m");
    let v = match holder {
        Holder::Map => m.ensure_mergeable_map("s").unwrap().get("u"),
        Holder::List => m.ensure_mergeable_list("s").unwrap().get(0),
        Holder::MovableList => m.ensure_mergeable_movable_list("s").unwrap().get(1),
        Holder::TreeMeta => {
            let tree = m.ensure_mergeable_tree("s").unwrap();
            tree.get_meta(tree.roots()[0]).unwrap().get("u")
        }
    };
    let id = v.unwrap().into_container().unwrap().id();
    assert!(id.is_unknown(), "{holder:?}: {id}");
    id
}

/// With the target state `same` as the hidden one, or hidden `m.s` holding
/// an extra plain value (`different`), re-activating `m.s` from a full-state
/// batch only keeps U. It used to be rejected as creating U.
#[test]
fn full_state_revival_keeps_existing_unknown_containers() {
    for holder in HOLDERS {
        for different in [false, true] {
            let doc = forge(|doc| build_holder(doc, holder));
            let target = doc.state_frontiers();
            let expected = state(&doc).0;
            let unknown = unknown_in_holder(&doc, holder);
            if different {
                add_plain_value(&doc, holder);
            }
            doc.get_text("t").insert(0, "x").unwrap();
            doc.get_map("m").delete("s").unwrap();
            doc.commit();

            let batch = doc.diff(&doc.state_frontiers(), &target).unwrap();
            assert!(batch.is_full_state());
            doc.apply_diff(batch)
                .unwrap_or_else(|e| panic!("{holder:?} different={different}: {e:?}"));
            assert_eq!(
                state(&doc).0["m"],
                expected["m"],
                "{holder:?} different={different}"
            );
            assert_eq!(
                unknown_in_holder(&doc, holder),
                unknown,
                "{holder:?} different={different}"
            );
        }
    }
}

/// Applied to a doc that has no hidden state for `m.s`, the same full-state
/// batch would have to create U: rejected before anything is written.
#[test]
fn full_state_revival_without_hidden_unknown_is_rejected_atomically() {
    for holder in HOLDERS {
        let src = forge(|doc| {
            doc.get_map("m").insert("k", 0).unwrap();
            build_holder(doc, holder)
        });
        let target = src.state_frontiers();
        src.get_map("m").delete("s").unwrap();
        src.commit();
        let batch = src.diff(&src.state_frontiers(), &target).unwrap();
        assert!(batch.is_full_state());

        // Same visible state as `src`, but `m.s` never existed
        let doc = LoroDoc::new();
        doc.get_map("m").insert("k", 0).unwrap();
        doc.get_text("t").insert(0, "x").unwrap();
        doc.commit();
        let mut with_witness = DiffBatch::default();
        with_witness
            .push(
                ContainerID::new_root("t", ContainerType::Text),
                Diff::Text(vec![loro::TextDelta::Insert {
                    insert: "partial".into(),
                    attributes: None,
                }]),
            )
            .unwrap();
        for (id, diff) in batch.iter() {
            with_witness.push(id.clone(), diff.clone()).unwrap();
        }
        with_witness.set_full_state(true);
        assert!(
            atomic(&doc, |d| d.apply_diff(with_witness)),
            "{holder:?} was not rejected"
        );
    }
}

/// A hidden child that full-state alignment keeps (`m.s.l`) is still aligned
/// although it is unreachable before the batch. If its aligned edit has to
/// create U, the batch is rejected before anything is written.
#[test]
fn full_state_revival_that_recreates_unknown_in_a_kept_child_is_rejected_atomically() {
    let doc = forge(|doc| {
        let s = doc.get_map("m").ensure_mergeable_map("s").unwrap();
        let l = s.insert_container("l", LoroList::new()).unwrap();
        edited_counter(l.insert_container(0, counter()).unwrap());
        l.push(1).unwrap();
    });
    let target = doc.state_frontiers();
    let s = doc.get_map("m").ensure_mergeable_map("s").unwrap();
    let l = s
        .get("l")
        .unwrap()
        .into_container()
        .unwrap()
        .into_list()
        .unwrap();
    l.delete(0, 1).unwrap();
    doc.get_text("t").insert(0, "x").unwrap();
    doc.get_map("m").delete("s").unwrap();
    doc.commit();

    let batch = doc.diff(&doc.state_frontiers(), &target).unwrap();
    assert!(batch.is_full_state());
    assert!(atomic(&doc, |d| d.apply_diff(batch)));
}

const WASM_FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../loro-wasm/tests/fixtures/unknown_mergeable_holders.ts"
);

/// loro-crdt tests can't forge mergeable children holding unknown
/// containers: JSON updates don't keep the binary mergeable markers. They
/// import these updates instead.
fn wasm_fixture() -> String {
    use base64::Engine;
    let mut out = String::from(
        "// Generated by `cargo test -p loro --test unknown_container_atomicity -- --ignored \\\n\
         //   write_wasm_fixture`. `all_updates` of a doc whose mergeable child `m.s` holds\n\
         // an unknown container `Unknown(9)` (forged from a Counter) and a plain value.\n\
         export const UNKNOWN_MERGEABLE_HOLDERS = {\n",
    );
    for (name, holder) in ["map", "list", "movableList", "treeMeta"]
        .into_iter()
        .zip(HOLDERS)
    {
        let doc = forge(|doc| build_holder(doc, holder));
        let bytes = doc.export(loro::ExportMode::all_updates()).unwrap();
        let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
        out.push_str(&format!("  {name}:\n    \"{b64}\",\n"));
    }
    out.push_str("} as const;\n");
    out
}

#[test]
#[ignore]
fn write_wasm_fixture() {
    std::fs::write(WASM_FIXTURE, wasm_fixture()).unwrap();
}

#[test]
fn wasm_fixture_is_up_to_date() {
    assert_eq!(
        std::fs::read_to_string(WASM_FIXTURE).unwrap(),
        wasm_fixture(),
        "run the ignored `write_wasm_fixture` test"
    );
}

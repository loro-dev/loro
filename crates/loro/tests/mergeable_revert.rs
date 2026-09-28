//! Reverting to a version where a mergeable child was visible must restore the child's
//! content exactly once.
//!
//! A mergeable child keeps its state at a deterministic cid while the parent marker is
//! gone. Revert and undo apply the child's actual change, which keeps char/element/TreeID
//! identity. `LoroDoc::diff` reports a re-activated child with its full state and marks the
//! batch (`DiffBatch::is_full_state`), so `apply_diff` aligns it with whatever hidden state
//! the target has. Local events keep main's shape (increments), so forwarding them needs a
//! target that shares the source's hidden state. See `context/mergeable-containers.md`.

use loro::{
    event::{Diff, DiffBatch, MapDelta},
    ExpandType, Frontiers, LoroDoc, StyleConfig, ToJson, TreeParentId, UndoManager,
};
use serde_json::{json, Value};
use std::{
    borrow::Cow,
    sync::{Arc, Mutex},
};

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
        #[cfg(feature = "counter")]
        Diff::Counter(c) => Diff::Counter(c),
        _ => Diff::Unknown,
    }
}

/// Runs `op` on `source` and forwards the local events it emits to `mirror` through
/// `apply_diff`. `mirror` must start in the same state as `source`.
fn forward_events(source: &LoroDoc, mirror: &LoroDoc, op: impl FnOnce(&LoroDoc)) {
    let batches: Arc<Mutex<Vec<DiffBatch>>> = Default::default();
    let sink = batches.clone();
    let sub = source.subscribe_root(Arc::new(move |e| {
        let mut batch = DiffBatch::default();
        for d in e.events {
            batch
                .push(d.target.clone(), owned_diff(d.diff))
                .unwrap_or_else(|_| panic!("duplicate container in one event"));
        }
        sink.lock().unwrap().push(batch);
    }));
    op(source);
    source.commit();
    drop(sub);
    for batch in batches.lock().unwrap().drain(..) {
        mirror.apply_diff(batch).unwrap();
    }
    mirror.commit();
}

/// A new doc with `d`'s visible state and none of its history or hidden state.
fn visible_copy(d: &LoroDoc) -> LoroDoc {
    let copy = LoroDoc::new();
    copy.set_peer_id(9).unwrap();
    copy.apply_diff(d.diff(&Frontiers::default(), &d.state_frontiers()).unwrap())
        .unwrap();
    copy.commit();
    copy
}

fn doc() -> LoroDoc {
    let d = LoroDoc::new();
    d.set_peer_id(1).unwrap();
    d.config_default_text_style(Some(StyleConfig {
        expand: ExpandType::After,
    }));
    d
}

/// Deep JSON with tree node ids and empty roots stripped: reviving a deleted regular tree
/// node through `apply_diff` creates a fresh node id, which is unrelated to this bug.
fn json(d: &LoroDoc) -> Value {
    fn strip(v: &mut Value) {
        match v {
            Value::Object(o) => {
                if o.contains_key("fractional_index") {
                    o.remove("id");
                    o.remove("parent");
                }
                o.values_mut().for_each(strip);
            }
            Value::Array(a) => a.iter_mut().for_each(strip),
            _ => {}
        }
    }
    let mut v = d.get_deep_value().to_json_value();
    strip(&mut v);
    // Empty root containers only exist in docs that touched them.
    v.as_object_mut().unwrap().retain(|_, root| match root {
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
        _ => true,
    });
    v
}

fn assert_replays_to(d: &LoroDoc, expected: &Value, what: &str) {
    assert_eq!(&json(d), expected, "{what}: wrong state");
    let replay = LoroDoc::new();
    replay
        .import(&d.export(loro::ExportMode::all_updates()).unwrap())
        .unwrap();
    // Empty root containers only exist in docs that touched them.
    let non_empty_roots = |v: &Value| {
        let mut v = v.clone();
        v.as_object_mut().unwrap().retain(|_, root| match root {
            Value::Array(a) => !a.is_empty(),
            Value::Object(o) => !o.is_empty(),
            _ => true,
        });
        v
    };
    assert_eq!(
        non_empty_roots(&json(&replay)),
        non_empty_roots(expected),
        "{what}: ops replay to a different state"
    );
}

/// Runs `setup`, records version `a`, runs `mutate`, then checks that every way of
/// going back to `a` (revert_to, diff + apply_diff, undo, checkout) yields `a`'s state.
///
/// With `reuses_children`, the reverted subtree hangs off a mergeable root, so revert must
/// reuse the hidden containers: reverting to `a` a second time emits no ops. (Regular
/// containers revived by `apply_diff` get fresh ids, so a second revert rewrites them.)
fn check(setup: impl Fn(&LoroDoc), mutate: impl Fn(&LoroDoc), reuses_children: bool) {
    let d = doc();
    setup(&d);
    d.commit();
    let a = d.state_frontiers();
    let expected_a = json(&d);
    mutate(&d);
    d.commit();
    let b = d.state_frontiers();
    let expected_b = json(&d);
    assert_ne!(expected_a, expected_b, "mutation must change the state");

    // revert_to
    let r = d.fork();
    r.revert_to(&a).unwrap();
    r.commit();
    assert_replays_to(&r, &expected_a, "revert_to");
    let len = r.len_ops();
    r.revert_to(&a).unwrap();
    r.commit();
    if reuses_children {
        assert_eq!(r.len_ops(), len, "second revert_to must not emit ops");
    }
    assert_replays_to(&r, &expected_a, "revert_to twice");
    // Revert forward again.
    r.revert_to(&b).unwrap();
    r.commit();
    assert_replays_to(&r, &expected_b, "revert_to back to b");

    // Local events of a revert, forwarded to a doc that shares the hidden state.
    let source = d.fork();
    let mirror = d.fork();
    forward_events(&source, &mirror, |s| s.revert_to(&a).unwrap());
    assert_eq!(json(&source), expected_a, "event forwarding: source");
    assert_eq!(json(&mirror), expected_a, "event forwarding: mirror");

    // Public (full-state) diff applied to a doc that only has the visible state.
    let visible_only = visible_copy(&d);
    visible_only.apply_diff(d.diff(&b, &a).unwrap()).unwrap();
    visible_only.commit();
    assert_eq!(json(&visible_only), expected_a, "diff to visible-only doc");
    // Without the flag, it is applied as on main, which is right for a doc without hidden
    // state.
    let visible_only = visible_copy(&d);
    let mut diff = d.diff(&b, &a).unwrap();
    diff.set_full_state(false);
    visible_only.apply_diff(diff).unwrap();
    visible_only.commit();
    assert_eq!(
        json(&visible_only),
        expected_a,
        "incremental diff to visible-only doc"
    );

    // diff + apply_diff
    let r = d.fork();
    let diff = r.diff(&b, &a).unwrap();
    r.apply_diff(diff).unwrap();
    r.commit();
    assert_replays_to(&r, &expected_a, "diff + apply_diff");

    // checkout
    let r = d.fork();
    r.checkout(&a).unwrap();
    assert_eq!(json(&r), expected_a, "checkout a");
    r.checkout(&b).unwrap();
    assert_eq!(json(&r), expected_b, "checkout b");
    r.checkout_to_latest();
    assert_eq!(json(&r), expected_b, "checkout latest");

    // undo / redo
    let u = doc();
    setup(&u);
    u.commit();
    let mut undo = UndoManager::new(&u);
    mutate(&u);
    u.commit();
    assert_eq!(json(&u), expected_b);
    assert!(undo.undo().unwrap());
    u.commit();
    assert_replays_to(&u, &expected_a, "undo");
    assert!(undo.redo().unwrap());
    u.commit();
    assert_replays_to(&u, &expected_b, "redo");
    assert!(undo.undo().unwrap());
    u.commit();
    assert_replays_to(&u, &expected_a, "undo after redo");
}

fn delete_s(d: &LoroDoc) {
    d.get_map("m").delete("s").unwrap();
}

#[test]
fn mergeable_text_revert_after_delete() {
    check(
        |d| {
            d.get_map("m")
                .ensure_mergeable_text("s")
                .unwrap()
                .insert(0, "hello")
                .unwrap();
        },
        delete_s,
        true,
    );
}

#[test]
fn mergeable_text_with_styles_revert_after_delete() {
    check(
        |d| {
            let t = d.get_map("m").ensure_mergeable_text("s").unwrap();
            t.insert(0, "hello world").unwrap();
            t.mark(0..5, "bold", true).unwrap();
        },
        delete_s,
        true,
    );
}

#[test]
#[cfg(feature = "counter")]
fn mergeable_counter_revert_after_delete() {
    check(
        |d| {
            d.get_map("m")
                .ensure_mergeable_counter("s")
                .unwrap()
                .increment(7.0)
                .unwrap();
        },
        delete_s,
        true,
    );
}

#[test]
fn mergeable_list_revert_after_delete() {
    check(
        |d| {
            d.get_map("m")
                .ensure_mergeable_list("s")
                .unwrap()
                .push("keep")
                .unwrap();
        },
        delete_s,
        true,
    );
}

#[test]
fn mergeable_movable_list_revert_after_delete() {
    check(
        |d| {
            let l = d.get_map("m").ensure_mergeable_movable_list("s").unwrap();
            l.push("keep").unwrap();
            l.push("second").unwrap();
            l.mov(1, 0).unwrap();
        },
        delete_s,
        true,
    );
}

#[test]
fn mergeable_map_revert_after_delete() {
    check(
        |d| {
            let m = d.get_map("m").ensure_mergeable_map("s").unwrap();
            m.insert("k", "v").unwrap();
            let t = m.insert_container("t", loro::LoroText::new()).unwrap();
            t.insert(0, "inner").unwrap();
            let l = m.insert_container("l", loro::LoroList::new()).unwrap();
            l.push(1).unwrap();
        },
        delete_s,
        true,
    );
}

#[test]
fn mergeable_tree_revert_after_delete() {
    check(
        |d| {
            let tree = d.get_map("m").ensure_mergeable_tree("s").unwrap();
            let root = tree.create(TreeParentId::Root).unwrap();
            let child = tree.create(root).unwrap();
            tree.get_meta(root).unwrap().insert("name", "root").unwrap();
            let t = tree
                .get_meta(child)
                .unwrap()
                .insert_container("t", loro::LoroText::new())
                .unwrap();
            t.insert(0, "child text").unwrap();
        },
        delete_s,
        true,
    );
}

#[test]
fn nested_mergeable_revert_after_outer_delete() {
    check(
        |d| {
            let outer = d.get_map("m").ensure_mergeable_map("s").unwrap();
            outer
                .ensure_mergeable_text("inner")
                .unwrap()
                .insert(0, "hi")
                .unwrap();
            let deeper = outer.ensure_mergeable_map("deeper").unwrap();
            deeper
                .ensure_mergeable_list("items")
                .unwrap()
                .push("x")
                .unwrap();
        },
        delete_s,
        true,
    );
}

#[test]
fn nested_mergeable_revert_after_inner_delete() {
    check(
        |d| {
            let outer = d.get_map("m").ensure_mergeable_map("s").unwrap();
            outer
                .ensure_mergeable_text("inner")
                .unwrap()
                .insert(0, "hi")
                .unwrap();
        },
        |d| {
            let outer = d.get_map("m").ensure_mergeable_map("s").unwrap();
            outer.delete("inner").unwrap();
        },
        true,
    );
}

#[test]
fn mergeable_under_list_element_revert_after_delete() {
    check(
        |d| {
            let list = d.get_list("l");
            let m = list.insert_container(0, loro::LoroMap::new()).unwrap();
            m.ensure_mergeable_text("s")
                .unwrap()
                .insert(0, "hello")
                .unwrap();
        },
        |d| d.get_list("l").delete(0, 1).unwrap(),
        false,
    );
}

#[test]
fn mergeable_under_movable_list_element_revert_after_delete() {
    check(
        |d| {
            let list = d.get_movable_list("l");
            let m = list.insert_container(0, loro::LoroMap::new()).unwrap();
            m.ensure_mergeable_counter("s")
                .unwrap()
                .increment(3.0)
                .unwrap();
        },
        |d| d.get_movable_list("l").delete(0, 1).unwrap(),
        false,
    );
}

#[test]
fn mergeable_under_tree_node_revert_after_delete() {
    check(
        |d| {
            let tree = d.get_tree("tree");
            let node = tree.create(TreeParentId::Root).unwrap();
            tree.get_meta(node)
                .unwrap()
                .ensure_mergeable_text("s")
                .unwrap()
                .insert(0, "hello")
                .unwrap();
        },
        |d| {
            let tree = d.get_tree("tree");
            let node = tree.roots()[0];
            tree.delete(node).unwrap();
        },
        false,
    );
}

#[test]
fn mergeable_text_edited_before_delete() {
    check(
        |d| {
            d.get_map("m")
                .ensure_mergeable_text("s")
                .unwrap()
                .insert(0, "hello")
                .unwrap();
        },
        |d| {
            let t = d.get_map("m").ensure_mergeable_text("s").unwrap();
            t.insert(5, " world").unwrap();
            t.delete(0, 1).unwrap();
            delete_s(d);
        },
        // The hidden text differs from the target, so the first revert rewrites it and a
        // second revert re-syncs character identity like any text revert.
        false,
    );
}

#[test]
fn empty_mergeable_text_revert_after_edit_and_delete() {
    check(
        |d| {
            d.get_map("m").ensure_mergeable_text("s").unwrap();
        },
        |d| {
            d.get_map("m")
                .ensure_mergeable_text("s")
                .unwrap()
                .insert(0, "hi")
                .unwrap();
            delete_s(d);
        },
        true,
    );
}

#[test]
#[cfg(feature = "counter")]
fn mergeable_kind_change_revert() {
    check(
        |d| {
            d.get_map("m")
                .ensure_mergeable_text("s")
                .unwrap()
                .insert(0, "hello")
                .unwrap();
        },
        |d| {
            d.get_map("m")
                .ensure_mergeable_counter("s")
                .unwrap()
                .increment(3.0)
                .unwrap();
        },
        true,
    );
}

#[test]
fn mergeable_overwritten_by_value_revert() {
    check(
        |d| {
            d.get_map("m")
                .ensure_mergeable_list("s")
                .unwrap()
                .push("keep")
                .unwrap();
        },
        |d| d.get_map("m").insert("s", "plain").unwrap(),
        true,
    );
}

/// The exact reproduction from the loro-js cross-check.
#[test]
fn revert_restores_mergeable_text_once() {
    let d = doc();
    let m = d.get_map("m");
    m.ensure_mergeable_text("s")
        .unwrap()
        .insert(0, "hello")
        .unwrap();
    d.commit();
    let a: Frontiers = d.state_frontiers();
    m.delete("s").unwrap();
    d.commit();
    let before = d.len_ops();
    d.revert_to(&a).unwrap();
    d.commit();
    assert_eq!(d.get_deep_value(), loro::loro_value!({"m": {"s": "hello"}}),);
    // Only the parent marker is rewritten; unchanged hidden content is reused as-is.
    assert_eq!(d.len_ops() - before, 1);
}

fn peer(d: &LoroDoc, id: u64) -> LoroDoc {
    let f = d.fork();
    f.set_peer_id(id).unwrap();
    f
}

fn sync(a: &LoroDoc, b: &LoroDoc) {
    a.import(&b.export(loro::ExportMode::all_updates()).unwrap())
        .unwrap();
    b.import(&a.export(loro::ExportMode::all_updates()).unwrap())
        .unwrap();
}

/// A doc whose mergeable children under `m` were visible with `target` content, then got
/// `diverge` applied and their keys deleted. Returns the doc and the target version.
fn deleted_after_divergence(
    target: impl Fn(&LoroDoc),
    diverge: impl Fn(&LoroDoc),
) -> (LoroDoc, Frontiers, Value) {
    let d = doc();
    target(&d);
    d.commit();
    let a = d.state_frontiers();
    let expected = json(&d);
    diverge(&d);
    d.commit();
    let keys: Vec<String> = d.get_map("m").keys().map(|k| k.to_string()).collect();
    for k in keys {
        d.get_map("m").delete(&k).unwrap();
    }
    d.commit();
    (d, a, expected)
}

fn diverged_children() -> (LoroDoc, Frontiers, Value) {
    deleted_after_divergence(
        |d| {
            let m = d.get_map("m");
            m.ensure_mergeable_text("t")
                .unwrap()
                .insert(0, "hello")
                .unwrap();
            m.ensure_mergeable_list("l").unwrap().push("keep").unwrap();
            m.ensure_mergeable_movable_list("ml")
                .unwrap()
                .push("keep")
                .unwrap();
            m.ensure_mergeable_map("nested")
                .unwrap()
                .ensure_mergeable_text("t")
                .unwrap()
                .insert(0, "hello")
                .unwrap();
            let tree = m.ensure_mergeable_tree("tree").unwrap();
            let n = tree.create(TreeParentId::Root).unwrap();
            tree.get_meta(n).unwrap().insert("v", "keep").unwrap();
        },
        |d| {
            let m = d.get_map("m");
            m.ensure_mergeable_text("t")
                .unwrap()
                .insert(5, "!")
                .unwrap();
            m.ensure_mergeable_list("l").unwrap().push("extra").unwrap();
            m.ensure_mergeable_movable_list("ml")
                .unwrap()
                .push("extra")
                .unwrap();
            m.ensure_mergeable_map("nested")
                .unwrap()
                .ensure_mergeable_text("t")
                .unwrap()
                .insert(5, "!")
                .unwrap();
            m.ensure_mergeable_tree("tree")
                .unwrap()
                .create(TreeParentId::Root)
                .unwrap();
        },
    )
}

fn tree_ids(d: &LoroDoc) -> Vec<loro::TreeID> {
    let m = d.get_map("m");
    let tree = m.ensure_mergeable_tree("tree").unwrap();
    tree.nodes()
        .into_iter()
        .filter(|n| !tree.is_node_deleted(n).unwrap())
        .collect()
}

/// Revert rewrites hidden content that differs from the target with identity-preserving
/// edits: only `!` / `extra` / the extra tree node are removed.
#[test]
fn revert_diverged_hidden_children_keeps_identity() {
    let (d, a, expected) = diverged_children();
    let tree_before = d.fork();
    tree_before.checkout(&a).unwrap();
    let original_node = tree_ids(&tree_before);
    let before = d.len_ops();
    d.revert_to(&a).unwrap();
    d.commit();
    assert_replays_to(&d, &expected, "revert");
    assert_eq!(tree_ids(&d), original_node, "tree node identity is kept");
    // Markers (5), plus deleting "!" twice, "extra" twice and one tree node.
    assert_eq!(d.len_ops() - before, 10);
}

/// Local revert events forwarded to another doc through `apply_diff` reproduce the source
/// state, including hidden children whose content matched or differed from the target.
#[test]
fn revert_events_forward_through_apply_diff() {
    let (d, a, expected) = diverged_children();
    let source = d.fork();
    let mirror = d.fork();
    forward_events(&source, &mirror, |s| s.revert_to(&a).unwrap());
    assert_eq!(json(&source), expected);
    assert_eq!(json(&mirror), expected);
}

#[test]
#[cfg(feature = "counter")]
fn revert_counter_events_forward_through_apply_diff() {
    // Hidden value equal to the target (7) and larger than it (10).
    for extra in [0.0, 3.0] {
        let (d, a, expected) = deleted_after_divergence(
            |d| {
                d.get_map("m")
                    .ensure_mergeable_counter("c")
                    .unwrap()
                    .increment(7.0)
                    .unwrap();
            },
            |d| {
                if extra != 0.0 {
                    d.get_map("m")
                        .ensure_mergeable_counter("c")
                        .unwrap()
                        .increment(extra)
                        .unwrap();
                }
            },
        );
        assert_eq!(expected, json!({"m": {"c": 7.0}}));
        let source = d.fork();
        let mirror = d.fork();
        forward_events(&source, &mirror, |s| s.revert_to(&a).unwrap());
        assert_eq!(json(&source), expected, "source, hidden extra {extra}");
        assert_eq!(json(&mirror), expected, "mirror, hidden extra {extra}");
    }
}

/// Two peers revert the same deleted state concurrently. Identity-preserving edits make
/// the two reverts delete the same elements, so the merge equals the target.
#[test]
fn concurrent_reverts_of_diverged_hidden_children_converge_to_target() {
    let (a, target, expected) = diverged_children();
    let b = peer(&a, 2);
    a.revert_to(&target).unwrap();
    a.commit();
    b.revert_to(&target).unwrap();
    b.commit();
    sync(&a, &b);
    assert_replays_to(&a, &expected, "peer a");
    assert_replays_to(&b, &expected, "peer b");
    assert_eq!(tree_ids(&a).len(), 1);
}

/// A revert that keeps a tree node must keep its TreeID, so a concurrent edit to that node
/// on another peer survives the merge.
#[test]
fn revert_keeps_tree_node_for_concurrent_meta_edit() {
    let a = doc();
    let tree = a.get_map("m").ensure_mergeable_tree("tree").unwrap();
    let n = tree.create(TreeParentId::Root).unwrap();
    tree.get_meta(n).unwrap().insert("v", "keep").unwrap();
    a.commit();
    let target = a.state_frontiers();
    tree.create(TreeParentId::Root).unwrap();
    a.commit();
    let b = peer(&a, 2);

    a.get_map("m").delete("tree").unwrap();
    a.commit();
    a.revert_to(&target).unwrap();
    a.commit();
    b.get_map("m")
        .ensure_mergeable_tree("tree")
        .unwrap()
        .get_meta(n)
        .unwrap()
        .insert("remote", "REMOTE")
        .unwrap();
    b.commit();
    sync(&a, &b);

    for d in [&a, &b] {
        assert_eq!(tree_ids(d), vec![n]);
        let meta = d
            .get_map("m")
            .ensure_mergeable_tree("tree")
            .unwrap()
            .get_meta(n)
            .unwrap();
        assert_eq!(
            meta.get_deep_value().to_json_value(),
            json!({"v": "keep", "remote": "REMOTE"})
        );
    }
}

/// Undoing a local delete only restores the parent marker, so a remote edit made to the
/// hidden child survives regardless of whether it arrives before or after the undo.
#[test]
fn undo_delete_keeps_concurrent_remote_edit_in_either_order() {
    for import_before_undo in [true, false] {
        let a = doc();
        a.get_map("m")
            .ensure_mergeable_text("s")
            .unwrap()
            .insert(0, "hello")
            .unwrap();
        a.commit();
        let b = peer(&a, 2);
        let mut undo = UndoManager::new(&a);
        delete_s(&a);
        a.commit();
        b.get_map("m")
            .ensure_mergeable_text("s")
            .unwrap()
            .insert(5, "!")
            .unwrap();
        b.commit();
        let remote = b.export(loro::ExportMode::all_updates()).unwrap();
        if import_before_undo {
            a.import(&remote).unwrap();
            assert_eq!(json(&a), json!({}));
        }
        assert!(undo.undo().unwrap());
        a.commit();
        if !import_before_undo {
            a.import(&remote).unwrap();
        }
        assert_replays_to(
            &a,
            &json!({"m": {"s": "hello!"}}),
            &format!("undo, import_before_undo={import_before_undo}"),
        );
    }
}

/// `revert_to` targets a version: a remote edit already observed is reverted, one that
/// arrives afterwards merges, exactly as for a root text.
#[test]
fn revert_after_remote_edit_to_hidden_child_behaves_like_root_text() {
    for import_before_revert in [true, false] {
        let a = doc();
        a.get_map("m")
            .ensure_mergeable_text("s")
            .unwrap()
            .insert(0, "hello")
            .unwrap();
        a.commit();
        let target = a.state_frontiers();
        let b = peer(&a, 2);
        delete_s(&a);
        a.commit();
        b.get_map("m")
            .ensure_mergeable_text("s")
            .unwrap()
            .insert(5, "!")
            .unwrap();
        b.commit();
        let remote = b.export(loro::ExportMode::all_updates()).unwrap();
        if import_before_revert {
            a.import(&remote).unwrap();
        }
        a.revert_to(&target).unwrap();
        a.commit();
        if !import_before_revert {
            a.import(&remote).unwrap();
        }
        let expected = if import_before_revert {
            "hello"
        } else {
            "hello!"
        };
        assert_replays_to(&a, &json!({"m": {"s": expected}}), "revert");
    }
}

/// `diff` keeps reporting a re-activated mergeable child with its full state, so applying it
/// restores the target content on docs whose hidden state is absent, the same, or different.
#[test]
fn public_diff_restores_children_whatever_the_target_hidden_state() {
    let (d, target, expected) = diverged_children();
    let deleted = d.state_frontiers();
    let diff = || d.diff(&deleted, &target).unwrap();

    // Never saw the children.
    let absent = LoroDoc::new();
    absent.get_map("m");
    absent.apply_diff(diff()).unwrap();
    absent.commit();
    assert_eq!(json(&absent), expected, "absent hidden state");

    // Same hidden state.
    let same = d.fork();
    same.apply_diff(diff()).unwrap();
    same.commit();
    assert_replays_to(&same, &expected, "same hidden state");
    assert_eq!(tree_ids(&same).len(), 1);

    // Different hidden state at the same deterministic cids.
    let (other, _, _) = deleted_after_divergence(
        |d| {
            let m = d.get_map("m");
            m.ensure_mergeable_text("t")
                .unwrap()
                .insert(0, "xhellz")
                .unwrap();
            m.ensure_mergeable_list("l").unwrap().push("other").unwrap();
            m.ensure_mergeable_movable_list("ml")
                .unwrap()
                .push(1)
                .unwrap();
            m.ensure_mergeable_map("nested")
                .unwrap()
                .ensure_mergeable_text("t")
                .unwrap()
                .insert(0, "zz")
                .unwrap();
            let tree = m.ensure_mergeable_tree("tree").unwrap();
            tree.create(TreeParentId::Root).unwrap();
        },
        |_| {},
    );
    other.set_peer_id(3).unwrap();
    other.apply_diff(diff()).unwrap();
    other.commit();
    assert_replays_to(&other, &expected, "different hidden state");
}

#[test]
#[cfg(feature = "counter")]
fn public_diff_restores_counter_whatever_the_target_hidden_state() {
    for extra in [0.0, 3.0] {
        let (d, target, expected) = deleted_after_divergence(
            |d| {
                d.get_map("m")
                    .ensure_mergeable_counter("c")
                    .unwrap()
                    .increment(7.0)
                    .unwrap();
            },
            |d| {
                d.get_map("m")
                    .ensure_mergeable_counter("c")
                    .unwrap()
                    .increment(extra)
                    .unwrap();
            },
        );
        let diff = d.diff(&d.state_frontiers(), &target).unwrap();
        let absent = LoroDoc::new();
        absent.get_map("m");
        absent.apply_diff(diff.clone()).unwrap();
        absent.commit();
        assert_eq!(json(&absent), expected, "absent, extra {extra}");
        let same = d.fork();
        same.apply_diff(diff).unwrap();
        same.commit();
        assert_eq!(json(&same), expected, "same, extra {extra}");
    }
}

/// A local re-activation (`ensure_mergeable_*` over a deleted key) keeps main's event shape:
/// only the parent marker, plus the transaction's own ops on the child. Forwarded to a doc that
/// shares the hidden state, the default (incremental) `apply_diff` reproduces the source.
#[test]
fn local_reensure_event_keeps_main_shape() {
    let d = doc();
    let m = d.get_map("m");
    m.ensure_mergeable_text("s")
        .unwrap()
        .insert(0, "hello")
        .unwrap();
    m.ensure_mergeable_list("l").unwrap().push("keep").unwrap();
    #[cfg(feature = "counter")]
    m.ensure_mergeable_counter("c")
        .unwrap()
        .increment(7.0)
        .unwrap();
    d.commit();
    for key in ["s", "l", "c"] {
        m.delete(key).unwrap();
    }
    d.commit();
    let mirror = d.fork();
    let seen: Arc<Mutex<Vec<String>>> = Default::default();
    let sink = seen.clone();
    let sub = d.subscribe_root(Arc::new(move |e| {
        for ev in e.events {
            sink.lock().unwrap().push(format!("{:?}", ev.target));
        }
    }));
    forward_events(&d, &mirror, |d| {
        let m = d.get_map("m");
        m.ensure_mergeable_text("s").unwrap();
        #[cfg(feature = "counter")]
        m.ensure_mergeable_counter("c").unwrap();
        m.ensure_mergeable_list("l").unwrap().push("more").unwrap();
    });
    drop(sub);
    let mut expected = json!({"m": {"s": "hello", "l": ["keep", "more"]}});
    #[cfg(feature = "counter")]
    {
        expected["m"]["c"] = json!(7.0);
    }
    assert_eq!(json(&d), expected);
    assert_eq!(json(&mirror), expected);
    // The unchanged text and counter get no entry of their own; the list only its push.
    let seen = seen.lock().unwrap();
    assert!(
        seen.iter().all(|t| !t.contains(">s") && !t.contains(">c")),
        "{seen:?}"
    );
}

/// A re-activates a child that B edited while it was hidden. B's import event reports the
/// child's full state, but its content did not change, so B can still undo its own edit.
#[test]
fn peer_can_undo_own_edit_after_remote_undo_reactivates_child() {
    type Edit = fn(&LoroDoc);
    let cases: [(&str, Edit, Edit, Value, Value); 4] = [
        (
            "text",
            |d| {
                d.get_map("m")
                    .ensure_mergeable_text("s")
                    .unwrap()
                    .insert(0, "hello")
                    .unwrap();
            },
            |d| {
                d.get_map("m")
                    .ensure_mergeable_text("s")
                    .unwrap()
                    .insert(5, "!")
                    .unwrap();
            },
            json!({"m": {"s": "hello!"}}),
            json!({"m": {"s": "hello"}}),
        ),
        (
            "list",
            |d| {
                d.get_map("m")
                    .ensure_mergeable_list("s")
                    .unwrap()
                    .push("keep")
                    .unwrap();
            },
            |d| {
                d.get_map("m")
                    .ensure_mergeable_list("s")
                    .unwrap()
                    .push("mine")
                    .unwrap();
            },
            json!({"m": {"s": ["keep", "mine"]}}),
            json!({"m": {"s": ["keep"]}}),
        ),
        (
            "movable list",
            |d| {
                d.get_map("m")
                    .ensure_mergeable_movable_list("s")
                    .unwrap()
                    .push("keep")
                    .unwrap();
            },
            |d| {
                d.get_map("m")
                    .ensure_mergeable_movable_list("s")
                    .unwrap()
                    .push("mine")
                    .unwrap();
            },
            json!({"m": {"s": ["keep", "mine"]}}),
            json!({"m": {"s": ["keep"]}}),
        ),
        (
            "nested text",
            |d| {
                d.get_map("m")
                    .ensure_mergeable_map("s")
                    .unwrap()
                    .ensure_mergeable_text("t")
                    .unwrap()
                    .insert(0, "hello")
                    .unwrap();
            },
            |d| {
                d.get_map("m")
                    .ensure_mergeable_map("s")
                    .unwrap()
                    .ensure_mergeable_text("t")
                    .unwrap()
                    .insert(5, "!")
                    .unwrap();
            },
            json!({"m": {"s": {"t": "hello!"}}}),
            json!({"m": {"s": {"t": "hello"}}}),
        ),
    ];
    for (name, setup, edit, merged, after_undo) in cases {
        for delete_arrives_first in [false, true] {
            let a = doc();
            setup(&a);
            a.commit();
            let b = peer(&a, 2);
            let mut ua = UndoManager::new(&a);
            let mut ub = UndoManager::new(&b);
            delete_s(&a);
            a.commit();
            edit(&b);
            b.commit();
            if delete_arrives_first {
                b.import(&a.export(loro::ExportMode::all_updates()).unwrap())
                    .unwrap();
            }
            a.import(&b.export(loro::ExportMode::all_updates()).unwrap())
                .unwrap();
            assert!(ua.undo().unwrap(), "{name}: A undoes its delete");
            a.commit();
            b.import(&a.export(loro::ExportMode::all_updates()).unwrap())
                .unwrap();
            let what = format!("{name}, delete_arrives_first={delete_arrives_first}");
            assert_eq!(json(&a), merged, "{what}: A");
            assert_eq!(json(&b), merged, "{what}: B");
            assert!(ub.can_undo(), "{what}");
            assert!(ub.undo().unwrap(), "{what}: B undoes its edit");
            b.commit();
            assert_replays_to(&b, &after_undo, &what);
            sync(&a, &b);
            assert_eq!(json(&a), after_undo, "{what}: A after sync");
        }
    }
}

/// Container children of a mergeable (movable) list, reordered/removed/added after the target
/// version and then hidden. The full-state `diff` restores them on docs with the same hidden
/// state, none, or a different one: movable-list children that exist in both are moved and keep
/// their content (no loss, no duplication).
#[test]
fn public_diff_restores_reordered_container_children() {
    type Mutation = fn(&LoroDoc, bool);
    fn children(d: &LoroDoc, movable: bool) -> usize {
        let m = d.get_map("m");
        if movable {
            m.ensure_mergeable_movable_list("s").unwrap().len()
        } else {
            m.ensure_mergeable_list("s").unwrap().len()
        }
    }
    let mutations: [(&str, Mutation); 5] = [
        ("swap first two", |d, movable| {
            if movable {
                d.get_map("m")
                    .ensure_mergeable_movable_list("s")
                    .unwrap()
                    .mov(0, 1)
                    .unwrap();
            } else {
                d.get_map("m")
                    .ensure_mergeable_list("s")
                    .unwrap()
                    .delete(0, 1)
                    .unwrap();
            }
        }),
        ("rotate", |d, movable| {
            if movable {
                d.get_map("m")
                    .ensure_mergeable_movable_list("s")
                    .unwrap()
                    .mov(2, 0)
                    .unwrap();
            }
        }),
        ("reverse", |d, movable| {
            if movable {
                let l = d.get_map("m").ensure_mergeable_movable_list("s").unwrap();
                l.mov(2, 0).unwrap();
                l.mov(2, 1).unwrap();
            }
        }),
        ("delete middle", |d, movable| {
            let m = d.get_map("m");
            if movable {
                m.ensure_mergeable_movable_list("s")
                    .unwrap()
                    .delete(1, 1)
                    .unwrap();
            } else {
                m.ensure_mergeable_list("s").unwrap().delete(1, 1).unwrap();
            }
        }),
        ("insert new child", |d, movable| {
            let m = d.get_map("m");
            if movable {
                m.ensure_mergeable_movable_list("s")
                    .unwrap()
                    .insert_container(1, loro::LoroText::new())
                    .unwrap()
                    .insert(0, "new")
                    .unwrap();
            } else {
                m.ensure_mergeable_list("s")
                    .unwrap()
                    .insert_container(1, loro::LoroText::new())
                    .unwrap()
                    .insert(0, "new")
                    .unwrap();
            }
        }),
    ];
    for movable in [true, false] {
        for texts in [false, true] {
            for (name, mutate) in mutations {
                let setup = |d: &LoroDoc| {
                    let m = d.get_map("m");
                    for (i, v) in ["A", "B", "C"].into_iter().enumerate() {
                        if texts {
                            let t = if movable {
                                m.ensure_mergeable_movable_list("s")
                                    .unwrap()
                                    .insert_container(i, loro::LoroText::new())
                                    .unwrap()
                            } else {
                                m.ensure_mergeable_list("s")
                                    .unwrap()
                                    .insert_container(i, loro::LoroText::new())
                                    .unwrap()
                            };
                            t.insert(0, v).unwrap();
                        } else {
                            let c = if movable {
                                m.ensure_mergeable_movable_list("s")
                                    .unwrap()
                                    .insert_container(i, loro::LoroMap::new())
                                    .unwrap()
                            } else {
                                m.ensure_mergeable_list("s")
                                    .unwrap()
                                    .insert_container(i, loro::LoroMap::new())
                                    .unwrap()
                            };
                            c.insert("v", v).unwrap();
                        }
                    }
                };
                let (d, target, expected) = deleted_after_divergence(setup, |d| mutate(d, movable));
                let what = format!("movable={movable} texts={texts} {name}");
                let diff = || d.diff(&d.state_frontiers(), &target).unwrap();

                let same = d.fork();
                same.apply_diff(diff()).unwrap();
                same.commit();
                assert_replays_to(&same, &expected, &format!("{what}: same hidden"));
                assert_eq!(children(&same, movable), 3, "{what}");

                let absent = LoroDoc::new();
                absent.get_map("m");
                absent.apply_diff(diff()).unwrap();
                absent.commit();
                assert_eq!(json(&absent), expected, "{what}: absent hidden");

                let (other, _, _) = deleted_after_divergence(
                    |d| {
                        let m = d.get_map("m");
                        if movable {
                            m.ensure_mergeable_movable_list("s")
                                .unwrap()
                                .push(1)
                                .unwrap();
                        } else {
                            m.ensure_mergeable_list("s").unwrap().push(1).unwrap();
                        }
                    },
                    |_| {},
                );
                other.set_peer_id(3).unwrap();
                other.apply_diff(diff()).unwrap();
                other.commit();
                assert_replays_to(&other, &expected, &format!("{what}: different hidden"));

                let r = d.fork();
                r.revert_to(&target).unwrap();
                r.commit();
                assert_replays_to(&r, &expected, &format!("{what}: revert_to"));
            }
        }
    }
}

/// The default `apply_diff` is incremental, as on main: a full-state batch applied without the
/// flag to a doc that keeps the child's hidden state appends to it. `LoroDoc::diff` sets the
/// flag, so this only happens to batches whose flag was cleared (or rebuilt from JS without
/// `{ fullState: true }`).
#[test]
fn default_apply_diff_is_incremental() {
    let d = doc();
    d.get_map("m")
        .ensure_mergeable_text("s")
        .unwrap()
        .insert(0, "hello")
        .unwrap();
    d.commit();
    let target = d.state_frontiers();
    delete_s(&d);
    d.commit();
    let mut diff = d.diff(&d.state_frontiers(), &target).unwrap();
    assert!(diff.is_full_state());
    diff.set_full_state(false);
    d.apply_diff(diff).unwrap();
    d.commit();
    assert_eq!(json(&d), json!({"m": {"s": "hellohello"}}));
}

/// Aligning text compares content and styles separately: kept chars whose style differs are
/// re-marked, not deleted and re-inserted, so a concurrent edit to them survives.
#[test]
fn public_diff_restyles_kept_chars() {
    let d = doc();
    let t = d.get_map("m").ensure_mergeable_text("s").unwrap();
    t.insert(0, "hello").unwrap();
    d.commit();
    let b = peer(&d, 3);
    t.mark(0..5, "bold", true).unwrap();
    d.commit();
    let bold = d.state_frontiers();
    t.unmark(0..5, "bold").unwrap();
    d.commit();
    delete_s(&d);
    d.commit();

    let r = d.fork();
    r.set_peer_id(4).unwrap();
    let before = r.len_ops();
    r.apply_diff(d.diff(&d.state_frontiers(), &bold).unwrap())
        .unwrap();
    r.commit();
    let restored = r.get_map("m").ensure_mergeable_text("s").unwrap();
    assert_eq!(
        restored.get_richtext_value().to_json_value(),
        json!([{"insert": "hello", "attributes": {"bold": true}}])
    );
    // The marker and the mark: no text is rewritten.
    assert!(r.len_ops() - before <= 3, "{} ops", r.len_ops() - before);

    // B concurrently replaces the old "h" with "H".
    let bt = b.get_map("m").ensure_mergeable_text("s").unwrap();
    bt.delete(0, 1).unwrap();
    bt.insert(0, "H").unwrap();
    b.commit();
    sync(&r, &b);
    assert_eq!(restored.to_string(), "Hello");
}

/// Committing many new mergeable children with a subscriber attached stays linear. A previous
/// revision rebuilt local events per re-activated child and scanned the whole transaction for
/// each (20k children: ~50 ms -> ~1.3-2.2 s).
///
/// `cargo test -p loro --test mergeable_revert perf_commit_many_mergeable_children -- --ignored --nocapture`
#[test]
#[ignore]
fn perf_commit_many_mergeable_children_with_subscriber() {
    for n in [5_000usize, 20_000] {
        let d = doc();
        let _sub = d.subscribe_root(Arc::new(|_| {}));
        let m = d.get_map("m");
        for i in 0..n {
            m.ensure_mergeable_text(&i.to_string())
                .unwrap()
                .insert(0, "x")
                .unwrap();
        }
        let start = std::time::Instant::now();
        d.commit();
        println!("{n} children: commit {:?}", start.elapsed());
    }
}

/// Text alignment with multi-byte chars (emoji, combining marks, CJK) and styles, against the
/// same hidden text, a different one, and none.
#[test]
fn public_diff_aligns_non_ascii_styled_text() {
    let cases: [(&str, &str); 6] = [
        ("a😀b", "a😀c"),
        ("😀😀", "😀😃"),
        ("é中文x", "é中文"),
        ("👍🏽ok", "👍🏾ok"),
        ("", "😀"),
        ("😀", ""),
    ];
    for (target_text, hidden_text) in cases {
        let d = doc();
        let t = d.get_map("m").ensure_mergeable_text("s").unwrap();
        t.insert(0, target_text).unwrap();
        if !target_text.is_empty() {
            t.mark(0..1, "bold", true).unwrap();
        }
        d.commit();
        let target = d.state_frontiers();
        let expected = t.get_richtext_value().to_json_value();
        t.delete(0, t.len_unicode()).unwrap();
        t.insert(0, hidden_text).unwrap();
        d.commit();
        delete_s(&d);
        d.commit();
        let diff = || d.diff(&d.state_frontiers(), &target).unwrap();
        let what = format!("{target_text:?} over {hidden_text:?}");

        let same = d.fork();
        same.apply_diff(diff()).unwrap();
        same.commit();
        let (other, _, _) = deleted_after_divergence(
            |d| {
                d.get_map("m")
                    .ensure_mergeable_text("s")
                    .unwrap()
                    .insert(0, "x😀y")
                    .unwrap();
            },
            |_| {},
        );
        other.config_default_text_style(Some(StyleConfig {
            expand: ExpandType::After,
        }));
        other.apply_diff(diff()).unwrap();
        other.commit();
        let absent = doc();
        absent.get_map("m");
        absent.apply_diff(diff()).unwrap();
        absent.commit();
        for (name, r) in [("same", &same), ("different", &other), ("absent", &absent)] {
            let text = r.get_map("m").ensure_mergeable_text("s").unwrap();
            assert_eq!(
                text.get_richtext_value().to_json_value(),
                expected,
                "{what}: {name}"
            );
        }
    }
}

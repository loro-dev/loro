//! Reverting to a version where a mergeable child was visible must restore the child's
//! content exactly once.
//!
//! A mergeable child keeps its state at a deterministic cid while the parent marker is
//! gone. The diff that re-activates it carries the child's *full* target state (the same
//! "revival" shape events use), so `apply_diff` has to set the hidden child to that
//! state instead of appending it. See `context/mergeable-containers.md`.

use loro::{ExpandType, Frontiers, LoroDoc, StyleConfig, ToJson, TreeParentId, UndoManager};
use serde_json::Value;

fn doc() -> LoroDoc {
    let d = LoroDoc::new();
    d.set_peer_id(1).unwrap();
    d.config_default_text_style(Some(StyleConfig {
        expand: ExpandType::After,
    }));
    d
}

/// Deep JSON with tree node ids stripped: reviving a deleted regular tree node through
/// `apply_diff` creates a fresh node id, which is unrelated to this bug.
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

/// Undoing a local delete while a remote peer edited the (now hidden) child must not
/// duplicate content. The revived child is set to its state at the undone version, so the
/// concurrent remote edit is dropped, the same as for a revived regular container, whose
/// concurrent edits stay in the dead original.
#[test]
fn undo_delete_with_concurrent_remote_edit_does_not_duplicate() {
    let a = doc();
    let b = LoroDoc::new();
    b.set_peer_id(2).unwrap();
    a.get_map("m")
        .ensure_mergeable_text("s")
        .unwrap()
        .insert(0, "hello")
        .unwrap();
    a.commit();
    b.import(&a.export(loro::ExportMode::all_updates()).unwrap())
        .unwrap();
    let mut undo = UndoManager::new(&a);
    delete_s(&a);
    a.commit();
    b.get_map("m")
        .ensure_mergeable_text("s")
        .unwrap()
        .insert(5, "!")
        .unwrap();
    b.commit();
    a.import(&b.export(loro::ExportMode::all_updates()).unwrap())
        .unwrap();
    assert_eq!(json(&a), serde_json::json!({"m": {}}));
    assert!(undo.undo().unwrap());
    a.commit();
    assert_replays_to(&a, &serde_json::json!({"m": {"s": "hello"}}), "undo");
}

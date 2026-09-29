//! `apply_diff`, `revert_to` and undo must either succeed or return `Err`
//! without changing the doc when they meet containers of an unknown type
//! (written by a newer Loro). The pre-check in `_apply_diff` has to predict
//! what the apply loop does to recreated, revived and removed containers.

use loro::{
    event::{Diff, DiffBatch, ListDiffItem, MapDelta},
    ContainerID, ContainerTrait, ContainerType, Frontiers, LoroDoc, LoroList, LoroMap, LoroResult,
    LoroValue, UndoManager, ValueOrContainer,
};
use rustc_hash::FxHashMap;
use std::borrow::Cow;

mod unknown_container_support;
use unknown_container_support::*;

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
    out.push_str("} as const;\n\n");

    // Reproductions from the second review of #1142
    let docs: [(&str, &str, fn(&LoroDoc)); 4] = [
        (
            "recreatedParent",
            "normal map `r.m` holding a mergeable list `k` = [U, 1]",
            |doc| {
                let m = doc
                    .get_map("r")
                    .insert_container("m", LoroMap::new())
                    .unwrap();
                let k = m.ensure_mergeable_list("k").unwrap();
                edited_counter(k.insert_container(0, counter()).unwrap());
                k.push(1).unwrap();
            },
        ),
        (
            "hiddenMergeableChild",
            "mergeable map `m.s` holding a list `l` = [U, 1]",
            |doc| {
                let s = doc.get_map("m").ensure_mergeable_map("s").unwrap();
                let l = s.insert_container("l", LoroList::new()).unwrap();
                edited_counter(l.insert_container(0, counter()).unwrap());
                l.push(1).unwrap();
            },
        ),
        ("unknownList", "list `us` = [U]", |doc| {
            edited_counter(doc.get_list("us").push_container(counter()).unwrap());
        }),
        (
            "recreatedHost",
            "list `L` = [map { v: 1, k: mergeable movable list [U, \"x\"] }]",
            |doc| {
                let p = doc
                    .get_list("L")
                    .insert_container(0, LoroMap::new())
                    .unwrap();
                p.insert("v", 1).unwrap();
                let k = p.ensure_mergeable_movable_list("k").unwrap();
                k.push("x").unwrap();
                edited_counter(k.insert_container(0, counter()).unwrap());
            },
        ),
    ];
    out.push_str("export const UNKNOWN_REVIEW_DOCS = {\n");
    for (name, what, build) in docs {
        let bytes = forge(build)
            .export(loro::ExportMode::all_updates())
            .unwrap();
        let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
        out.push_str(&format!("  // {what}\n  {name}:\n    \"{b64}\",\n"));
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

// Reproductions from the second review of #1142.

/// `f` fails on an unknown container and leaves `doc` unchanged.
fn assert_rejected_atomically<T: std::fmt::Debug>(
    doc: &LoroDoc,
    f: impl FnOnce(&LoroDoc) -> LoroResult<T>,
) {
    let before = state(doc);
    let err = f(doc).unwrap_err();
    assert!(is_unknown_err(&err), "{err:?}");
    assert_eq!(state(doc), before);
}

/// `batch` with the MovableList inserts marked as moves.
fn as_moves(batch: &DiffBatch) -> DiffBatch {
    let mut ans = DiffBatch::default();
    for (id, diff) in batch.iter() {
        let diff = match diff {
            Diff::List(items) if id.container_type() == ContainerType::MovableList => Diff::List(
                items
                    .iter()
                    .map(|item| match item {
                        ListDiffItem::Insert { insert, .. } => ListDiffItem::Insert {
                            insert: insert.clone(),
                            is_move: true,
                        },
                        item => item.clone(),
                    })
                    .collect(),
            ),
            diff => diff.clone(),
        };
        ans.push(id.clone(), diff).unwrap();
    }
    ans.set_full_state(batch.is_full_state());
    ans
}

/// Recreating a container recreates its mergeable children under new ids.
/// Aligning such a child's full state with the new, empty child can't keep
/// U, even though the doc kept U in the old child. Used to write the parent
/// before failing.
#[test]
fn revival_under_a_recreated_parent_is_rejected_atomically() {
    for host in ["map", "list", "tree"] {
        let doc = forge(|doc| {
            let parent = match host {
                "map" => doc
                    .get_map("r")
                    .insert_container("m", LoroMap::new())
                    .unwrap(),
                "list" => doc
                    .get_list("l")
                    .insert_container(0, LoroMap::new())
                    .unwrap(),
                _ => {
                    let tree = doc.get_tree("tree");
                    let meta = tree.get_meta(tree.create(None).unwrap()).unwrap();
                    meta.insert("a", 1).unwrap();
                    meta
                }
            };
            let k = parent.ensure_mergeable_movable_list("k").unwrap();
            k.push("a").unwrap();
            edited_counter(k.insert_container(1, counter()).unwrap());
        });
        let v0 = doc.state_frontiers();
        match host {
            "map" => doc.get_map("r").delete("m").unwrap(),
            "list" => doc.get_list("l").delete(0, 1).unwrap(),
            _ => {
                let tree = doc.get_tree("tree");
                tree.delete(tree.roots()[0]).unwrap();
            }
        }
        doc.commit();
        let v1 = doc.state_frontiers();

        for full_state in [true, false] {
            let mut batch = doc.diff(&v1, &v0).unwrap();
            batch.set_full_state(full_state);
            // Crafted: the child's entries marked as moves
            let moves = as_moves(&batch);
            assert_rejected_atomically(&doc, |d| d.apply_diff(batch));
            assert_rejected_atomically(&doc, |d| d.apply_diff(moves));
        }
        assert_rejected_atomically(&doc, |d| d.revert_to(&v0));
    }
}

/// One step deletes U inside a regular child of a mergeable container (or
/// its tree node meta) and hides the container. Undoing it revives the
/// container first, so the child is applied and would have to recreate U.
/// Undo used to return `Ok(true)` with U silently gone.
#[test]
fn undo_that_revives_a_mergeable_parent_of_a_deleted_unknown_is_rejected() {
    for tree in [false, true] {
        let doc = forge(|doc| {
            let m = doc.get_map("m");
            if tree {
                let st = m.ensure_mergeable_tree("st").unwrap();
                let meta = st.get_meta(st.create(None).unwrap()).unwrap();
                edited_counter(meta.insert_container("u", counter()).unwrap());
                meta.insert("a", 1).unwrap();
            } else {
                let s = m.ensure_mergeable_map("s").unwrap();
                let l = s.insert_container("l", LoroList::new()).unwrap();
                edited_counter(l.insert_container(0, counter()).unwrap());
                l.push(1).unwrap();
            }
        });
        let v0 = doc.state_frontiers();
        let mut undo = UndoManager::new(&doc);
        let m = doc.get_map("m");
        if tree {
            let st = m.ensure_mergeable_tree("st").unwrap();
            st.get_meta(st.roots()[0]).unwrap().delete("u").unwrap();
        } else {
            let s = m.ensure_mergeable_map("s").unwrap();
            let l = s.get("l").unwrap().into_container().unwrap();
            l.into_list().unwrap().delete(0, 1).unwrap();
        }
        doc.get_text("t").insert(0, "x").unwrap();
        m.delete(if tree { "st" } else { "s" }).unwrap();
        doc.commit();
        let v1 = doc.state_frontiers();

        assert_rejected_atomically(&doc, |_| undo.undo());
        assert_rejected_atomically(&doc, |d| d.revert_to(&v0));
        for full_state in [true, false] {
            let mut batch = doc.diff(&v1, &v0).unwrap();
            batch.set_full_state(full_state);
            assert_rejected_atomically(&doc, |d| d.apply_diff(batch));
        }
    }
}

/// A rejected undo step stays in the doc. The steps before it are rebased
/// over its changes, like over remote ones. Used to undo `b` instead of `a`.
#[test]
fn undo_after_a_rejected_step_undoes_the_right_edits() {
    for b_in_front in [false, true] {
        let doc = forge(|doc| {
            let list = doc
                .get_map("m")
                .insert_container("k", LoroList::new())
                .unwrap();
            edited_counter(list.insert_container(0, counter()).unwrap());
            list.push(1).unwrap();
        });
        let mut undo = UndoManager::new(&doc);
        let t = doc.get_text("t");
        t.insert(0, "a").unwrap();
        doc.commit();
        t.insert(if b_in_front { 0 } else { 1 }, "b").unwrap();
        doc.get_map("m").delete("k").unwrap();
        doc.commit();

        assert_rejected_atomically(&doc, |_| undo.undo());
        assert!(undo.undo().unwrap());
        assert_eq!(t.to_string(), "b");
        assert!(undo.redo().unwrap());
        assert_eq!(t.to_string(), if b_in_front { "ba" } else { "ab" });
    }
}

/// The same with a remote edit after the steps: undoing "A" used to be out
/// of bounds, swallowed, and the step lost.
#[test]
fn undo_after_a_rejected_step_with_remote_edits() {
    let doc = forge(|doc| {
        let list = doc
            .get_map("m")
            .insert_container("k", LoroList::new())
            .unwrap();
        edited_counter(list.insert_container(0, counter()).unwrap());
        list.push(1).unwrap();
        doc.get_text("t").insert(0, "0123").unwrap();
    });
    let mut undo = UndoManager::new(&doc);
    let t = doc.get_text("t");
    t.insert(4, "A").unwrap();
    doc.commit();
    t.delete(0, 2).unwrap();
    doc.get_map("m").delete("k").unwrap();
    doc.commit();
    let remote = doc.fork();
    remote.set_peer_id(77).unwrap();
    remote.get_text("t").insert(3, "R").unwrap();
    remote.commit();
    doc.import(
        &remote
            .export(loro::ExportMode::updates(&doc.oplog_vv()))
            .unwrap(),
    )
    .unwrap();
    assert_eq!(t.to_string(), "23AR");

    assert_rejected_atomically(&doc, |_| undo.undo());
    assert!(undo.undo().unwrap());
    assert_eq!(t.to_string(), "23R");
}

/// Redo mirrors undo: a rejected redo step (its U was deleted remotely) must
/// not make the next redo miss its edit. Used to leave "a" instead of "ac".
#[test]
fn redo_after_a_rejected_redo_step() {
    let doc = forge(|doc| {
        let ml = doc.get_movable_list("ml");
        edited_counter(ml.push_container(counter()).unwrap());
        ml.push("x").unwrap();
        ml.push("y").unwrap();
    });
    let mut undo = UndoManager::new(&doc);
    let t = doc.get_text("t");
    t.insert(0, "a").unwrap();
    doc.commit();
    t.insert(1, "b").unwrap();
    doc.get_movable_list("ml").mov(0, 2).unwrap();
    doc.commit();
    t.insert(2, "c").unwrap();
    doc.commit();
    for _ in 0..3 {
        assert!(undo.undo().unwrap());
    }
    assert_eq!(t.to_string(), "");
    let remote = doc.fork();
    remote.set_peer_id(77).unwrap();
    remote.get_movable_list("ml").delete(0, 1).unwrap();
    remote.commit();
    doc.import(
        &remote
            .export(loro::ExportMode::updates(&doc.oplog_vv()))
            .unwrap(),
    )
    .unwrap();

    assert!(undo.redo().unwrap());
    assert_eq!(t.to_string(), "a");
    assert_rejected_atomically(&doc, |_| undo.redo());
    assert!(undo.redo().unwrap());
    assert_eq!(t.to_string(), "ac");
}

// Reproductions from the third review of #1142.

/// Where a mergeable movable list `k = [U, "x"]` sits: in a map in a list, in
/// a tree node's meta, or one level deeper in a mergeable map of that map.
#[derive(Clone, Copy, Debug)]
enum RemappedHost {
    ListElement,
    TreeMeta,
    Nested,
}

/// One step moves U inside `k` and hides `k`; the next deletes the host. Undo
/// recreates the host under a new id (the undo manager remaps it), and a
/// remote peer fills the new host's `k`. Undoing the first step writes `k`'s
/// marker on the new host, where U is not: it would create U. The check used
/// to look into the old hidden `k`, accept the move, and undo returned
/// `Ok(true)` half applied.
#[test]
fn undo_into_a_host_the_undo_manager_recreated_is_rejected() {
    for host in [
        RemappedHost::ListElement,
        RemappedHost::TreeMeta,
        RemappedHost::Nested,
    ] {
        for remote_fill in [true, false] {
            let doc = forge(|doc| {
                let h = match host {
                    RemappedHost::TreeMeta => {
                        let t = doc.get_tree("T");
                        t.get_meta(t.create(None).unwrap()).unwrap()
                    }
                    _ => doc
                        .get_list("L")
                        .insert_container(0, LoroMap::new())
                        .unwrap(),
                };
                h.insert("v", 1).unwrap();
                let h = match host {
                    RemappedHost::Nested => h.ensure_mergeable_map("s").unwrap(),
                    _ => h,
                };
                let k = h.ensure_mergeable_movable_list("k").unwrap();
                k.push("x").unwrap();
                edited_counter(k.insert_container(0, counter()).unwrap());
            });
            let holder = |doc: &LoroDoc| -> LoroMap {
                let h = match host {
                    RemappedHost::TreeMeta => {
                        let t = doc.get_tree("T");
                        t.get_meta(t.roots()[0]).unwrap()
                    }
                    _ => doc
                        .get_list("L")
                        .get(0)
                        .unwrap()
                        .into_container()
                        .unwrap()
                        .into_map()
                        .unwrap(),
                };
                match host {
                    RemappedHost::Nested => h.ensure_mergeable_map("s").unwrap(),
                    _ => h,
                }
            };
            let mut undo = UndoManager::new(&doc);
            let h = holder(&doc);
            h.ensure_mergeable_movable_list("k")
                .unwrap()
                .mov(0, 1)
                .unwrap();
            h.delete("k").unwrap();
            doc.commit();
            match host {
                RemappedHost::TreeMeta => {
                    let t = doc.get_tree("T");
                    t.delete(t.roots()[0]).unwrap();
                }
                _ => doc.get_list("L").delete(0, 1).unwrap(),
            }
            doc.commit();
            assert!(undo.undo().unwrap());
            if remote_fill {
                let remote = doc.fork();
                remote.set_peer_id(3).unwrap();
                let k = holder(&remote).ensure_mergeable_movable_list("k").unwrap();
                k.push("y").unwrap();
                k.push("z").unwrap();
                remote.commit();
                doc.import(
                    &remote
                        .export(loro::ExportMode::updates(&doc.oplog_vv()))
                        .unwrap(),
                )
                .unwrap();
            }
            assert_rejected_atomically(&doc, |_| undo.undo());
        }
    }
}

/// A Map entry that writes the mergeable child of key `k` under key `q`
/// (only a hand-built batch does this), then moves U inside it. The loop
/// writes `q`'s marker, which isn't where U is. Used to write the marker (or
/// clear `q` with a full state) before failing.
#[test]
fn mergeable_value_under_another_key_is_rejected_atomically() {
    for full_state in [false, true] {
        let doc = forge(|doc| {
            let a = doc.get_map("A");
            let k = a.ensure_mergeable_movable_list("k").unwrap();
            k.push("x").unwrap();
            edited_counter(k.insert_container(0, counter()).unwrap());
            let q = a.ensure_mergeable_movable_list("q").unwrap();
            q.push("y").unwrap();
            q.push("z").unwrap();
        });
        let a = doc.get_map("A");
        let k = a.ensure_mergeable_movable_list("k").unwrap();
        let u = k.get(0).unwrap().into_container().unwrap();
        let moved_u = if full_state {
            vec![ListDiffItem::Insert {
                insert: vec![
                    ValueOrContainer::Container(u),
                    ValueOrContainer::Value("x".into()),
                ],
                is_move: true,
            }]
        } else {
            vec![
                ListDiffItem::Retain { retain: 1 },
                ListDiffItem::Insert {
                    insert: vec![ValueOrContainer::Container(u)],
                    is_move: true,
                },
            ]
        };
        let mut diff = batch(vec![
            (
                a.id(),
                Diff::Map(MapDelta {
                    updated: FxHashMap::from_iter([(
                        Cow::Borrowed("q"),
                        Some(ValueOrContainer::Container(loro::Container::MovableList(
                            k.clone(),
                        ))),
                    )]),
                }),
            ),
            (k.id(), Diff::List(moved_u)),
        ]);
        diff.set_full_state(full_state);
        assert_rejected_atomically(&doc, |d| d.apply_diff(diff));
    }
}

/// A full-state batch in which the entry of a hidden mergeable map `a.b`
/// comes before the entry that revives it (JS `applyDiff` takes any order).
/// The loop skips `b`'s entry, so `b.ml` is not aligned but applied as is,
/// creating U. The plan used to count `b`'s marker writes; the batch applied
/// half before failing.
#[test]
fn reordered_full_state_revival_is_rejected_atomically() {
    let doc = forge(|doc| {
        let a = doc.get_map("r").ensure_mergeable_map("a").unwrap();
        let b = a.ensure_mergeable_map("b").unwrap();
        b.insert("v", 1).unwrap();
        let ml = b.ensure_mergeable_movable_list("ml").unwrap();
        ml.push("a").unwrap();
        edited_counter(ml.insert_container(0, counter()).unwrap());
    });
    let v0 = doc.state_frontiers();
    let expected = state(&doc).0;
    let a = doc.get_map("r").ensure_mergeable_map("a").unwrap();
    a.delete("b").unwrap();
    doc.commit();
    let natural = doc.diff(&doc.state_frontiers(), &v0).unwrap();
    let entries: Vec<_> = natural
        .iter()
        .map(|(id, d)| (id.clone(), d.clone()))
        .collect();
    let a_id = a.id();
    let b_id = ContainerID::new_mergeable(&a_id, "b", ContainerType::Map);
    let pos = |id: &ContainerID| entries.iter().position(|(c, _)| c == id).unwrap();
    assert!(pos(&a_id) < pos(&b_id));

    // `b` before `a`
    let mut reordered = entries.clone();
    reordered.swap(pos(&a_id), pos(&b_id));
    let mut reordered = batch(reordered);
    reordered.set_full_state(true);
    assert_rejected_atomically(&doc, |d| d.apply_diff(reordered));

    // The natural order keeps U
    doc.apply_diff(natural).unwrap();
    assert_eq!(state(&doc).0, expected);
}

/// The steps before a rejected step are rebased over its changes like over
/// remote changes, including their limits: if the rejected step moved an
/// element an earlier step inserted, undoing that earlier step no longer
/// finds it (an undo of a no-op falls through to the step before). Undo then
/// behaves exactly as if a remote peer had made the move.
#[test]
fn undo_after_a_rejected_move_matches_a_remote_move() {
    let undo_all = |rejected_local_move: bool| -> Vec<(bool, serde_json::Value)> {
        let doc = forge(|doc| {
            edited_counter(doc.get_list("us").push_container(counter()).unwrap());
        });
        let remote = doc.fork();
        remote.set_peer_id(3).unwrap();
        let mut undo = UndoManager::new(&doc);
        let ml = doc.get_movable_list("ml");
        ml.insert(0, "x").unwrap();
        ml.insert(1, "y").unwrap();
        doc.commit();
        ml.insert(0, "L1").unwrap();
        doc.commit();
        if rejected_local_move {
            ml.mov(0, 2).unwrap();
            doc.get_list("us").delete(0, 1).unwrap();
            doc.commit();
            assert_rejected_atomically(&doc, |_| undo.undo());
        } else {
            let sync = |a: &LoroDoc, b: &LoroDoc| {
                b.import(&a.export(loro::ExportMode::updates(&b.oplog_vv())).unwrap())
                    .unwrap();
            };
            sync(&doc, &remote);
            remote.get_movable_list("ml").mov(0, 2).unwrap();
            remote.commit();
            sync(&remote, &doc);
        }
        (0..3)
            .map(|_| {
                let r = undo.undo().unwrap();
                (r, loro::ToJson::to_json_value(&ml.get_value()))
            })
            .collect()
    };
    assert_eq!(undo_all(true), undo_all(false));
}

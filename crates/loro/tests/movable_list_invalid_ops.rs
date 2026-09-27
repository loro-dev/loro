//! Forged or corrupted movable-list `Move`/`Set` ops used to panic inside the
//! state/oplog locks, poisoning them and aborting the process. Ops without a
//! meaning are now rejected with `Err` and leave the document untouched and
//! usable; a move/set of a deleted element is applied with the same semantics as
//! a concurrent one, identically on every import path.
//! See `context/movable-list-op-validation.md`.

use loro::{ExportMode, LoroDoc, LoroValue, ToJson, VersionVector};
use serde_json::{json, Value};

/// Peer 1 inserts `[a, b, c]` (counters/lamports 0..=2) and then deletes `b`
/// (counter 3), so the list is `[a, c]` and `b`'s element id is `L1@1`.
fn base() -> LoroDoc {
    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    let list = doc.get_movable_list("list");
    list.insert(0, "a").unwrap();
    list.insert(1, "b").unwrap();
    list.insert(2, "c").unwrap();
    doc.commit();
    list.delete(1, 1).unwrap();
    doc.commit();
    doc
}

/// A single change by peer 1, causally after `base()`, holding one op on `container`.
/// Element ids in `content` use the peer index into `["1", "2"]`.
fn forged_on(container: &str, content: Value) -> String {
    json!({
        "schema_version": 1,
        "start_version": {},
        "peers": ["1", "2"],
        "changes": [{
            "id": "4@0",
            "timestamp": 0,
            "deps": ["3@0"],
            "lamport": 4,
            "msg": null,
            "ops": [{"container": container, "content": content, "counter": 4}]
        }]
    })
    .to_string()
}

fn forged(content: Value) -> String {
    forged_on("cid:root-list:MovableList", content)
}

fn move_deleted() -> Value {
    json!({"type": "move", "from": 0, "to": 1, "elem_id": "L1@0"})
}

struct Snapshot {
    value: LoroValue,
    vv: VersionVector,
}

impl Snapshot {
    fn of(doc: &LoroDoc) -> Self {
        Self {
            value: doc.get_deep_value(),
            vv: doc.oplog_vv(),
        }
    }

    fn assert_unchanged(&self, doc: &LoroDoc) {
        assert_eq!(doc.get_deep_value(), self.value);
        assert_eq!(doc.oplog_vv(), self.vv);
    }
}

/// The doc still accepts local edits and valid remote updates after a rejected import.
fn assert_usable(doc: &LoroDoc) {
    let list = doc.get_movable_list("list");
    let len = list.len();
    list.push("local").unwrap();
    list.mov(0, list.len() - 1).unwrap();
    doc.commit();
    assert_eq!(list.len(), len + 1);

    let remote = LoroDoc::new();
    remote.set_peer_id(9).unwrap();
    remote
        .import(&doc.export(ExportMode::Snapshot).unwrap())
        .unwrap();
    remote.get_movable_list("list").push("remote").unwrap();
    remote.commit();
    doc.import(&remote.export(ExportMode::updates(&doc.oplog_vv())).unwrap())
        .unwrap();
    assert_eq!(doc.get_deep_value(), remote.get_deep_value());
}

/// `base()` plus an edit by peer 2 that the forged op (deps `3@1`) cannot see, so
/// importing the forged op is concurrent with the doc's version.
fn base_with_concurrent_edit() -> LoroDoc {
    let doc = base();
    doc.set_peer_id(2).unwrap();
    doc.get_movable_list("list").insert(0, "x").unwrap();
    doc.commit();
    doc
}

fn assert_rejected(doc: &LoroDoc, json: String) {
    let before = Snapshot::of(doc);
    let err = doc.import_json_updates(json).unwrap_err();
    assert!(
        matches!(err, loro::LoroError::DecodeError(_)),
        "unexpected error: {err:?}"
    );
    before.assert_unchanged(doc);
    assert_usable(doc);
}

/// Import `json` on top of `base()` through every path and check they agree with
/// a doc that replays the whole history at once.
fn assert_same_result_on_every_import_path(json: String, expected: Value) {
    let carrier = LoroDoc::new();
    carrier
        .import(&base().export(ExportMode::all_updates()).unwrap())
        .unwrap();
    // Detached imports only touch the oplog, which gives us the op as binary.
    carrier.detach();
    carrier.import_json_updates(json.clone()).unwrap();
    let update = carrier
        .export(ExportMode::updates_in_range(vec![loro::IdSpan::new(
            1, 4, 5,
        )]))
        .unwrap();
    let all = carrier.export(ExportMode::all_updates()).unwrap();

    let replayed = LoroDoc::new();
    replayed.import(&all).unwrap();
    assert_eq!(replayed.get_deep_value().to_json_value(), expected);

    let from_snapshot = LoroDoc::new();
    from_snapshot
        .import(&base().export(ExportMode::Snapshot).unwrap())
        .unwrap();
    let detached = base();
    detached.detach();
    let docs = [base(), base(), base(), from_snapshot, detached];
    docs[0].import_json_updates(json.clone()).unwrap();
    docs[1].import(&update).unwrap();
    docs[2].import_batch(&[update.clone()]).unwrap();
    docs[3].import_json_updates(json).unwrap();
    docs[4].import(&update).unwrap();
    docs[4].attach();
    for doc in docs {
        assert_eq!(doc.get_deep_value(), replayed.get_deep_value());
        assert_eq!(doc.oplog_vv(), replayed.oplog_vv());
        assert_usable(&doc);
    }
}

#[test]
fn move_of_deleted_element_is_applied_like_a_concurrent_move() {
    // `from: 0` points at `a` in the op's version, so `a`'s position is consumed
    // and `b` comes back at index 1, exactly as for a concurrent move.
    assert_same_result_on_every_import_path(forged(move_deleted()), json!({"list": ["c", "b"]}));
}

#[test]
fn set_of_deleted_element_is_applied_like_a_concurrent_set() {
    assert_same_result_on_every_import_path(
        forged(json!({"type": "set", "elem_id": "L1@0", "value": "z"})),
        json!({"list": ["a", "c"]}),
    );
}

#[test]
fn move_or_set_of_unknown_element_is_rejected() {
    let ops = [
        json!({"type": "move", "from": 0, "to": 1, "elem_id": "L99@0"}),
        json!({"type": "set", "elem_id": "L99@0", "value": "z"}),
        // Peer 2 has no ops at all in `base()`.
        json!({"type": "move", "from": 0, "to": 1, "elem_id": "L0@1"}),
        // Lamport 3 is `base()`'s delete op, not an element.
        json!({"type": "set", "elem_id": "L3@0", "value": "z"}),
    ];
    for op in ops {
        assert_rejected(&base(), forged(op.clone()));
        assert_rejected(&base_with_concurrent_edit(), forged(op));
    }
}

#[test]
fn move_of_element_from_another_container_is_rejected() {
    let doc = base();
    let other = doc.get_movable_list("other");
    other.insert(0, "o").unwrap();
    doc.commit();
    // `other`'s element is `L4@1`; target it from `list` in a change after it.
    let json = json!({
        "schema_version": 1,
        "start_version": {},
        "peers": ["1"],
        "changes": [{
            "id": "5@0", "timestamp": 0, "deps": ["4@0"], "lamport": 5, "msg": null,
            "ops": [{
                "container": "cid:root-list:MovableList",
                "content": {"type": "move", "from": 0, "to": 1, "elem_id": "L4@0"},
                "counter": 5
            }]
        }]
    })
    .to_string();
    assert_rejected(&doc, json);
}

#[test]
fn move_of_element_outside_causal_history_is_rejected() {
    // Peer 2 inserts `y` concurrently with `base()`'s delete; the forged op by
    // peer 1 depends only on `3@1` and so cannot have seen `y` (`L3@2`).
    let doc = base();
    let concurrent = LoroDoc::new();
    concurrent.set_peer_id(2).unwrap();
    let mut before_delete = VersionVector::default();
    before_delete.insert(1, 3);
    concurrent
        .import(
            &doc.export(ExportMode::updates_till(&before_delete))
                .unwrap(),
        )
        .unwrap();
    concurrent.get_movable_list("list").push("y").unwrap();
    concurrent.commit();
    doc.import(&concurrent.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    assert_eq!(
        doc.get_deep_value().to_json_value(),
        json!({"list": ["a", "c", "y"]})
    );

    assert_rejected(
        &doc,
        forged(json!({"type": "move", "from": 0, "to": 1, "elem_id": "L3@1"})),
    );
}

#[test]
fn move_with_out_of_bounds_index_is_rejected() {
    let ops = [
        json!({"type": "move", "from": 9, "to": 0, "elem_id": "L0@0"}),
        json!({"type": "move", "from": 0, "to": 9, "elem_id": "L0@0"}),
        json!({"type": "move", "from": 5, "to": 1, "elem_id": "L1@0"}),
    ];
    for op in ops {
        assert_rejected(&base(), forged(op.clone()));
        assert_rejected(&base_with_concurrent_edit(), forged(op));
    }
}

#[test]
fn detached_import_of_unknown_element_is_rejected() {
    let doc = base();
    doc.detach();
    let before = Snapshot::of(&doc);
    doc.import_json_updates(forged(
        json!({"type": "move", "from": 0, "to": 1, "elem_id": "L99@0"}),
    ))
    .unwrap_err();
    before.assert_unchanged(&doc);
    doc.attach();
    assert_usable(&doc);
}

#[test]
fn import_batch_with_out_of_bounds_move_rolls_back_and_stays_attached() {
    // Detached imports only touch the oplog (state bounds are checked when the
    // doc reattaches), which lets us export the invalid op as binary.
    let carrier = base();
    carrier.detach();
    carrier
        .import_json_updates(forged(
            json!({"type": "move", "from": 9, "to": 0, "elem_id": "L0@0"}),
        ))
        .unwrap();
    let forged_update = carrier
        .export(ExportMode::updates_in_range(vec![loro::IdSpan::new(
            1, 4, 5,
        )]))
        .unwrap();

    let doc = base();
    let valid = LoroDoc::new();
    valid.set_peer_id(5).unwrap();
    valid
        .import(&doc.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    valid.get_movable_list("list").push("v").unwrap();
    valid.commit();
    let valid_update = valid.export(ExportMode::updates(&doc.oplog_vv())).unwrap();

    let before = Snapshot::of(&doc);
    doc.import(&forged_update).unwrap_err();
    before.assert_unchanged(&doc);

    doc.import_batch(&[valid_update, forged_update])
        .unwrap_err();
    assert!(!doc.is_detached());
    before.assert_unchanged(&doc);
    assert_usable(&doc);
}

#[test]
fn concurrent_delete_and_move_resurrects_moved_element() {
    // Legitimate concurrency: a move of an element concurrently deleted by
    // another peer is kept, because the move created a new position for it.
    let a = LoroDoc::new();
    a.set_peer_id(1).unwrap();
    let list = a.get_movable_list("list");
    list.insert(0, "a").unwrap();
    list.insert(1, "b").unwrap();
    list.insert(2, "c").unwrap();
    a.commit();
    let b = LoroDoc::new();
    b.set_peer_id(2).unwrap();
    b.import(&a.export(ExportMode::all_updates()).unwrap())
        .unwrap();

    a.get_movable_list("list").delete(1, 1).unwrap();
    a.commit();
    b.get_movable_list("list").mov(1, 2).unwrap();
    b.get_movable_list("list").set(2, "B").unwrap();
    b.commit();

    a.import(&b.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    b.import(&a.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    assert_eq!(a.get_deep_value(), b.get_deep_value());
    assert_eq!(
        a.get_deep_value().to_json_value(),
        json!({"list": ["a", "c", "B"]})
    );

    // A peer that only now learns about both branches reaches the same state.
    let c = LoroDoc::new();
    c.import(&a.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    assert_eq!(c.get_deep_value(), a.get_deep_value());
}

#[test]
fn move_and_set_on_shallow_root_element_still_import() {
    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    let list = doc.get_movable_list("list");
    list.insert(0, "a").unwrap();
    list.insert(1, "b").unwrap();
    list.insert(2, "c").unwrap();
    doc.commit();
    list.insert(3, "d").unwrap();
    doc.commit();

    let shallow = LoroDoc::new();
    shallow
        .import(
            &doc.export(ExportMode::shallow_snapshot(&doc.oplog_frontiers()))
                .unwrap(),
        )
        .unwrap();
    let vv = shallow.oplog_vv();

    // Elements created before the shallow root are moved/set after it.
    list.mov(0, 3).unwrap();
    list.set(0, "B").unwrap();
    doc.commit();
    shallow
        .import(&doc.export(ExportMode::updates(&vv)).unwrap())
        .unwrap();
    assert_eq!(shallow.get_deep_value(), doc.get_deep_value());

    // Unknown elements are still rejected on a shallow doc.
    let json = json!({
        "schema_version": 1,
        "start_version": {},
        "peers": ["1"],
        "changes": [{
            "id": "6@0", "timestamp": 0, "deps": ["5@0"], "lamport": 6, "msg": null,
            "ops": [{
                "container": "cid:root-list:MovableList",
                "content": {"type": "move", "from": 0, "to": 1, "elem_id": "L99@0"},
                "counter": 6
            }]
        }]
    })
    .to_string();
    assert_rejected(&shallow, json);
}

#[test]
fn move_of_element_deleted_before_shallow_root_is_rejected() {
    // The trimmed history of a shallow doc only keeps elements alive at the root,
    // and no valid op after the root can see `b`, which was deleted before it.
    let src = base();
    let shallow_snapshot = src
        .export(ExportMode::shallow_snapshot(&src.oplog_frontiers()))
        .unwrap();
    for op in [
        move_deleted(),
        json!({"type": "set", "elem_id": "L1@0", "value": "z"}),
        json!({"type": "move", "from": 0, "to": 1, "elem_id": "L99@0"}),
    ] {
        let doc = LoroDoc::new();
        doc.import(&shallow_snapshot).unwrap();
        assert_rejected(&doc, forged(op));
    }

    // `a` is alive at the root, so it can still be moved and set.
    let doc = LoroDoc::new();
    doc.import(&shallow_snapshot).unwrap();
    doc.import_json_updates(forged(
        json!({"type": "set", "elem_id": "L0@0", "value": "A"}),
    ))
    .unwrap();
    assert_eq!(
        doc.get_deep_value().to_json_value(),
        json!({"list": ["A", "c"]})
    );
}

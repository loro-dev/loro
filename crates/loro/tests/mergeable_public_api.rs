//! Coverage for the public `loro::LoroMap::ensure_mergeable_*` wrappers.
//!
//! The integration tests in `crates/loro-internal/tests/mergeable_container/*` exercise
//! the underlying `MapHandler` directly. These tests confirm the public `loro` crate's
//! wrapper methods on `LoroMap` forward to the handler correctly and that the returned
//! `LoroCounter` / `LoroMap` / `LoroList` / `LoroMovableList` / `LoroText` / `LoroTree`
//! values are usable end-to-end through the documented public API.

use loro::{
    ContainerID, ContainerTrait, ContainerType, ExportMode, Index, LoroDoc, ToJson, TreeParentId,
    ValueOrContainer,
};
use serde_json::json;
use serial_test::parallel;

fn doc(peer: u64) -> LoroDoc {
    let d = LoroDoc::new();
    d.set_peer_id(peer).unwrap();
    d
}

fn sync(a: &LoroDoc, b: &LoroDoc) {
    a.import(&b.export(ExportMode::updates(&a.oplog_vv())).unwrap())
        .unwrap();
    b.import(&a.export(ExportMode::updates(&b.oplog_vv())).unwrap())
        .unwrap();
}

/// `LoroMap::ensure_mergeable_counter` returns a working `LoroCounter` whose increments
/// converge across peers on concurrent first-create.
#[test]
#[parallel]
#[cfg(feature = "counter")]
fn loro_map_ensure_mergeable_counter_through_public_api() {
    let a = doc(1);
    let b = doc(2);

    let a_counter = a
        .get_map("state")
        .ensure_mergeable_counter("revision")
        .unwrap();
    let b_counter = b
        .get_map("state")
        .ensure_mergeable_counter("revision")
        .unwrap();
    assert_eq!(
        a_counter.id(),
        b_counter.id(),
        "both peers must produce the same deterministic cid via the public API"
    );
    assert!(a_counter.id().is_mergeable());

    a_counter.increment(1.0).unwrap();
    b_counter.increment(1.0).unwrap();
    a.commit();
    b.commit();
    sync(&a, &b);

    assert_eq!(
        a.get_deep_value().to_json_value(),
        json!({ "state": { "revision": 2.0 } })
    );
    assert_eq!(
        b.get_deep_value().to_json_value(),
        a.get_deep_value().to_json_value()
    );
}

/// `LoroMap::ensure_mergeable_map` returns a working `LoroMap` that supports nested
/// disjoint-key writes converging across peers.
#[test]
#[parallel]
fn loro_map_ensure_mergeable_map_through_public_api() {
    let a = doc(1);
    let b = doc(2);

    let a_profile = a.get_map("state").ensure_mergeable_map("profile").unwrap();
    let b_profile = b.get_map("state").ensure_mergeable_map("profile").unwrap();
    assert_eq!(a_profile.id(), b_profile.id());

    a_profile.insert("name", "Ada").unwrap();
    b_profile.insert("title", "Engineer").unwrap();
    a.commit();
    b.commit();
    sync(&a, &b);

    assert_eq!(
        a.get_deep_value().to_json_value(),
        json!({ "state": { "profile": { "name": "Ada", "title": "Engineer" } } })
    );
}

/// `LoroMap::ensure_mergeable_list` returns a working `LoroList` whose concurrent inserts
/// both survive after sync.
#[test]
#[parallel]
fn loro_map_ensure_mergeable_list_through_public_api() {
    let a = doc(1);
    let b = doc(2);

    let a_items = a.get_map("state").ensure_mergeable_list("items").unwrap();
    let b_items = b.get_map("state").ensure_mergeable_list("items").unwrap();
    assert_eq!(a_items.id(), b_items.id());

    a_items.insert(0, "A").unwrap();
    b_items.insert(0, "B").unwrap();
    a.commit();
    b.commit();
    sync(&a, &b);

    let value = a.get_deep_value().to_json_value();
    assert!(
        value == json!({ "state": { "items": ["A", "B"] } })
            || value == json!({ "state": { "items": ["B", "A"] } }),
        "both concurrent inserts must survive on the merged list; got {value}"
    );
    assert_eq!(b.get_deep_value().to_json_value(), value);
}

/// `LoroMap::ensure_mergeable_text` returns a working `LoroText` whose concurrent edits
/// both survive after sync.
#[test]
#[parallel]
fn loro_map_ensure_mergeable_text_through_public_api() {
    let a = doc(1);
    let b = doc(2);

    let a_notes = a.get_map("state").ensure_mergeable_text("notes").unwrap();
    let b_notes = b.get_map("state").ensure_mergeable_text("notes").unwrap();
    assert_eq!(a_notes.id(), b_notes.id());

    a_notes.insert(0, "A").unwrap();
    b_notes.insert(0, "B").unwrap();
    a.commit();
    b.commit();
    sync(&a, &b);

    let value = a.get_deep_value().to_json_value();
    assert!(
        value == json!({ "state": { "notes": "AB" } })
            || value == json!({ "state": { "notes": "BA" } }),
        "both concurrent text edits must survive; got {value}"
    );
}

/// `LoroMap::ensure_mergeable_movable_list` returns a working `LoroMovableList` whose
/// concurrent first-create from two peers resolves to the same deterministic cid,
/// and inserts on both peers survive after sync. Also exercises the `mov` operation
/// on the returned handler to confirm the full public surface forwards correctly.
#[test]
#[parallel]
fn loro_map_ensure_mergeable_movable_list_through_public_api() {
    let a = doc(1);
    let b = doc(2);

    let a_items = a
        .get_map("state")
        .ensure_mergeable_movable_list("items")
        .unwrap();
    let b_items = b
        .get_map("state")
        .ensure_mergeable_movable_list("items")
        .unwrap();
    assert_eq!(
        a_items.id(),
        b_items.id(),
        "both peers must produce the same deterministic mergeable MovableList cid"
    );
    assert!(a_items.id().is_mergeable());
    assert_eq!(a_items.id().container_type(), ContainerType::MovableList);

    a_items.insert(0, "first").unwrap();
    a_items.insert(1, "second").unwrap();
    b_items.insert(0, "from_b").unwrap();
    a.commit();
    b.commit();
    sync(&a, &b);

    let value = a.get_deep_value().to_json_value();
    let items = value["state"]["items"]
        .as_array()
        .expect("items must be a list");
    assert_eq!(
        items.len(),
        3,
        "both peers' inserts must survive on the merged movable list; got {value}"
    );
    assert!(items.contains(&json!("first")));
    assert!(items.contains(&json!("second")));
    assert!(items.contains(&json!("from_b")));
    assert_eq!(b.get_deep_value().to_json_value(), value);

    // Exercise the MovableList-specific `mov` operation through the handler returned
    // by the mergeable getter to confirm it's a fully-functional MovableList.
    let pre_move = a.get_deep_value().to_json_value();
    let a_items_again = a
        .get_map("state")
        .ensure_mergeable_movable_list("items")
        .unwrap();
    a_items_again.mov(0, a_items_again.len() - 1).unwrap();
    a.commit();
    let post_move = a.get_deep_value().to_json_value();
    assert_ne!(
        pre_move, post_move,
        "mov on the returned MovableList handler must change order"
    );
}

/// `LoroMap::ensure_mergeable_tree` returns a working `LoroTree` whose nodes are
/// reachable through the deep value and whose tree operations forward correctly
/// through the public API.
#[test]
#[parallel]
fn loro_map_ensure_mergeable_tree_through_public_api() {
    let a = doc(1);
    let b = doc(2);

    let a_tree = a.get_map("state").ensure_mergeable_tree("hierarchy").unwrap();
    let b_tree = b.get_map("state").ensure_mergeable_tree("hierarchy").unwrap();
    assert_eq!(
        a_tree.id(),
        b_tree.id(),
        "both peers must produce the same deterministic mergeable Tree cid"
    );
    assert!(a_tree.id().is_mergeable());
    assert_eq!(a_tree.id().container_type(), ContainerType::Tree);

    // Exercise the Tree-specific create operation through the handler returned by
    // the mergeable getter on both peers, then converge.
    let a_root_node = a_tree.create(TreeParentId::Root).unwrap();
    let _a_child = a_tree.create(TreeParentId::Node(a_root_node)).unwrap();
    let _b_root_node = b_tree.create(TreeParentId::Root).unwrap();
    a.commit();
    b.commit();
    sync(&a, &b);

    let va = a.get_deep_value().to_json_value();
    let vb = b.get_deep_value().to_json_value();
    assert_eq!(va, vb, "Tree state must converge across peers after sync");

    // Both root nodes must show up in the merged tree.
    let hierarchy = &va["state"]["hierarchy"];
    let roots = hierarchy.as_array().expect("tree value must be an array");
    assert_eq!(
        roots.len(),
        2,
        "both peers' root nodes must survive on the merged tree; got {va}"
    );
}

/// Kind change through the public API: register one mergeable kind under a key,
/// then ask for another. Requesting a different kind is a deliberate kind change
/// that rewrites the parent map marker, so the public `LoroMap::ensure_mergeable_*`
/// wrappers succeed and the active child switches to the newly requested kind.
#[test]
#[parallel]
fn loro_map_ensure_mergeable_kind_change_through_public_api() {
    let d = doc(1);
    let root = d.get_map("state");

    let text = root.ensure_mergeable_text("field").unwrap();
    text.insert(0, "hello").unwrap();
    d.commit();
    assert_eq!(
        d.get_deep_value().to_json_value(),
        json!({ "state": { "field": "hello" } })
    );

    // Switch the kind: this rewrites the marker to Map.
    let map = root.ensure_mergeable_map("field").unwrap();
    map.insert("k", 1).unwrap();
    d.commit();
    assert_eq!(
        d.get_deep_value().to_json_value(),
        json!({ "state": { "field": { "k": 1 } } }),
        "requesting a different kind rewrites the marker to that kind"
    );
}

/// A logical path returned for a mergeable child must be accepted by `get_by_path`.
/// The final map slot stores a binary marker internally, but public path lookup
/// should return the child container, not that raw marker value.
#[test]
#[parallel]
#[cfg(feature = "counter")]
fn loro_get_by_path_round_trips_mergeable_final_child() {
    let d = doc(1);
    let root = d.get_map("state");
    let counter = root.ensure_mergeable_counter("revision").unwrap();

    let path = d
        .get_path_to_container(&counter.id())
        .expect("mergeable counter should have a logical path");
    let indexes = path
        .iter()
        .map(|(_, index)| index.clone())
        .collect::<Vec<_>>();
    assert_eq!(
        indexes,
        vec![Index::Key("state".into()), Index::Key("revision".into())]
    );

    let value = d
        .get_by_path(&indexes)
        .expect("logical mergeable path should resolve");
    let ValueOrContainer::Container(container) = value else {
        panic!("expected mergeable child container from get_by_path");
    };
    assert_eq!(container.id(), counter.id());
}

/// Nested logical paths must resolve through mergeable map intermediates without
/// materializing or re-getting them first.
#[test]
#[parallel]
#[cfg(feature = "counter")]
fn loro_get_by_path_round_trips_nested_mergeable_child() {
    let d = doc(1);
    let root = d.get_map("state");
    let profile = root.ensure_mergeable_map("profile").unwrap();
    let counter = profile.ensure_mergeable_counter("revision").unwrap();
    counter.increment(7.0).unwrap();
    d.commit();

    let path = d
        .get_path_to_container(&counter.id())
        .expect("nested mergeable counter should have a logical path");
    let indexes = path
        .iter()
        .map(|(_, index)| index.clone())
        .collect::<Vec<_>>();
    assert_eq!(
        indexes,
        vec![
            Index::Key("state".into()),
            Index::Key("profile".into()),
            Index::Key("revision".into()),
        ]
    );

    let value = d
        .get_by_path(&indexes)
        .expect("nested logical mergeable path should resolve");
    let ValueOrContainer::Container(container) = value else {
        panic!("expected nested mergeable child container from get_by_path");
    };
    assert_eq!(container.id(), counter.id());
    let loro::Container::Counter(counter_from_path) = container else {
        panic!("expected counter container from get_by_path");
    };
    assert_eq!(counter_from_path.get_value(), 7.0);

    let profile_value = d
        .get_by_path(&[Index::Key("state".into()), Index::Key("profile".into())])
        .expect("mergeable map intermediate should resolve");
    let ValueOrContainer::Container(container) = profile_value else {
        panic!("expected mergeable map container from get_by_path");
    };
    assert_eq!(container.id(), profile.id());
}

/// `LoroDoc::has_container` for mergeable cids: a never-ensured mergeable cid must
/// report `false`, but once `ensure_mergeable_*` has written the parent's child ref the
/// id must resolve — before any op is written into the child and before commit. This
/// aligns id lookup with `map.get(key)` and `toJSON()`, which already expose the child.
#[test]
#[parallel]
#[cfg(feature = "counter")]
fn loro_has_container_for_mergeable_cid_through_public_api() {
    let d = doc(1);
    let root = d.get_map("state");

    let parent_id = root.id();
    let unwritten_cid = ContainerID::new_mergeable(&parent_id, "revision", ContainerType::Counter);
    assert!(unwritten_cid.is_mergeable());
    assert!(
        !d.has_container(&unwritten_cid),
        "has_container must report false for a never-ensured mergeable cid"
    );
    assert!(
        d.get_container(unwritten_cid.clone()).is_none(),
        "get_container must report None for a never-ensured mergeable cid"
    );

    let counter = root.ensure_mergeable_counter("revision").unwrap();
    assert_eq!(counter.id(), unwritten_cid);
    assert!(
        d.has_container(&unwritten_cid),
        "has_container must report true right after ensure, before any op or commit"
    );

    counter.increment(1.0).unwrap();
    assert!(
        d.has_container(&unwritten_cid),
        "has_container must report true after the mergeable child has state"
    );
}

/// An ensured-but-empty mergeable child must be retrievable by id through
/// `LoroDoc::get_container`, and the returned handler must be the same container
/// (writes through it converge with the original handle).
#[test]
#[parallel]
fn loro_get_container_resolves_ensured_but_empty_mergeable_child() {
    let d = doc(1);
    let records = d.get_map("records");
    let note = records.ensure_mergeable_map("note").unwrap();

    let retrieved = d
        .get_container(note.id())
        .expect("ensured-but-empty mergeable child must resolve by id");
    let loro::Container::Map(retrieved) = retrieved else {
        panic!("expected a map container");
    };
    assert_eq!(retrieved.id(), note.id());

    retrieved.insert("title", "hello").unwrap();
    assert_eq!(
        d.get_deep_value().to_json_value(),
        json!({ "records": { "note": { "title": "hello" } } })
    );
}

/// The ensured-but-empty state must resolve by id on a remote peer too: peer A ensures
/// an empty mergeable child and syncs; peer B sees it in `toJSON()` and must also be
/// able to resolve the same cid via `has_container` / `get_container`.
#[test]
#[parallel]
fn loro_get_container_resolves_ensured_but_empty_mergeable_child_on_remote_peer() {
    let a = doc(1);
    let b = doc(2);

    let note = a.get_map("records").ensure_mergeable_map("note").unwrap();
    a.commit();
    sync(&a, &b);

    assert_eq!(
        b.get_deep_value().to_json_value(),
        json!({ "records": { "note": {} } })
    );
    assert!(
        b.has_container(&note.id()),
        "remote peer must resolve an ensured-but-empty mergeable child by id"
    );
    let retrieved = b
        .get_container(note.id())
        .expect("remote peer must resolve an ensured-but-empty mergeable child by id");
    assert_eq!(retrieved.id(), note.id());
}

/// Deletion semantics: an ensured-but-empty mergeable child whose parent ref is removed
/// has no state anywhere, so its id stops resolving. A mergeable child that already has
/// ops keeps resolving after the ref is removed — matching how ordinary deleted
/// containers stay retrievable by id.
#[test]
#[parallel]
fn loro_get_container_for_deleted_mergeable_children() {
    let d = doc(1);
    let root = d.get_map("state");

    let empty = root.ensure_mergeable_map("empty").unwrap();
    let written = root.ensure_mergeable_map("written").unwrap();
    written.insert("x", 1).unwrap();
    d.commit();
    assert!(d.has_container(&empty.id()));
    assert!(d.has_container(&written.id()));

    root.delete("empty").unwrap();
    root.delete("written").unwrap();
    d.commit();

    assert!(
        !d.has_container(&empty.id()),
        "an ensured-but-empty mergeable child must stop resolving once its ref is removed"
    );
    assert!(
        d.has_container(&written.id()),
        "a mergeable child with its own state must stay resolvable after deletion, \
         matching ordinary deleted-container semantics"
    );
}

/// `ensure_mergeable_*` must not silently overwrite a non-mergeable value through the public API:
/// a scalar already at the key makes the call an `ArgErr` and leaves the value in place.
#[test]
#[parallel]
fn loro_map_ensure_mergeable_rejects_overwriting_scalar_through_public_api() {
    use loro::LoroError;

    let d = doc(1);
    let root = d.get_map("state");
    root.insert("field", 5).unwrap();

    let err = root
        .ensure_mergeable_list("field")
        .expect_err("ensure_mergeable over a scalar must error");
    assert!(
        matches!(err, LoroError::ArgErr(_)),
        "expected ArgErr, got {err:?}"
    );
    let still = root.get("field").expect("scalar must still be present");
    assert_eq!(
        still.get_deep_value(),
        5.into(),
        "the existing scalar value must be left untouched"
    );
}

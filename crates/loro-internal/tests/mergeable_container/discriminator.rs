//! Marker-based mergeable type-conflict resolution.
//!
//! `ensure_mergeable_<kind>(key)` records which container kind is active at
//! `(parent, key)` by writing a compact binary marker op against the parent map.
//! The active mergeable child is whichever marker the parent map's regular LWW
//! resolves to. See loro-dev/loro#759.

#[path = "common.rs"]
mod common;
use common::{doc, sync};

use loro_common::{mergeable_marker, ContainerType, LoroError, LoroValue, MERGEABLE_MARKER_MAGIC};
use loro_internal::{HandlerTrait, ToJson};
use serde_json::json;

/// `ensure_mergeable_map(key)` writes the marker into the parent map's slot at
/// `key`, so a plain `map.get(key)` reads back that binary value.
#[test]
fn ensure_mergeable_writes_marker_into_parent_slot() {
    let d = doc(1);
    let root = d.get_map("state");

    assert_eq!(root.get("tags"), None, "key starts empty");

    let _list = root.ensure_mergeable_list("tags").unwrap();

    assert_eq!(
        root.get("tags"),
        Some(mergeable_marker(&root.id(), "tags", ContainerType::List)),
        "ensure_mergeable_list must write the List marker into the parent slot"
    );
}

/// `ensure_mergeable_<kind>(key)` must not silently clobber an existing non-mergeable value at the
/// key. A plain scalar or a regular child container occupying the slot makes the call an error,
/// so the marker overwrite is never a hidden side effect of a `get_`-named API.
#[test]
fn ensure_mergeable_rejects_overwriting_a_scalar_value() {
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
    assert_eq!(
        root.get("field"),
        Some(5.into()),
        "the existing scalar value must be left untouched"
    );
}

/// The same guard applies when the slot holds a regular child container: requesting a mergeable
/// child there is an error and must not overwrite the container reference.
#[test]
fn ensure_mergeable_rejects_overwriting_a_regular_child_container() {
    let d = doc(1);
    let root = d.get_map("state");
    root.insert_container("field", loro_internal::handler::MapHandler::new_detached())
        .unwrap();

    let err = root
        .ensure_mergeable_list("field")
        .expect_err("ensure_mergeable over a regular child container must error");
    assert!(
        matches!(err, LoroError::ArgErr(_)),
        "expected ArgErr, got {err:?}"
    );
}

/// An empty slot, or a slot already holding a mergeable marker, is fair game:
/// `ensure_mergeable_<kind>` creates on empty and is idempotent / kind-changes over an existing
/// marker. Only non-mergeable occupants are rejected.
#[test]
fn ensure_mergeable_allows_empty_slot_and_existing_marker() {
    let d = doc(1);
    let root = d.get_map("state");

    // Empty slot: creates.
    let _list = root.ensure_mergeable_list("a").unwrap();
    // Same kind again: idempotent, still Ok.
    let _list2 = root.ensure_mergeable_list("a").unwrap();
    // Different kind over an existing marker: kind change, still Ok.
    let _map = root.ensure_mergeable_map("a").unwrap();
}

/// A user string must stay a plain scalar and block mergeable child creation.
#[test]
fn user_string_is_not_mergeable() {
    let d = doc(1);
    let root = d.get_map("state");

    root.insert("field", "not-a-marker").unwrap();
    d.commit_then_renew();

    assert_eq!(
        d.get_deep_value().to_json_value(),
        json!({ "state": { "field": "not-a-marker" } })
    );
    let err = root
        .ensure_mergeable_map("field")
        .expect_err("user string must not be treated as a mergeable marker");
    assert!(
        matches!(err, LoroError::ArgErr(_)),
        "expected ArgErr, got {err:?}"
    );
}

/// A marker is bound to its exact `(parent, key, kind)` by the digest. Copying
/// the binary payload to another key leaves it as an inert user binary value.
#[test]
fn marker_for_another_key_is_not_mergeable() {
    let d = doc(1);
    let root = d.get_map("state");
    let marker_for_other = mergeable_marker(&root.id(), "other", ContainerType::Map);

    root.insert("field", marker_for_other.clone()).unwrap();
    d.commit_then_renew();

    assert_eq!(root.get("field"), Some(marker_for_other));
    let err = root
        .ensure_mergeable_map("field")
        .expect_err("marker digest for another key must not activate this key");
    assert!(
        matches!(err, LoroError::ArgErr(_)),
        "expected ArgErr, got {err:?}"
    );
}

/// A second `ensure_mergeable_<kind>(key)` with the SAME kind is idempotent: it
/// does not emit another op (the parent slot already holds the matching marker).
#[test]
fn repeated_ensure_mergeable_same_kind_is_idempotent() {
    let d = doc(1);
    let root = d.get_map("state");

    let _first = root.ensure_mergeable_map("profile").unwrap();
    d.commit_then_renew();
    let ops_after_first = d.len_ops();

    let _second = root.ensure_mergeable_map("profile").unwrap();
    d.commit_then_renew();
    let ops_after_second = d.len_ops();

    assert_eq!(
        ops_after_second, ops_after_first,
        "a second same-kind ensure_mergeable must not emit another op"
    );
}

/// The resolver substitutes the resolved child for the binary marker in deep value: the (empty)
/// child is visible immediately as its default, and the raw marker never leaks into the read view.
#[test]
fn marker_resolves_to_child_in_deep_value() {
    let d = doc(1);
    let root = d.get_map("state");
    let _list = root.ensure_mergeable_list("tags").unwrap();
    d.commit_then_renew();

    assert_eq!(
        d.get_deep_value().to_json_value(),
        json!({ "state": { "tags": [] } }),
        "the marker must resolve to the child's value, not leak as a binary value"
    );

    // Now add an item; the resolved child must reflect it.
    let list = root.ensure_mergeable_list("tags").unwrap();
    list.insert(0, "hello").unwrap();
    d.commit_then_renew();
    assert_eq!(
        d.get_deep_value().to_json_value(),
        json!({ "state": { "tags": ["hello"] } })
    );
}

/// A nested chain of mergeable children resolves at every hop: the intermediate mergeable map is
/// visible (it has a marker on its parent) even though it never receives a direct content op, only
/// its nested child does. This is the case the old `has_ops` gate broke.
#[test]
#[cfg(feature = "counter")]
fn nested_mergeable_chain_resolves_at_every_hop() {
    let d = doc(1);
    let profile = d.get_map("state").ensure_mergeable_map("profile").unwrap();
    let rev = profile.ensure_mergeable_counter("revision").unwrap();
    rev.increment(5.0).unwrap();
    d.commit_then_renew();

    assert_eq!(
        d.get_deep_value().to_json_value(),
        json!({ "state": { "profile": { "revision": 5.0 } } }),
        "intermediate mergeable map must be visible via its marker"
    );
}

/// Two peers concurrently create the SAME kind under the same key. Both write the identical
/// marker, so the Map LWW merge is a no-op and both peers' contributions land in the one
/// deterministic cid.
#[test]
fn concurrent_same_kind_creation_merges_via_identical_marker() {
    let a = doc(1);
    let b = doc(2);

    let a_list = a.get_map("state").ensure_mergeable_list("tags").unwrap();
    a_list.insert(0, "from_a").unwrap();
    a.commit_then_renew();

    let b_list = b.get_map("state").ensure_mergeable_list("tags").unwrap();
    b_list.insert(0, "from_b").unwrap();
    b.commit_then_renew();

    sync(&a, &b);

    let va = a.get_deep_value().to_json_value();
    let vb = b.get_deep_value().to_json_value();
    assert_eq!(va, vb, "same-kind concurrent creation must converge");

    let tags = &va["state"]["tags"];
    assert!(tags.is_array(), "tags must be a list, got {tags:?}");
    let items: Vec<&str> = tags
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert!(
        items.contains(&"from_a") && items.contains(&"from_b"),
        "both peers' items must be preserved in the merged list; got {items:?}"
    );
}

/// Kind-change cycle: a key cycles List -> Map -> List. Each `ensure_mergeable_<kind>` rewrites the
/// marker. When the marker returns to List, the ORIGINAL List's contents resurface (its container
/// state was preserved by name across the cycle), proving the cycle is non-lossy.
#[test]
fn kind_change_cycle_resurfaces_original_list_contents() {
    let d = doc(1);
    let root = d.get_map("state");

    // v1: List with an item.
    let list = root.ensure_mergeable_list("field").unwrap();
    list.insert(0, "original").unwrap();
    d.commit_then_renew();
    assert_eq!(
        d.get_deep_value().to_json_value(),
        json!({ "state": { "field": ["original"] } })
    );

    // v2: Map (kind change). The List is now hidden, but its state is preserved by name.
    let map = root.ensure_mergeable_map("field").unwrap();
    map.insert("k", 1).unwrap();
    d.commit_then_renew();
    assert_eq!(
        d.get_deep_value().to_json_value(),
        json!({ "state": { "field": { "k": 1 } } }),
        "Map marker active; List hidden"
    );

    // v3: back to List. The marker returns to List and the ORIGINAL contents resurface.
    let list_again = root.ensure_mergeable_list("field").unwrap();
    d.commit_then_renew();
    assert_eq!(
        list_again.id(),
        list.id(),
        "List cid is deterministic across the cycle"
    );
    assert_eq!(
        d.get_deep_value().to_json_value(),
        json!({ "state": { "field": ["original"] } }),
        "original List contents resurface after the List -> Map -> List cycle"
    );
}

/// Forward-compatibility envelope: the marker is a real Map value, so an older client that doesn't
/// understand mergeable containers sees it as an inert binary scalar. It does not look like a
/// plausible user string and does not introduce a fake container edge.
#[test]
fn marker_is_observable_as_raw_binary_for_old_clients() {
    let d = doc(1);
    let root = d.get_map("state");
    let _list = root.ensure_mergeable_list("tags").unwrap();
    d.commit_then_renew();

    // The raw slot value is the compact binary marker.
    let raw = root.get("tags").expect("slot has a value");
    let LoroValue::Binary(bytes) = &raw else {
        panic!("marker must be a binary value; got {raw:?}");
    };
    assert_eq!(bytes.len(), 8);
    assert!(bytes.starts_with(&MERGEABLE_MARKER_MAGIC));
    assert_eq!(
        bytes[MERGEABLE_MARKER_MAGIC.len()],
        ContainerType::List.to_u8()
    );
}

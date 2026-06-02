//! Discriminator-based mergeable type-conflict resolution.
//!
//! `get_mergeable_<kind>(key)` records which container kind is active at
//! `(parent, key)` by writing a `MapSet { key, value: String("🤝:<kind>") }`
//! op against the parent map. The active mergeable child is whichever
//! discriminator the parent map's regular LWW resolves to. See
//! loro-dev/loro#759.

#[path = "common.rs"]
mod common;
use common::{doc, sync};

use loro_common::{mergeable_discriminator, ContainerType, LoroError};
use loro_internal::{HandlerTrait, ToJson};
use serde_json::json;

/// `get_mergeable_map(key)` writes the discriminator string into the parent
/// map's slot at `key`, so a plain `map.get(key)` reads back `"🤝:Map"`.
#[test]
fn get_mergeable_writes_discriminator_into_parent_slot() {
    let d = doc(1);
    let root = d.get_map("state");

    assert_eq!(root.get("tags"), None, "key starts empty");

    let _list = root.get_mergeable_list("tags").unwrap();

    assert_eq!(
        root.get("tags"),
        Some(mergeable_discriminator(ContainerType::List)),
        "get_mergeable_list must write the List discriminator into the parent slot"
    );
}

/// `get_mergeable_<kind>(key)` must not silently clobber an existing non-mergeable value at the
/// key. A plain scalar or a regular child container occupying the slot makes the call an error,
/// so the discriminator-overwrite is never a hidden side effect of a `get_`-named API.
#[test]
fn get_mergeable_rejects_overwriting_a_scalar_value() {
    let d = doc(1);
    let root = d.get_map("state");
    root.insert("field", 5).unwrap();

    let err = root
        .get_mergeable_list("field")
        .expect_err("get_mergeable over a scalar must error");
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
fn get_mergeable_rejects_overwriting_a_regular_child_container() {
    let d = doc(1);
    let root = d.get_map("state");
    root.insert_container("field", loro_internal::handler::MapHandler::new_detached())
        .unwrap();

    let err = root
        .get_mergeable_list("field")
        .expect_err("get_mergeable over a regular child container must error");
    assert!(
        matches!(err, LoroError::ArgErr(_)),
        "expected ArgErr, got {err:?}"
    );
}

/// An empty slot, or a slot already holding a mergeable discriminator, is fair game:
/// `get_mergeable_<kind>` creates on empty and is idempotent / kind-changes over an existing
/// discriminator. Only non-mergeable occupants are rejected.
#[test]
fn get_mergeable_allows_empty_slot_and_existing_discriminator() {
    let d = doc(1);
    let root = d.get_map("state");

    // Empty slot: creates.
    let _list = root.get_mergeable_list("a").unwrap();
    // Same kind again: idempotent, still Ok.
    let _list2 = root.get_mergeable_list("a").unwrap();
    // Different kind over an existing discriminator: kind change, still Ok.
    let _map = root.get_mergeable_map("a").unwrap();
}

/// A second `get_mergeable_<kind>(key)` with the SAME kind is idempotent: it
/// does not emit another op (the parent slot already holds the matching
/// discriminator).
#[test]
fn repeated_get_mergeable_same_kind_is_idempotent() {
    let d = doc(1);
    let root = d.get_map("state");

    let _first = root.get_mergeable_map("profile").unwrap();
    d.commit_then_renew();
    let ops_after_first = d.len_ops();

    let _second = root.get_mergeable_map("profile").unwrap();
    d.commit_then_renew();
    let ops_after_second = d.len_ops();

    assert_eq!(
        ops_after_second, ops_after_first,
        "a second same-kind get_mergeable must not emit another op"
    );
}

/// The resolver substitutes the resolved child for the discriminator string
/// in deep value: the (empty) child is visible immediately as its default,
/// and the raw `"🤝:List"` string never leaks into the read view.
#[test]
fn discriminator_resolves_to_child_in_deep_value() {
    let d = doc(1);
    let root = d.get_map("state");
    let _list = root.get_mergeable_list("tags").unwrap();
    d.commit_then_renew();

    assert_eq!(
        d.get_deep_value().to_json_value(),
        json!({ "state": { "tags": [] } }),
        "the discriminator must resolve to the child's value, not leak as a string"
    );

    // Now add an item; the resolved child must reflect it.
    let list = root.get_mergeable_list("tags").unwrap();
    list.insert(0, "hello").unwrap();
    d.commit_then_renew();
    assert_eq!(
        d.get_deep_value().to_json_value(),
        json!({ "state": { "tags": ["hello"] } })
    );
}

/// A nested chain of mergeable children resolves at every hop: the
/// intermediate mergeable map is visible (it has a discriminator on its
/// parent) even though it never receives a direct content op — only its
/// nested child does. This is the case the old `has_ops` gate broke.
#[test]
#[cfg(feature = "counter")]
fn nested_mergeable_chain_resolves_at_every_hop() {
    let d = doc(1);
    let profile = d.get_map("state").get_mergeable_map("profile").unwrap();
    let rev = profile.get_mergeable_counter("revision").unwrap();
    rev.increment(5.0).unwrap();
    d.commit_then_renew();

    assert_eq!(
        d.get_deep_value().to_json_value(),
        json!({ "state": { "profile": { "revision": 5.0 } } }),
        "intermediate mergeable map must be visible via its discriminator"
    );
}

/// Two peers concurrently create the SAME kind under the same key. Both write
/// the identical discriminator string, so the Map LWW merge is a no-op and
/// both peers' contributions land in the one deterministic cid.
#[test]
fn concurrent_same_kind_creation_merges_via_identical_discriminator() {
    let a = doc(1);
    let b = doc(2);

    let a_list = a.get_map("state").get_mergeable_list("tags").unwrap();
    a_list.insert(0, "from_a").unwrap();
    a.commit_then_renew();

    let b_list = b.get_map("state").get_mergeable_list("tags").unwrap();
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

/// Kind-change cycle: a key cycles List -> Map -> List. Each `get_mergeable_<kind>` rewrites the
/// discriminator. When the discriminator returns to List, the ORIGINAL List's contents resurface
/// (its container state was preserved by name across the cycle), proving the cycle is non-lossy.
#[test]
fn kind_change_cycle_resurfaces_original_list_contents() {
    let d = doc(1);
    let root = d.get_map("state");

    // v1: List with an item.
    let list = root.get_mergeable_list("field").unwrap();
    list.insert(0, "original").unwrap();
    d.commit_then_renew();
    assert_eq!(
        d.get_deep_value().to_json_value(),
        json!({ "state": { "field": ["original"] } })
    );

    // v2: Map (kind change). The List is now hidden, but its state is preserved by name.
    let map = root.get_mergeable_map("field").unwrap();
    map.insert("k", 1).unwrap();
    d.commit_then_renew();
    assert_eq!(
        d.get_deep_value().to_json_value(),
        json!({ "state": { "field": { "k": 1 } } }),
        "Map discriminator active; List hidden"
    );

    // v3: back to List. The discriminator returns to List and the ORIGINAL contents resurface.
    let list_again = root.get_mergeable_list("field").unwrap();
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

/// Forward-compatibility envelope (issue #759 §5): the discriminator is a real Map value, so an
/// older client that doesn't understand mergeable containers sees it as a plain string in toJSON
/// output (a sentinel in the reserved namespace), rather than a plausible user value. We assert
/// the discriminator string is observable as a raw value via `MapHandler::get` (the surface an
/// older client's value-table read would hit).
#[test]
fn discriminator_is_observable_as_raw_string_for_old_clients() {
    use loro_common::MERGEABLE_NAMESPACE_PREFIX;

    let d = doc(1);
    let root = d.get_map("state");
    let _list = root.get_mergeable_list("tags").unwrap();
    d.commit_then_renew();

    // The raw slot value is the discriminator string in the reserved namespace.
    let raw = root.get("tags").expect("slot has a value");
    let loro_common::LoroValue::String(s) = &raw else {
        panic!("discriminator must be a string value; got {raw:?}");
    };
    assert!(
        s.starts_with(MERGEABLE_NAMESPACE_PREFIX),
        "old clients see the discriminator as a reserved-namespace sentinel string; got {s:?}"
    );
    assert_eq!(s.as_str(), "🤝:List");
}

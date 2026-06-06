//! Path resolution, deep value, subscriptions, undo, and has_container.

#[path = "common.rs"]
mod common;
use common::doc;

use std::sync::{Arc, Mutex};

use loro_internal::{event::Index, HandlerTrait, ToJson};
use serde_json::json;

#[test]
#[cfg(feature = "counter")]
fn get_path_returns_logical_parent_path_for_mergeable_child() {
    let doc = doc(1);
    let root = doc.get_map("state");
    let counter = root.ensure_mergeable_counter("revision").unwrap();
    // Exercise the cid path directly; child-side wiring needed to keep
    // mutating ops alive arrives in a later commit.
    let _ = counter.increment(1.0);

    let path = doc
        .get_path_to_container(&counter.id())
        .expect("mergeable counter should have a logical path");
    let indexes = path
        .iter()
        .map(|(_, index)| index.clone())
        .collect::<Vec<_>>();
    assert_eq!(
        indexes,
        vec![Index::Key("state".into()), Index::Key("revision".into()),],
        "mergeable child path should walk logical parent edges, not the synthetic Root name",
    );
}

#[test]
#[cfg(feature = "counter")]
fn deep_value_nests_mergeable_child_under_parent_and_hides_synthetic_root() {
    let doc = doc(1);
    let root = doc.get_map("state");
    let counter = root.ensure_mergeable_counter("revision").unwrap();
    counter.increment(2.0).unwrap();

    assert_eq!(
        doc.get_deep_value().to_json_value(),
        json!({ "state": { "revision": 2.0 } })
    );
}

/// `ensure_mergeable_*` writes a marker op into the parent map, which
/// realizes the child immediately (loro-dev/loro#759). So an unmutated
/// mergeable child IS visible in deep value, rendered as its empty default.
/// Because the marker is a real op, every peer that imports it agrees
/// on this view — there is no originating-peer divergence.
#[test]
fn deep_value_shows_unmutated_mergeable_child_as_empty() {
    let a = doc(1);
    let _child = a.get_map("state").ensure_mergeable_map("nested").unwrap();
    a.commit_then_renew();

    assert_eq!(
        a.get_deep_value().to_json_value(),
        json!({ "state": { "nested": {} } }),
        "the marker realizes the child; it shows as its empty default",
    );
}

/// Subscribing to the *parent* map must receive events when one of its
/// mergeable children is mutated: subscriptions on ancestor containers should
/// observe deltas from mergeable descendants, not only subscriptions on the
/// mergeable child itself.
#[test]
#[cfg(feature = "counter")]
fn parent_map_subscription_receives_mergeable_child_events() {
    let doc = doc(1);
    let root = doc.get_map("state");
    let counter = root.ensure_mergeable_counter("revision").unwrap();

    let received: Arc<Mutex<Vec<Vec<Index>>>> = Arc::new(Mutex::new(Vec::new()));
    let received_clone = received.clone();
    let _sub = doc.subscribe(
        &root.id(),
        Arc::new(move |event| {
            let mut g = received_clone.lock().unwrap();
            for container_diff in event.events.iter() {
                g.push(
                    container_diff
                        .path
                        .iter()
                        .map(|(_, idx)| idx.clone())
                        .collect::<Vec<_>>(),
                );
            }
        }),
    );

    counter.increment(1.0).unwrap();
    doc.commit_then_renew();

    let captured = received.lock().unwrap();
    assert!(
        captured.iter().any(|path| path
            .iter()
            .any(|idx| matches!(idx, Index::Key(k) if &**k == "revision"))),
        "parent map subscriber should see an event whose path includes the mergeable child's key 'revision'; got {captured:?}",
    );
}

/// A subscription on the mergeable child's cid must receive events when the
/// child is mutated, symmetric to the existing parent-map subscription test.
#[test]
#[cfg(feature = "counter")]
fn mergeable_child_subscription_receives_own_events() {
    let doc = doc(1);
    let counter = doc
        .get_map("state")
        .ensure_mergeable_counter("revision")
        .unwrap();

    let count: Arc<Mutex<usize>> = Arc::new(Mutex::new(0));
    let count_clone = count.clone();
    let _sub = doc.subscribe(
        &counter.id(),
        Arc::new(move |_event| {
            *count_clone.lock().unwrap() += 1;
        }),
    );

    counter.increment(1.0).unwrap();
    doc.commit_then_renew();
    counter.increment(2.0).unwrap();
    doc.commit_then_renew();

    let observed = *count.lock().unwrap();
    assert!(
        observed >= 2,
        "mergeable-child subscription must fire at least once per commit; got {observed} events"
    );
}

/// Mutations on a mergeable child must be undoable. Undoing reverts the
/// child's value; the parent marker persists (because it isn't the
/// increment op being undone).
#[test]
#[cfg(feature = "counter")]
fn undo_manager_reverts_mergeable_counter_mutation() {
    use loro_internal::UndoManager;

    let doc = doc(1);
    let counter = doc
        .get_map("state")
        .ensure_mergeable_counter("revision")
        .unwrap();
    let undo = UndoManager::new(&doc);

    counter.increment(5.0).unwrap();
    doc.commit_then_renew();
    assert_eq!(counter.get_value().to_json_value(), json!(5.0));

    let did_undo = undo.undo().expect("undo must succeed");
    assert!(did_undo, "undo must report it did something");

    // The mergeable child still exists (registration is not an op), but its
    // value is back to zero.
    assert_eq!(
        counter.get_value().to_json_value(),
        json!(0.0),
        "undo must revert the increment"
    );
}

/// Mergeable Roots have a deterministic `ContainerID::Root` in a reserved namespace,
/// but they are conceptually parented to a regular Map. The top-level root enumeration
/// (driven by `DocState::preferred_root_containers`, surfaced through
/// `LoroDoc::get_value`) must NOT include the synthetic mergeable Root — otherwise
/// the doc would expose a top-level key with a `🤝:...` hex name alongside the real
/// roots.
///
/// This guards the `id.is_mergeable()` skip in `preferred_root_containers` against
/// accidental removal: without it, peers would see two top-level entries (the actual
/// parent map plus the synthetic mergeable root) instead of one.
#[test]
#[cfg(feature = "counter")]
fn top_level_root_enumeration_skips_mergeable_roots() {
    use loro_common::MERGEABLE_NAMESPACE_PREFIX;

    let doc = doc(1);
    let root = doc.get_map("state");
    let counter = root.ensure_mergeable_counter("revision").unwrap();
    counter.increment(1.0).unwrap();
    doc.commit_then_renew();

    // Sanity: the mergeable cid carries the synthetic namespace prefix.
    let mergeable_cid = counter.id();
    assert!(mergeable_cid.is_mergeable());

    // `get_value()` returns a Map keyed by top-level root names. The mergeable cid's
    // synthetic Root name (🤝:<hex>) must NOT appear here.
    let top_level = doc.get_value().to_json_value();
    let map = top_level
        .as_object()
        .expect("top-level doc value must be a JSON object");
    let keys: Vec<&str> = map.keys().map(|s| s.as_str()).collect();
    assert!(
        keys.iter().any(|k| *k == "state"),
        "real parent root 'state' must appear in top-level enumeration; got {keys:?}"
    );
    assert!(
        keys.iter()
            .all(|k| !k.starts_with(MERGEABLE_NAMESPACE_PREFIX)),
        "no mergeable-namespace Root name must appear at the top level; got {keys:?}"
    );
    // Even more strictly: the synthetic root's exact name must not be a top-level key.
    let synthetic_name = match &mergeable_cid {
        loro_common::ContainerID::Root { name, .. } => name.to_string(),
        _ => unreachable!("mergeable cid is always a Root"),
    };
    assert!(
        !keys.iter().any(|k| *k == synthetic_name),
        "synthetic mergeable root name {synthetic_name:?} must not be in top-level keys {keys:?}"
    );

    // Deep value must show the mergeable child nested under its logical parent, not as a
    // sibling top-level entry.
    assert_eq!(
        doc.get_deep_value().to_json_value(),
        json!({ "state": { "revision": 1.0 } }),
        "mergeable child must be nested under its logical parent, not surfaced at the top level"
    );
}

/// `LoroDoc::has_container(cid)` must return `false` for a mergeable cid
/// that has never been written to, and `true` after the child has been
/// mutated. Mergeable existence depends on state, not on the name shape, so
/// the short-circuit for plain `Root` ids must skip mergeable namespace cids.
#[test]
#[cfg(feature = "counter")]
fn has_container_reports_false_for_unwritten_mergeable_child() {
    use loro_common::{ContainerID, ContainerType};

    let doc = doc(1);
    let root = doc.get_map("state");

    // Build the deterministic mergeable cid by hand, WITHOUT calling
    // ensure_mergeable_counter (which would write the marker).
    let parent_id = root.id();
    let unwritten_cid = ContainerID::new_mergeable(&parent_id, "revision", ContainerType::Counter);
    assert!(unwritten_cid.is_mergeable());
    assert!(
        !doc.has_container(&unwritten_cid),
        "has_container must report false for an unwritten mergeable cid"
    );

    // Now actually create and mutate the child. has_container must flip to true.
    let counter = root.ensure_mergeable_counter("revision").unwrap();
    counter.increment(1.0).unwrap();
    assert_eq!(counter.id(), unwritten_cid, "cid is deterministic");
    assert!(
        doc.has_container(&unwritten_cid),
        "has_container must report true after the mergeable child has state"
    );

    // Regular root containers still report true as before (regression guard).
    let regular_root = ContainerID::new_root("state", ContainerType::Map);
    assert!(doc.has_container(&regular_root));
}

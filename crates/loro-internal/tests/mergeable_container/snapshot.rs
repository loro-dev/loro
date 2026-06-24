//! Snapshot and update-import round-trips, including LWW recovery.

#[path = "common.rs"]
mod common;
use common::{doc, sync};

use loro_internal::{cursor::PosType, event::Index, loro::ExportMode, HandlerTrait, ToJson};
use serde_json::json;

/// Snapshot round-trip must preserve the parent edges (logical path) and
/// state values for mergeable child containers nested inside other mergeable
/// child containers.
///
/// Source peer creates `state` → mergeable map `profile` → mergeable counter
/// `revision`, mutates each, then exports a snapshot. Importing into a fresh
/// peer must reproduce the same deep value and the same logical path for
/// the counter.
#[test]
#[cfg(feature = "counter")]
fn snapshot_roundtrip_preserves_mergeable_parent_edges_and_values() {
    let source = doc(1);
    let root = source.get_map("state");
    let nested = root.ensure_mergeable_map("profile").unwrap();
    nested.insert("name", "Ada").unwrap();
    let counter = nested.ensure_mergeable_counter("revision").unwrap();
    counter.increment(3.0).unwrap();
    let counter_id = counter.id();

    let snapshot = source.export(ExportMode::Snapshot).unwrap();
    let imported = doc(2);
    imported.import(&snapshot).unwrap();

    let path = imported
        .get_path_to_container(&counter_id)
        .expect("mergeable counter must have a logical path after snapshot import");
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
        ],
        "imported counter should walk logical parent edges across two mergeable hops"
    );

    assert_eq!(
        imported.get_deep_value().to_json_value(),
        source.get_deep_value().to_json_value(),
        "deep value of the imported doc must match the source after snapshot round-trip"
    );

    let imported_counter = imported
        .get_map("state")
        .ensure_mergeable_map("profile")
        .unwrap()
        .ensure_mergeable_counter("revision")
        .unwrap();
    assert_eq!(imported_counter.id(), counter_id);
}

/// Peer B imports updates that originated from peer A's `ensure_mergeable_counter`
/// + `increment` calls, but peer B never locally called `ensure_mergeable_*`.
/// After import, peer B's deep value, container enumeration, and path
/// resolution for the mergeable child must all reflect the imported state.
#[test]
#[cfg(feature = "counter")]
fn update_import_resolves_mergeable_logical_edge_on_receiver() {
    let a = doc(1);
    let b = doc(2);

    let a_counter = a
        .get_map("state")
        .ensure_mergeable_counter("revision")
        .unwrap();
    a_counter.increment(5.0).unwrap();
    let a_counter_id = a_counter.id();
    a.commit_then_renew();

    // Peer B imports A's updates WITHOUT first calling ensure_mergeable_counter.
    let updates = a.export(ExportMode::updates(&b.oplog_vv())).unwrap();
    b.import(&updates).unwrap();

    // Path resolution must work directly from the deterministic cid. Calling the getter first
    // would rewrite the marker and could hide resolver bugs after import.
    let path = b.get_path_to_container(&a_counter_id).expect("path");
    let indexes = path.iter().map(|(_, idx)| idx.clone()).collect::<Vec<_>>();
    assert_eq!(
        indexes,
        vec![Index::Key("state".into()), Index::Key("revision".into())]
    );

    assert_eq!(
        b.get_deep_value().to_json_value(),
        json!({ "state": { "revision": 5.0 } }),
        "after update import, peer B's deep value must include the mergeable child"
    );

    // Peer B then locally resolves the mergeable handler — this must return
    // the same cid as the one peer A wrote, and the existing value.
    let b_counter = b
        .get_map("state")
        .ensure_mergeable_counter("revision")
        .unwrap();
    assert_eq!(b_counter.id(), a_counter_id);
    assert_eq!(b_counter.get_value().to_json_value(), json!(5.0));
}

/// Create a mergeable counter but never mutate it, then export a snapshot.
/// `ensure_mergeable_*` writes a marker into the parent map's value table,
/// which is ordinary map state and rides through the snapshot like any other
/// value (loro-dev/loro#759). So the receiving peer resolves the marker
/// to the same deterministic cid and sees the child as its empty default — no
/// special recovery is needed.
#[test]
#[cfg(feature = "counter")]
fn unmutated_mergeable_child_survives_snapshot_via_marker() {
    let a = doc(1);
    let counter = a
        .get_map("state")
        .ensure_mergeable_counter("revision")
        .unwrap();
    let counter_id = counter.id();
    // Deliberately no increment. Commit anyway so any pending state is flushed.
    a.commit_then_renew();

    let snapshot = a.export(ExportMode::Snapshot).unwrap();
    let b = doc(2);
    b.import(&snapshot).unwrap();

    let has_container_before_path = b.has_container(&counter_id);
    let path = b
        .get_path_to_container(&counter_id)
        .expect("unmutated mergeable child must resolve from marker");
    let indexes = path.iter().map(|(_, idx)| idx.clone()).collect::<Vec<_>>();
    assert_eq!(
        indexes,
        vec![Index::Key("state".into()), Index::Key("revision".into())]
    );
    assert_eq!(
        b.has_container(&counter_id),
        has_container_before_path,
        "path lookup must not change has_container semantics for mergeable cids"
    );

    // The binary marker rides through the snapshot as a normal map value, so B
    // resolves the same deterministic cid and renders it as an empty counter.
    assert_eq!(
        b.get_deep_value().to_json_value(),
        json!({ "state": { "revision": 0.0 } }),
        "marker survives snapshot round-trip; child resolves to its empty default",
    );

    let b_counter = b
        .get_map("state")
        .ensure_mergeable_counter("revision")
        .unwrap();
    assert_eq!(b_counter.id(), counter_id, "cid still deterministic");
    assert_eq!(b_counter.get_value().to_json_value(), json!(0.0));
}

/// Snapshot import where both peers registered the same `(key, kind)`:
/// deterministic cids match, recovery walk converges, content from peer A
/// wins through normal CRDT merge.
#[test]
fn snapshot_import_same_type_collision_converges() {
    let a = doc(1);
    let a_text = a.get_map("state").ensure_mergeable_text("notes").unwrap();
    a_text.insert(0, "A", PosType::Unicode).unwrap();
    a.commit_then_renew();
    let snapshot = a.export(ExportMode::Snapshot).unwrap();

    let b = doc(2);
    let b_text = b.get_map("state").ensure_mergeable_text("notes").unwrap();
    b_text.insert(0, "B", PosType::Unicode).unwrap();
    b.commit_then_renew();
    assert_eq!(a_text.id(), b_text.id(), "cids must match before import");

    b.import(&snapshot).unwrap();

    // Sync back so A sees both.
    sync(&a, &b);
    let value = a.get_deep_value().to_json_value();
    assert!(
        value == json!({ "state": { "notes": "AB" } })
            || value == json!({ "state": { "notes": "BA" } }),
        "both edits must survive on same-type collision; got {value}"
    );
    assert_eq!(b.get_deep_value().to_json_value(), value);
}

/// Snapshot import where the LOCAL peer registered a different kind for the same key than the
/// SNAPSHOT peer. The two binary markers compete in the parent map's slot for "k"; the parent
/// map's regular LWW deterministically resolves to one, so exactly one kind is visible. Both
/// containers' states are preserved; the loser is reachable by explicit lookup.
#[test]
fn snapshot_import_different_type_collision_resolves_by_lww() {
    let a = doc(1);
    let a_text = a.get_map("state").ensure_mergeable_text("k").unwrap();
    a_text.insert(0, "hello", PosType::Unicode).unwrap();
    a.commit_then_renew();
    let snapshot = a.export(ExportMode::Snapshot).unwrap();

    let b = doc(2);
    let b_map = b.get_map("state").ensure_mergeable_map("k").unwrap();
    b_map.insert("flag", true).unwrap();
    b.commit_then_renew();
    assert_ne!(
        a_text.id(),
        b_map.id(),
        "different kinds under the same key MUST produce different cids"
    );

    b.import(&snapshot).expect("import must not fail");

    // Exactly one kind is visible in deep value (whichever marker won the Map LWW).
    let value = b.get_deep_value().to_json_value();
    let k = &value["state"]["k"];
    let visible_kinds = [k.is_string(), k.is_object()];
    assert_eq!(
        visible_kinds.iter().filter(|x| **x).count(),
        1,
        "exactly one kind must be visible after LWW; got {k:?}"
    );

    // Both getters still succeed (each rewrites the marker to its kind). Neither errors —
    // requesting a kind is a kind change, not a conflict.
    let _ = b.get_map("state").ensure_mergeable_text("k").unwrap();
    let _ = b.get_map("state").ensure_mergeable_map("k").unwrap();
}

/// After snapshot import resolves `"k"` to a Text marker on the receiver, a local
/// `ensure_mergeable_map("k")` does NOT error — it rewrites the marker to Map (a deliberate
/// kind change). The Text container stays reachable by name; requesting it again rewrites the
/// marker back to Text and resurfaces its preserved contents.
#[test]
fn different_kind_request_after_snapshot_is_a_kind_change() {
    let a = doc(1);
    let a_text = a.get_map("state").ensure_mergeable_text("k").unwrap();
    a_text.insert(0, "x", PosType::Unicode).unwrap();
    a.commit_then_renew();
    let snapshot = a.export(ExportMode::Snapshot).unwrap();

    let b = doc(2);
    b.import(&snapshot).unwrap();
    assert_eq!(
        b.get_deep_value().to_json_value(),
        json!({ "state": { "k": "x" } }),
        "imported Text marker resolves to its content"
    );

    // Requesting a Map rewrites the marker to Map; no error.
    let b_map = b.get_map("state").ensure_mergeable_map("k").unwrap();
    b_map.insert("flag", true).unwrap();
    b.commit_then_renew();
    assert_eq!(
        b.get_deep_value().to_json_value(),
        json!({ "state": { "k": { "flag": true } } }),
        "different-kind request is a kind change, not an error"
    );

    // The Text is still reachable; requesting it again resurfaces its preserved content.
    let b_text = b.get_map("state").ensure_mergeable_text("k").unwrap();
    assert_eq!(b_text.id(), a_text.id());
    b.commit_then_renew();
    assert_eq!(
        b.get_deep_value().to_json_value(),
        json!({ "state": { "k": "x" } }),
        "re-requesting Text rewrites the marker back and resurfaces preserved content"
    );
}

/// Shallow snapshot export must also preserve the LOSING kind's state in a concurrent-kind
/// conflict. The loser is not reachable through the parent's winning marker — its only
/// addressability is via the deterministic mergeable cid. The snapshot's alive-container walk has
/// to include mergeable cids as retention roots, otherwise `retain_keys` would silently GC the
/// loser, and a later `ensure_mergeable_<loser_kind>` on the receiver would resurface an empty
/// child instead of preserved contents.
#[test]
fn shallow_snapshot_preserves_losing_kind_state() {
    let a = doc(1);
    let a_text = a.get_map("state").ensure_mergeable_text("k").unwrap();
    a_text.insert(0, "from_a", PosType::Unicode).unwrap();
    let a_text_id = a_text.id();
    a.commit_then_renew();

    // Advance A's clock past A's text marker, then rewrite the marker to Map (a kind change).
    // The Map marker now wins LWW on A and is the visible kind; the Text child becomes the loser
    // whose state lives only at its deterministic cid.
    for i in 0..6 {
        a.get_map("state").insert(&format!("noise_{i}"), i).unwrap();
        a.commit_then_renew();
    }
    let a_map = a.get_map("state").ensure_mergeable_map("k").unwrap();
    a_map.insert("from_a_map", true).unwrap();
    a.commit_then_renew();

    let frontiers = a.state_frontiers();
    let snapshot = a
        .export(ExportMode::ShallowSnapshot(std::borrow::Cow::Owned(
            frontiers,
        )))
        .unwrap();

    let b = doc(2);
    b.import(&snapshot).unwrap();

    // Sanity: Map marker won on B; Text is not the visible kind under "k".
    let value = b.get_deep_value().to_json_value();
    assert!(
        value["state"]["k"].is_object(),
        "Map marker wins on B; got {value}"
    );

    // The loser's Text state must have ridden through the shallow snapshot. Re-requesting it
    // rewrites the marker to Text and resurfaces the preserved "from_a" content.
    let b_text = b.get_map("state").ensure_mergeable_text("k").unwrap();
    assert_eq!(b_text.id(), a_text_id, "deterministic cid is stable");
    assert_eq!(
        b_text.get_value().to_json_value(),
        json!("from_a"),
        "losing-kind state must survive shallow snapshot export"
    );
}

/// Shallow snapshot export should preserve mergeable child state and parent
/// edges on the receiver, the same as a full snapshot.
#[test]
#[cfg(feature = "counter")]
fn shallow_snapshot_roundtrip_preserves_mergeable_child() {
    let a = doc(1);
    let counter = a
        .get_map("state")
        .ensure_mergeable_counter("revision")
        .unwrap();
    counter.increment(4.0).unwrap();
    a.commit_then_renew();

    // ShallowSnapshot at current frontiers.
    let frontiers = a.state_frontiers();
    let snapshot = a
        .export(ExportMode::ShallowSnapshot(std::borrow::Cow::Owned(
            frontiers,
        )))
        .unwrap();

    let b = doc(2);
    b.import(&snapshot).unwrap();
    assert_eq!(
        b.get_deep_value().to_json_value(),
        json!({ "state": { "revision": 4.0 } }),
        "shallow snapshot must carry mergeable child state and logical edge resolution"
    );
}

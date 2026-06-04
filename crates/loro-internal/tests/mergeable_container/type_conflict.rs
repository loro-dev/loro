//! Concurrent different-kind creation, resolved by the parent map's marker LWW.
//!
//! When two peers create different kinds under the same key, each writes a different binary marker
//! into the parent map's slot. The parent map's regular LWW deterministically resolves to one
//! marker, so every peer surfaces the same kind; the loser's container stays reachable only by an
//! explicit `get_mergeable_<kind>` lookup (which rewrites the marker).
//! See loro-dev/loro#759.

#[path = "common.rs"]
mod common;
use common::{doc, sync};

use loro_internal::{cursor::PosType, event::Index, loro::ExportMode, HandlerTrait, ToJson};
use serde_json::json;

/// Requesting a different kind under a key that already holds another kind's marker is a
/// deliberate, local kind change: it rewrites the marker. The new kind becomes active and
/// the new handler is usable. (This is the building block of the List -> Map -> List cycle.)
#[test]
fn local_different_kind_request_rewrites_the_marker() {
    let doc = doc(1);
    let root = doc.get_map("state");

    let text = root.get_mergeable_text("field").unwrap();
    text.insert(0, "hello", PosType::Unicode).unwrap();
    doc.commit_then_renew();
    assert_eq!(
        doc.get_deep_value().to_json_value(),
        json!({ "state": { "field": "hello" } })
    );

    // Switch the kind: this rewrites the marker to Map.
    let map = root.get_mergeable_map("field").unwrap();
    map.insert("k", 1).unwrap();
    doc.commit_then_renew();
    assert_eq!(
        doc.get_deep_value().to_json_value(),
        json!({ "state": { "field": { "k": 1 } } }),
        "requesting a different kind rewrites the marker to that kind"
    );
}

/// When two competing-kind markers land in the same parent's slot under the same key during
/// import, the parent map's regular LWW deterministically picks one. The visible kind is the same
/// on every peer; the loser's container is still reachable by name but not surfaced by default.
#[test]
#[cfg(feature = "counter")]
fn different_kind_collision_resolves_by_map_lww_at_import() {
    // Peer A: text under "k", low lamport.
    let a = doc(1);
    let a_text = a.get_map("state").get_mergeable_text("k").unwrap();
    a_text.insert(0, "from_a", PosType::Unicode).unwrap();
    let a_text_id = a_text.id();
    a.commit_then_renew();

    // Peer B: map under "k", with a later lamport (advance B's clock first).
    let b = doc(2);
    let b_state = b.get_map("state");
    for i in 0..5 {
        b_state.insert(&format!("filler_{i}"), i).unwrap();
        b.commit_then_renew();
    }
    let b_map = b_state.get_mergeable_map("k").unwrap();
    b_map.insert("from_b", true).unwrap();
    let b_map_id = b_map.id();
    b.commit_then_renew();

    // B imports A's snapshot. The Text and Map markers compete in B's map slot for "k"; B's Map
    // marker has the higher lamport, so Map wins.
    let snapshot = a.export(ExportMode::Snapshot).unwrap();
    b.import(&snapshot).unwrap();

    // Check the direct cids before calling any getter. The losing Text cid must not have a logical
    // path while the Map marker wins; the winning Map cid resolves from the marker.
    assert!(
        b.get_path_to_container(&a_text_id).is_none(),
        "old Text cid must be inactive while the Map marker wins"
    );
    let map_path = b
        .get_path_to_container(&b_map_id)
        .expect("winning Map cid must resolve from the parent marker");
    let indexes = map_path
        .iter()
        .map(|(_, index)| index.clone())
        .collect::<Vec<_>>();
    assert_eq!(
        indexes,
        vec![Index::Key("state".into()), Index::Key("k".into())]
    );

    let value = b.get_deep_value().to_json_value();
    let k_value = &value["state"]["k"];
    assert!(
        k_value.is_object(),
        "Map marker won the LWW; got {k_value:?}"
    );
    assert_eq!(k_value["from_b"], json!(true));

    // The loser's Text container is still reachable by an explicit get_mergeable_text — which
    // rewrites the marker back to Text (a local kind change), so it now resurfaces.
    let b_text = b.get_map("state").get_mergeable_text("k").unwrap();
    assert_eq!(b_text.id(), a_text_id);
    assert_eq!(
        b_text.get_value().to_json_value(),
        json!("from_a"),
        "loser's container is preserved and reachable by name"
    );
}

/// Three peers each create a different kind under the same key and mutate it once. After a full
/// round-robin sync, every peer's parent-map LWW resolves to the same marker, so all agree
/// on exactly one visible kind.
#[test]
#[cfg(feature = "counter")]
fn three_peer_different_kind_conflict_converges() {
    let a = doc(1);
    let b = doc(2);
    let c = doc(3);

    let a_text = a.get_map("state").get_mergeable_text("k").unwrap();
    a_text.insert(0, "from_a", PosType::Unicode).unwrap();
    a.commit_then_renew();

    let b_map = b.get_map("state").get_mergeable_map("k").unwrap();
    b_map.insert("from_b", true).unwrap();
    b.commit_then_renew();

    let c_list = c.get_map("state").get_mergeable_list("k").unwrap();
    c_list.insert(0, "from_c").unwrap();
    c.commit_then_renew();

    sync(&a, &b);
    sync(&b, &c);
    sync(&a, &c);
    sync(&a, &b);

    let va = a.get_deep_value().to_json_value();
    let vb = b.get_deep_value().to_json_value();
    let vc = c.get_deep_value().to_json_value();
    assert_eq!(va, vb, "A and B must agree");
    assert_eq!(vb, vc, "B and C must agree");

    // Exactly one kind is visible under "k": Text -> string, Map -> object, List -> array.
    let k = &va["state"]["k"];
    let survivors = [k.is_string(), k.is_object(), k.is_array()];
    let count: usize = survivors.iter().filter(|x| **x).count();
    assert_eq!(
        count, 1,
        "exactly one kind must be visible; got {survivors:?} for value {k:?}"
    );
}

/// Concurrent SAME-kind creation: both peers write the identical marker, so the Map LWW
/// merge is a no-op and both contributions land in the one deterministic cid.
#[test]
fn concurrent_same_kind_creation_converges() {
    let a = doc(1);
    let b = doc(2);

    let a_list = a.get_map("state").get_mergeable_list("k").unwrap();
    a_list.insert(0, "from_a").unwrap();
    a.commit_then_renew();

    let b_list = b.get_map("state").get_mergeable_list("k").unwrap();
    b_list.insert(0, "from_b").unwrap();
    b.commit_then_renew();

    sync(&a, &b);

    let va = a.get_deep_value().to_json_value();
    assert_eq!(va, b.get_deep_value().to_json_value(), "peers converge");
    let items: Vec<&str> = va["state"]["k"]
        .as_array()
        .expect("k is a list")
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert!(
        items.contains(&"from_a") && items.contains(&"from_b"),
        "both contributions preserved; got {items:?}"
    );
}

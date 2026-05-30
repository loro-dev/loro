//! Delete semantics for mergeable children.
//!
//! `delete(key)` overwrites the `"🤝:<kind>"` discriminator in the parent map's value slot with
//! `None`, exactly as deleting a regular child container clears its value-table entry. The child
//! becomes unreachable; its container state is preserved in history. Re-calling
//! `get_mergeable_<kind>(key)` rewrites the discriminator and resurfaces the preserved state, and
//! Map LWW resolves any concurrent delete-vs-recreate race — symmetric with normal Loro
//! write/delete (loro-dev/loro#759).

#[path = "common.rs"]
mod common;
use common::{doc, sync};

use loro_internal::{loro::ExportMode, HandlerTrait, ToJson};
use serde_json::json;

/// `delete(key)` on a mergeable key clears the discriminator, so the child no longer appears in
/// deep value. The child's container state is preserved: re-getting it resolves to the same
/// deterministic cid and resurfaces the prior value (delete detaches, it does not reset).
#[test]
#[cfg(feature = "counter")]
fn delete_clears_discriminator_and_recreate_resurfaces_state() {
    let doc = doc(1);
    let root = doc.get_map("state");
    let counter = root.get_mergeable_counter("revision").unwrap();
    counter.increment(3.0).unwrap();
    doc.commit_then_renew();
    assert_eq!(
        doc.get_deep_value().to_json_value(),
        json!({ "state": { "revision": 3.0 } })
    );

    root.delete("revision").unwrap();
    doc.commit_then_renew();

    // Cleared: not in deep value, exactly like any deleted container.
    assert_eq!(
        doc.get_deep_value().to_json_value(),
        json!({ "state": {} }),
        "delete must clear the mergeable child from deep value"
    );

    // Re-get rewrites the discriminator. The child resurfaces with its PRESERVED value (3.0),
    // not a reset to 0.0 — delete detaches, it does not destroy the container state.
    let counter2 = root.get_mergeable_counter("revision").unwrap();
    doc.commit_then_renew();
    assert_eq!(counter2.id(), counter.id(), "deterministic cid is stable");
    assert_eq!(
        doc.get_deep_value().to_json_value(),
        json!({ "state": { "revision": 3.0 } }),
        "re-get rewrites the discriminator and resurfaces the preserved state"
    );
}

/// `delete(key)` records nothing special in side state — the slot value (`None`) is the whole
/// story. After delete, the parent map simply has no value at the key.
#[test]
#[cfg(feature = "counter")]
fn delete_leaves_no_value_at_key() {
    let doc = doc(1);
    let root = doc.get_map("state");
    let counter = root.get_mergeable_counter("revision").unwrap();
    counter.increment(1.0).unwrap();
    doc.commit_then_renew();
    assert!(root.get("revision").is_some(), "discriminator present");

    root.delete("revision").unwrap();
    doc.commit_then_renew();

    assert_eq!(
        root.get("revision"),
        None,
        "after delete the slot holds no value"
    );
}

/// A local `delete` clears the child from deep value immediately, without waiting for any
/// remote import cycle.
#[test]
#[cfg(feature = "counter")]
fn local_delete_immediately_clears_mergeable_child() {
    let doc = doc(1);
    let root = doc.get_map("state");
    let counter = root.get_mergeable_counter("revision").unwrap();
    counter.increment(1.0).unwrap();
    doc.commit_then_renew();
    assert_eq!(
        doc.get_deep_value().to_json_value(),
        json!({ "state": { "revision": 1.0 } })
    );

    root.delete("revision").unwrap();
    doc.commit_then_renew();

    assert_eq!(
        doc.get_deep_value().to_json_value(),
        json!({ "state": {} }),
        "local delete must remove the mergeable child from deep value"
    );
}

/// Recreate after seeing a delete wins: peer A deletes the key; peer B imports the delete (its
/// slot is now cleared), then re-calls `get_mergeable_<kind>`, which re-emits a FRESH discriminator
/// because the slot is empty. That discriminator's IdLp dominates the delete, so the child is
/// visible again and its preserved state resurfaces — symmetric with re-`set_container` after a
/// regular delete.
///
/// (Note the symmetry: if B re-calls `get_mergeable_<kind>` while still holding the OLD
/// discriminator it imported earlier — i.e. before seeing the delete — the call is idempotent and
/// emits no op, so the concurrent delete wins, exactly as a regular concurrent delete beats a
/// stale local handle. That case is covered by `concurrent_delete_wins_against_earlier_increment`.)
#[test]
#[cfg(feature = "counter")]
fn recreate_after_seen_delete_resurfaces_preserved_state() {
    let a = doc(1);
    let b = doc(2);

    let a_root = a.get_map("state");
    let a_counter = a_root.get_mergeable_counter("revision").unwrap();
    a_counter.increment(1.0).unwrap();
    a.commit_then_renew();
    sync(&a, &b);

    // A deletes; B imports the delete so B's slot is cleared.
    a_root.delete("revision").unwrap();
    a.commit_then_renew();
    sync(&a, &b);
    assert_eq!(
        b.get_deep_value().to_json_value(),
        json!({ "state": {} }),
        "B sees the cleared slot after importing the delete"
    );

    // B re-creates: the slot is empty, so this re-emits a fresh discriminator dominating the
    // delete. The preserved counter state (1.0) resurfaces, plus B's new increment.
    let b_counter = b
        .get_map("state")
        .get_mergeable_counter("revision")
        .unwrap();
    b_counter.increment(100.0).unwrap();
    b.commit_then_renew();

    sync(&a, &b);

    let va = a.get_deep_value().to_json_value();
    let vb = b.get_deep_value().to_json_value();
    assert_eq!(va, vb, "peers must converge");
    assert_eq!(
        va["state"]["revision"],
        json!(101.0),
        "recreate after seeing the delete resurfaces preserved state (1.0) + new increment (100.0)"
    );
}

/// Concurrent delete-wins: peer A increments, peer B deletes with a higher IdLp and never
/// recreates. The delete dominates the discriminator via Map LWW, so the child is gone on both
/// peers — symmetric with a concurrent delete winning over a regular write.
#[test]
#[cfg(feature = "counter")]
fn concurrent_delete_wins_against_earlier_increment() {
    let a = doc(1);
    let b = doc(2);

    let a_root = a.get_map("state");
    let a_counter = a_root.get_mergeable_counter("revision").unwrap();
    a_counter.increment(1.0).unwrap();
    a.commit_then_renew();
    sync(&a, &b);

    // B increments first (low IdLp).
    let b_root = b.get_map("state");
    let b_counter = b_root.get_mergeable_counter("revision").unwrap();
    b_counter.increment(100.0).unwrap();
    b.commit_then_renew();

    // A advances its clock past B's increment, then deletes.
    for i in 0..5 {
        a_root.insert(&format!("noise_{i}"), i).unwrap();
        a.commit_then_renew();
    }
    a_root.delete("revision").unwrap();
    a.commit_then_renew();

    sync(&a, &b);

    let va = a.get_deep_value().to_json_value();
    let vb = b.get_deep_value().to_json_value();
    assert_eq!(va, vb);
    assert!(
        va["state"].get("revision").is_none(),
        "delete with higher IdLp must dominate the discriminator; got {va}"
    );
}

/// Remote delete clears the child on the receiver: peer B has a visible mergeable counter; peer A
/// deletes the key with a higher IdLp. After sync, B's deep value drops the counter because A's
/// delete wins the Map LWW for the discriminator slot.
#[test]
#[cfg(feature = "counter")]
fn remote_delete_clears_mergeable_child_on_receiver() {
    let a = doc(1);
    let b = doc(2);

    let b_root = b.get_map("state");
    let b_counter = b_root.get_mergeable_counter("revision").unwrap();
    b_counter.increment(1.0).unwrap();
    b.commit_then_renew();
    sync(&a, &b);
    assert_eq!(
        b.get_deep_value().to_json_value(),
        json!({ "state": { "revision": 1.0 } })
    );

    let a_root = a.get_map("state");
    for i in 0..5 {
        a_root.insert(&format!("noise_{i}"), i).unwrap();
        a.commit_then_renew();
    }
    a_root.delete("revision").unwrap();
    a.commit_then_renew();

    sync(&a, &b);

    let vb = b.get_deep_value().to_json_value();
    assert!(
        vb["state"].get("revision").is_none(),
        "remote delete must clear B's mergeable child; got {vb}"
    );
}

/// Three-peer convergence across a delete and a competing-kind recreate: B creates a counter, A
/// deletes it with a higher IdLp, C recreates the key as a text. All peers converge on the text.
#[test]
#[cfg(feature = "counter")]
fn three_peer_delete_then_recreate_converges() {
    use loro_internal::cursor::PosType;

    let a = doc(1);
    let b = doc(2);
    let c = doc(3);

    let b_counter = b
        .get_map("state")
        .get_mergeable_counter("revision")
        .unwrap();
    b_counter.increment(1.0).unwrap();
    b.commit_then_renew();

    sync(&a, &b);
    sync(&b, &c);

    let a_root = a.get_map("state");
    for i in 0..6 {
        a_root.insert(&format!("noise_{i}"), i).unwrap();
        a.commit_then_renew();
    }
    a_root.delete("revision").unwrap();
    a.commit_then_renew();

    sync(&a, &b);
    sync(&a, &c);

    // C recreates "revision" as a text with a post-delete IdLp.
    let c_text = c.get_map("state").get_mergeable_text("revision").unwrap();
    c_text.insert(0, "after-delete", PosType::Unicode).unwrap();
    c.commit_then_renew();

    sync(&b, &c);
    sync(&a, &c);
    sync(&a, &b);

    let expected = json!({
        "state": {
            "noise_0": 0,
            "noise_1": 1,
            "noise_2": 2,
            "noise_3": 3,
            "noise_4": 4,
            "noise_5": 5,
            "revision": "after-delete",
        }
    });
    assert_eq!(a.get_deep_value().to_json_value(), expected, "A converges");
    assert_eq!(b.get_deep_value().to_json_value(), expected, "B converges");
    assert_eq!(c.get_deep_value().to_json_value(), expected, "C converges");
}

/// Snapshot round-trip preserves the delete: a peer creates and deletes a mergeable counter, then
/// exports a snapshot. The receiver sees no counter — the `None` slot rides through the snapshot
/// as ordinary map state, so no special recovery is needed.
#[test]
#[cfg(feature = "counter")]
fn snapshot_roundtrip_preserves_delete() {
    let a = doc(1);
    let a_root = a.get_map("state");
    let a_counter = a_root.get_mergeable_counter("revision").unwrap();
    a_counter.increment(5.0).unwrap();
    a_root.delete("revision").unwrap();
    a.commit_then_renew();
    assert_eq!(a.get_deep_value().to_json_value(), json!({ "state": {} }));

    let snapshot = a.export(ExportMode::Snapshot).unwrap();
    let b = doc(2);
    b.import(&snapshot).unwrap();

    assert_eq!(
        b.get_deep_value().to_json_value(),
        json!({ "state": {} }),
        "snapshot import must preserve the cleared slot; no mergeable child appears"
    );
    assert_eq!(
        b.get_map("state").get("revision"),
        None,
        "the cleared slot has no value after snapshot import"
    );
}

/// `delete` on a mergeable key emits exactly one op — a regular `MapSet { value: None }` — by the
/// local peer. No new op types are introduced; older peers apply it as an ordinary map-key clear.
#[test]
#[cfg(feature = "counter")]
fn delete_on_mergeable_key_emits_only_existing_op_types() {
    let doc = doc(1);
    let root = doc.get_map("state");
    let counter = root.get_mergeable_counter("revision").unwrap();
    counter.increment(1.0).unwrap();
    doc.commit_then_renew();
    let counter_before = doc.oplog_vv().get(&1).copied().unwrap_or(0);

    root.delete("revision").unwrap();
    doc.commit_then_renew();
    let counter_after = doc.oplog_vv().get(&1).copied().unwrap_or(0);

    let new_ops = counter_after - counter_before;
    assert_eq!(
        new_ops, 1,
        "delete must emit exactly one op (the MapSet clearing the slot); got {new_ops}"
    );
}

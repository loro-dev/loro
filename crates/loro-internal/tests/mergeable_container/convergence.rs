//! Concurrent first-create convergence and basic merging.

#[path = "common.rs"]
mod common;
use common::{doc, sync};

use loro_internal::{cursor::PosType, event::Index, handler::ValueOrHandler, HandlerTrait, ToJson};
use serde_json::json;

#[test]
#[cfg(feature = "counter")]
fn concurrent_counter_increments_show_current_lost_update_bug() {
    let a = doc(1);
    let b = doc(2);

    let a_root = a.get_map("state");
    let b_root = b.get_map("state");

    let a_counter = a_root.ensure_mergeable_counter("revision").unwrap();
    let b_counter = b_root.ensure_mergeable_counter("revision").unwrap();

    assert_eq!(
        a_counter.id(),
        b_counter.id(),
        "both peers should produce the same deterministic cid"
    );
    assert!(
        a_counter.id().is_mergeable(),
        "counter cid should be in the mergeable namespace"
    );

    a_counter.increment(1.0).unwrap();
    b_counter.increment(1.0).unwrap();

    sync(&a, &b);

    assert_eq!(
        a.get_deep_value().to_json_value(),
        json!({ "state": { "revision": 2.0 } }),
        "both concurrent increments should survive on the merged counter",
    );
    assert_eq!(
        b.get_deep_value().to_json_value(),
        a.get_deep_value().to_json_value()
    );

    let a_cid = match a_root.get_("revision") {
        Some(ValueOrHandler::Handler(handler)) => Some(handler.id()),
        _ => None,
    };
    let b_cid = match b_root.get_("revision") {
        Some(ValueOrHandler::Handler(handler)) => Some(handler.id()),
        _ => None,
    };
    assert_eq!(
        a_cid, b_cid,
        "both peers should resolve the same logical counter cid"
    );
}

#[test]
fn concurrent_text_updates_show_current_lost_update_bug() {
    let a = doc(1);
    let b = doc(2);
    let a_text = a.get_map("state").ensure_mergeable_text("notes").unwrap();
    let b_text = b.get_map("state").ensure_mergeable_text("notes").unwrap();

    assert_eq!(
        a_text.id(),
        b_text.id(),
        "both peers should produce the same deterministic cid"
    );
    assert!(
        a_text.id().is_mergeable(),
        "text cid should be in the mergeable namespace"
    );

    a_text.insert(0, "A", PosType::Unicode).unwrap();
    b_text.insert(0, "B", PosType::Unicode).unwrap();
    sync(&a, &b);

    let value = a.get_deep_value().to_json_value();
    assert!(
        value == json!({ "state": { "notes": "AB" } })
            || value == json!({ "state": { "notes": "BA" } }),
        "both concurrent text edits should survive on the merged text; got {value}",
    );
}

#[test]
fn concurrent_list_inserts_show_current_lost_update_bug() {
    let a = doc(1);
    let b = doc(2);
    let a_list = a.get_map("state").ensure_mergeable_list("items").unwrap();
    let b_list = b.get_map("state").ensure_mergeable_list("items").unwrap();

    assert_eq!(
        a_list.id(),
        b_list.id(),
        "both peers should produce the same deterministic cid"
    );
    assert!(
        a_list.id().is_mergeable(),
        "list cid should be in the mergeable namespace"
    );

    a_list.insert(0, "A").unwrap();
    b_list.insert(0, "B").unwrap();
    sync(&a, &b);

    let value = a.get_deep_value().to_json_value();
    assert!(
        value == json!({ "state": { "items": ["A", "B"] } })
            || value == json!({ "state": { "items": ["B", "A"] } }),
        "both concurrent list inserts should survive on the merged list; got {value}",
    );
}

/// Two peers each obtain the "profile" Map via `ensure_mergeable_map` and write
/// to *different* keys. With non-mergeable child Maps, each peer creates a
/// distinct peer-specific cid, so LWW drops one peer's Map entirely even
/// though the key sets are disjoint. Once child Maps are mergeable the merged
/// value should contain both keys.
#[test]
fn concurrent_map_writes_show_current_lost_update_bug() {
    let a = doc(1);
    let b = doc(2);
    let a_map = a.get_map("state").ensure_mergeable_map("profile").unwrap();
    let b_map = b.get_map("state").ensure_mergeable_map("profile").unwrap();

    assert_eq!(
        a_map.id(),
        b_map.id(),
        "both peers should produce the same deterministic cid"
    );
    assert!(
        a_map.id().is_mergeable(),
        "map cid should be in the mergeable namespace"
    );

    a_map.insert("name", "Ada").unwrap();
    b_map.insert("title", "Engineer").unwrap();
    sync(&a, &b);

    assert_eq!(
        a.get_deep_value().to_json_value(),
        json!({ "state": { "profile": { "name": "Ada", "title": "Engineer" } } })
    );
}

/// Three peers each increment the same mergeable counter once. After a full
/// round-robin sync, every peer must observe `3.0` and the same deterministic
/// cid. Two-peer tests can't catch ordering or idempotency bugs that fire
/// only when more than two histories overlap.
#[test]
#[cfg(feature = "counter")]
fn three_peer_mergeable_counter_convergence() {
    let a = doc(1);
    let b = doc(2);
    let c = doc(3);

    let a_counter = a
        .get_map("state")
        .ensure_mergeable_counter("revision")
        .unwrap();
    let b_counter = b
        .get_map("state")
        .ensure_mergeable_counter("revision")
        .unwrap();
    let c_counter = c
        .get_map("state")
        .ensure_mergeable_counter("revision")
        .unwrap();
    assert_eq!(a_counter.id(), b_counter.id());
    assert_eq!(b_counter.id(), c_counter.id());

    a_counter.increment(1.0).unwrap();
    b_counter.increment(1.0).unwrap();
    c_counter.increment(1.0).unwrap();

    // Full round-robin: every pair syncs.
    sync(&a, &b);
    sync(&b, &c);
    sync(&a, &c);
    sync(&a, &b);

    let expected = json!({ "state": { "revision": 3.0 } });
    assert_eq!(a.get_deep_value().to_json_value(), expected);
    assert_eq!(b.get_deep_value().to_json_value(), expected);
    assert_eq!(c.get_deep_value().to_json_value(), expected);
}

/// After A and B sync once, both peers concurrently mutate the same
/// mergeable counter again, then sync. Convergence must hold on the second
/// round — the deterministic cid plus CRDT merge must keep working after
/// the logical mergeable edge has already been resolved and used.
#[test]
#[cfg(feature = "counter")]
fn post_merge_concurrent_counter_increments_converge() {
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
    a_counter.increment(1.0).unwrap();
    b_counter.increment(1.0).unwrap();
    sync(&a, &b);
    assert_eq!(
        a.get_deep_value().to_json_value(),
        json!({ "state": { "revision": 2.0 } })
    );

    // Round 2: concurrent edits on the already-merged child.
    a_counter.increment(10.0).unwrap();
    b_counter.increment(100.0).unwrap();
    sync(&a, &b);

    let expected = json!({ "state": { "revision": 112.0 } });
    assert_eq!(a.get_deep_value().to_json_value(), expected);
    assert_eq!(b.get_deep_value().to_json_value(), expected);
}

/// Two peers independently navigate `state → mergeable map "profile" →
/// mergeable counter "revision"` and increment. The deterministic cid for
/// "revision" is the same on both peers (it's a function of the "profile"
/// cid, which is itself deterministic from "state"'s cid + "profile" + Map).
/// After sync, both peers see the counter at 2.0 nested correctly.
#[test]
#[cfg(feature = "counter")]
fn nested_mergeable_concurrent_counter_converges() {
    let a = doc(1);
    let b = doc(2);

    let a_profile = a.get_map("state").ensure_mergeable_map("profile").unwrap();
    let b_profile = b.get_map("state").ensure_mergeable_map("profile").unwrap();
    assert_eq!(a_profile.id(), b_profile.id());

    let a_rev = a_profile.ensure_mergeable_counter("revision").unwrap();
    let b_rev = b_profile.ensure_mergeable_counter("revision").unwrap();
    assert_eq!(a_rev.id(), b_rev.id());

    a_rev.increment(1.0).unwrap();
    b_rev.increment(1.0).unwrap();
    sync(&a, &b);

    let expected = json!({ "state": { "profile": { "revision": 2.0 } } });
    assert_eq!(a.get_deep_value().to_json_value(), expected);
    assert_eq!(b.get_deep_value().to_json_value(), expected);

    // Path resolution still walks both mergeable hops.
    let path = a.get_path_to_container(&a_rev.id()).expect("path");
    let indexes = path.iter().map(|(_, idx)| idx.clone()).collect::<Vec<_>>();
    assert_eq!(
        indexes,
        vec![
            Index::Key("state".into()),
            Index::Key("profile".into()),
            Index::Key("revision".into()),
        ]
    );
}

/// `ensure_mergeable_*` cannot work on a detached handler: the deterministic cid is computed from
/// the parent's cid, which a detached parent doesn't have. Falling back to a non-deterministic
/// regular child would silently break the mergeable guarantee once the handler attaches, so the
/// detached call must surface that misuse explicitly instead.
#[test]
#[cfg(feature = "counter")]
fn detached_map_ensure_mergeable_counter_rejects_misuse() {
    use loro_internal::MapHandler;
    let detached = MapHandler::new_detached();
    let err = detached
        .ensure_mergeable_counter("revision")
        .expect_err("detached ensure_mergeable_* must error");
    assert!(
        matches!(err, loro_common::LoroError::MisuseDetachedContainer { .. }),
        "detached error must be MisuseDetachedContainer; got {err:?}"
    );
}

//! A remote change can depend on its own peer only through the previous op of
//! that peer. Other deps on its own peer are import errors, before anything is
//! applied.

use loro::{CommitOptions, ExportMode, JsonSchema, LoroDoc, LoroError, VersionVector, ID};

/// Alice (peer 1) writes three changes at counters 0, 1 and 2. Bob (peer 2)
/// writes one change between her second and third, so her change at counter 1
/// has a successor in the DAG.
fn alice_and_bob() -> JsonSchema {
    let alice = LoroDoc::new();
    alice.set_peer_id(1).unwrap();
    for k in ["a", "b"] {
        alice.get_map("m").insert(k, k).unwrap();
        alice.commit_with(CommitOptions::new().commit_msg(k));
    }
    let bob = LoroDoc::new();
    bob.set_peer_id(2).unwrap();
    bob.import(&alice.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    bob.get_map("m").insert("bob", 1).unwrap();
    bob.commit();
    alice
        .import(&bob.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    alice.get_map("m").insert("c", "c").unwrap();
    alice.commit_with(CommitOptions::new().commit_msg("c"));
    alice
        .export_json_updates_without_peer_compression(&VersionVector::default(), &alice.oplog_vv())
        .unwrap()
}

fn index_of(json: &JsonSchema, id: ID) -> usize {
    json.changes.iter().position(|c| c.id == id).unwrap()
}

/// Imports `json` into a new doc and checks that the import fails with a
/// decode error, leaves the doc empty, and the doc stays usable.
fn assert_rejected(json: JsonSchema, detached: bool) {
    let doc = LoroDoc::new();
    if detached {
        doc.detach();
    }
    let err = doc.import_json_updates(json).unwrap_err();
    assert!(matches!(err, LoroError::DecodeError(_)), "{err:?}");
    assert!(doc.oplog_vv().is_empty());
    doc.checkout_to_latest();
    doc.get_map("x").insert("still", "usable").unwrap();
    doc.commit();
    let copy = LoroDoc::new();
    copy.import(&doc.export(ExportMode::Snapshot).unwrap())
        .unwrap();
    assert_eq!(copy.get_deep_value(), doc.get_deep_value());
}

/// Alice's change `2@1` claims a dep on her older op `0@1`. It used to panic
/// with "lamport is not continuous" inside the import.
#[test]
fn dep_on_an_older_op_of_the_own_peer_is_rejected() {
    let honest = alice_and_bob();
    let c = index_of(&honest, ID::new(1, 2));
    let cases = [
        (false, vec![ID::new(1, 0)]),
        (true, vec![ID::new(1, 0)]),
        (true, vec![ID::new(1, 0), ID::new(2, 0)]),
    ];
    for (with_bob, deps) in cases {
        let mut json = honest.clone();
        json.changes[c].deps = deps;
        if !with_bob {
            json.changes.retain(|c| c.id.peer == 1);
        }
        for detached in [false, true] {
            assert_rejected(json.clone(), detached);
        }
    }
}

/// Deps on the change's own counter or on a later counter of its peer, and a
/// first change that depends on its own peer, are rejected too.
#[test]
fn other_deps_on_the_own_peer_are_rejected() {
    let honest = alice_and_bob();
    let c = index_of(&honest, ID::new(1, 1));
    for dep in [ID::new(1, 1), ID::new(1, 2), ID::new(1, 7)] {
        let mut json = honest.clone();
        json.changes[c].deps = vec![dep];
        assert_rejected(json, false);
    }
    let mut json = honest.clone();
    json.changes[index_of(&honest, ID::new(1, 0))].deps = vec![ID::new(1, 0)];
    assert_rejected(json, false);

    let doc = LoroDoc::new();
    doc.import_json_updates(honest).unwrap();
    assert_eq!(doc.get_map("m").len(), 4);
}

/// A rejected change rejects the whole update, also the valid changes before
/// it, and the doc keeps what it had.
#[test]
fn a_rejected_change_rejects_the_whole_update() {
    let honest = alice_and_bob();
    let doc = LoroDoc::new();
    let mut first = honest.clone();
    first.changes.truncate(1);
    doc.import_json_updates(first).unwrap();
    let vv = doc.oplog_vv();
    let value = doc.get_deep_value();

    let mut json = honest.clone();
    let c = index_of(&json, ID::new(1, 2));
    json.changes[c].deps = vec![ID::new(1, 0)];
    assert!(doc.import_json_updates(json).is_err());
    assert_eq!(doc.oplog_vv(), vv);
    assert_eq!(doc.get_deep_value(), value);

    doc.import_json_updates(honest).unwrap();
    assert_eq!(doc.get_map("m").len(), 4);
}

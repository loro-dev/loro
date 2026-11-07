//! A change that waits as pending because its deps are missing gets its
//! lamport from its deps once they arrive, not the lamport it carried in the
//! update.

use loro::{CommitOptions, ExportMode, JsonChange, JsonSchema, LoroDoc, VersionVector, ID};

/// Base change of peer 10, then Alice (peer 1, two changes) and Bob (peer 2)
/// set `m.k` concurrently. Bob wins with the honest lamports. Returns the
/// honest doc and its JSON updates.
fn conflict() -> (LoroDoc, JsonSchema) {
    let z = LoroDoc::new();
    z.set_peer_id(10).unwrap();
    z.get_map("m").insert("z", 1).unwrap();
    z.commit();
    let base = z.export(ExportMode::all_updates()).unwrap();
    let alice = LoroDoc::new();
    alice.set_peer_id(1).unwrap();
    alice.import(&base).unwrap();
    alice.get_map("m").insert("k", "alice").unwrap();
    alice.commit_with(CommitOptions::new().commit_msg("a0"));
    alice.get_map("m").insert("j", "alice").unwrap();
    alice.commit_with(CommitOptions::new().commit_msg("a1"));
    let bob = LoroDoc::new();
    bob.set_peer_id(2).unwrap();
    bob.import(&base).unwrap();
    bob.get_map("m").insert("k", "bob").unwrap();
    bob.commit();
    let honest = LoroDoc::new();
    honest
        .import(&alice.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    honest
        .import(&bob.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    let json = honest
        .export_json_updates_without_peer_compression(&VersionVector::default(), &honest.oplog_vv())
        .unwrap();
    (honest, json)
}

/// The `n`th change of `peer` in `json`.
fn pick(json: &JsonSchema, peer: u64, n: usize) -> JsonChange {
    json.changes
        .iter()
        .filter(|c| c.id.peer == peer)
        .nth(n)
        .unwrap()
        .clone()
}

/// `json` with only `changes`, in that order.
fn with_changes(json: &JsonSchema, changes: Vec<JsonChange>) -> JsonSchema {
    let mut s = json.clone();
    s.changes = changes;
    s
}

fn value_of_k(doc: &LoroDoc) -> loro::LoroValue {
    doc.get_map("m").get("k").unwrap().into_value().unwrap()
}

/// Alice's first change claims lamport 1000 and comes before its dep, so it
/// waits as pending. It used to keep lamport 1000 and win the conflict on
/// `m.k`, while the same change imported after its dep gets lamport 1.
#[test]
fn pending_change_gets_its_lamport_from_its_deps() {
    let (honest, json) = conflict();
    assert_eq!(value_of_k(&honest), "bob".into());
    let mut a0 = pick(&json, 1, 0);
    a0.lamport = 1000;
    let forged = with_changes(
        &json,
        vec![a0, pick(&json, 10, 0), pick(&json, 2, 0), pick(&json, 1, 1)],
    );
    for detached in [false, true] {
        let doc = LoroDoc::new();
        if detached {
            doc.detach();
        }
        let status = doc.import_json_updates(forged.clone()).unwrap();
        assert!(status.pending.is_none());
        assert_eq!(doc.oplog_vv(), honest.oplog_vv());
        doc.checkout_to_latest();
        assert_eq!(doc.get_change(ID::new(1, 0)).unwrap().lamport, 1);
        assert_eq!(value_of_k(&doc), "bob".into(), "detached {detached}");
    }
}

/// Peer 1 commits twice. The second change claims lamport 1000 and comes
/// before the first. When the first arrives, the DAG merges the two changes
/// into one node, which used to panic with "lamport is not continuous".
#[test]
fn pending_change_with_a_raised_lamport_does_not_panic() {
    let a = LoroDoc::new();
    a.set_peer_id(1).unwrap();
    a.get_map("m").insert("k", 1).unwrap();
    a.commit_with(CommitOptions::new().commit_msg("1"));
    a.get_map("m").insert("k", 2).unwrap();
    a.commit_with(CommitOptions::new().commit_msg("2"));
    let json = a
        .export_json_updates_without_peer_compression(&VersionVector::default(), &a.oplog_vv())
        .unwrap();
    let mut second = pick(&json, 1, 1);
    second.lamport = 1000;
    let doc = LoroDoc::new();
    doc.import_json_updates(with_changes(&json, vec![second]))
        .unwrap();
    doc.import_json_updates(with_changes(&json, vec![pick(&json, 1, 0)]))
        .unwrap();
    assert_eq!(doc.get_change(ID::new(1, 1)).unwrap().lamport, 1);
    assert_eq!(doc.get_deep_value(), a.get_deep_value());
}

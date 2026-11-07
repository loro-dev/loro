//! Imports while a manual [`Transaction`] is open return `ImportWhenInTxn`. The
//! transaction's ops are in the version vector but not in the change store, and it took
//! its next id before the import. See `context/import-peer-id-reuse.md`.

use loro_common::{IdSpan, LoroError, LoroResult, LoroValue};
use loro_internal::{
    cursor::PosType,
    loro::{CommitOptions, ExportMode},
    txn::Transaction,
    HandlerTrait, LoroDoc, ToJson,
};

fn writer(peer: u64) -> LoroDoc {
    let doc = LoroDoc::new_auto_commit();
    doc.set_peer_id(peer).unwrap();
    doc
}

fn all_updates(doc: &LoroDoc) -> Vec<u8> {
    doc.export(ExportMode::all_updates()).unwrap()
}

/// `1@0`, and `7@0..2` on top of it.
struct Parked {
    dep: Vec<u8>,
    update: Vec<u8>,
}

fn parked() -> Parked {
    let source = writer(1);
    source
        .get_text("t")
        .insert(0, &"a".repeat(100), PosType::Unicode)
        .unwrap();
    source.commit_then_renew();
    let dep = all_updates(&source);
    let doc = writer(7);
    doc.import(&dep).unwrap();
    doc.get_text("t").insert(0, "bc", PosType::Unicode).unwrap();
    doc.commit_then_renew();
    let update = doc
        .export(ExportMode::updates_in_range(vec![IdSpan::new(7, 0, 2)]))
        .unwrap();
    Parked { dep, update }
}

fn assert_import_rejected(result: LoroResult<impl std::fmt::Debug>) {
    match result {
        Err(LoroError::ImportWhenInTxn) => {}
        other => panic!("expected ImportWhenInTxn, got {other:?}"),
    }
}

/// The import used to park the update, and the commit took `7@0`. The dep then applied
/// the rest of the update over it: `lamport is not continuous` under the op log lock.
#[test]
fn import_that_would_park_is_rejected_in_a_transaction() {
    let Parked { dep, update } = parked();
    let doc = LoroDoc::new();
    doc.set_peer_id(7).unwrap();
    let mut txn = doc.txn().unwrap();
    assert_import_rejected(doc.import(&update));
    doc.get_text("t")
        .insert_with_txn(&mut txn, 0, "x", PosType::Unicode)
        .unwrap();
    txn.commit().unwrap();

    doc.import(&dep).unwrap();
    assert_eq!(
        doc.get_text("t").get_value().as_string().unwrap().len(),
        101
    );
    match doc.import(&update) {
        Err(LoroError::UsedOpID { .. }) => {}
        other => panic!("expected UsedOpID, got {other:?}"),
    }
}

/// `7@0` is in the version vector and not in the change store, so the comparison
/// skipped it and the import dropped the other `7@0` with `Ok`.
#[test]
fn import_over_uncommitted_ops_is_rejected() {
    let other = writer(7);
    other.get_map("m").insert("k", 2).unwrap();
    other.commit_then_renew();

    let doc = LoroDoc::new();
    doc.set_peer_id(7).unwrap();
    let mut txn = doc.txn().unwrap();
    doc.get_map("m")
        .insert_with_txn(&mut txn, "k", LoroValue::from(1))
        .unwrap();
    assert_import_rejected(doc.import(&all_updates(&other)));
    txn.commit().unwrap();

    match doc.import(&all_updates(&other)) {
        Err(LoroError::UsedOpID { .. }) => {}
        other => panic!("expected UsedOpID, got {other:?}"),
    }
}

/// It used to panic in state apply under the op log lock, and abort on drop.
#[test]
fn every_import_entry_is_rejected_in_a_transaction() {
    let other = writer(5);
    other.get_map("m").insert("k", 2).unwrap();
    other.commit_then_renew();
    let updates = all_updates(&other);
    let snapshot = other.export(ExportMode::Snapshot).unwrap();
    let json = other
        .export_json_updates(&Default::default(), &other.oplog_vv(), false)
        .unwrap();

    let doc = LoroDoc::new();
    doc.set_peer_id(7).unwrap();
    let mut txn = doc.txn().unwrap();
    assert_import_rejected(doc.import(&updates));
    assert_import_rejected(doc.import(&snapshot));
    assert_import_rejected(doc.import_json_updates(json));
    assert_import_rejected(doc.import_batch(&[updates.clone(), snapshot.clone()]));
    assert_import_rejected(doc.import_batch(&[updates.clone()]));
    doc.get_map("m")
        .insert_with_txn(&mut txn, "local", LoroValue::from(1))
        .unwrap();
    txn.commit().unwrap();

    doc.import(&updates).unwrap();
    assert_eq!(
        doc.get_deep_value().to_json_value()["m"]
            .as_object()
            .map(|m| m.len()),
        Some(2)
    );
}

/// An auto-commit doc with no pending auto-commit transaction, so a manual one can
/// open: the barrier of each import then renews the auto-commit one. That used to
/// panic in `auto_commit_txn`, poisoning the transaction lock.
fn auto_commit_doc_without_its_transaction() -> LoroDoc {
    let doc = LoroDoc::new_auto_commit();
    doc.commit_with(CommitOptions::new().immediate_renew(false));
    doc
}

#[test]
fn import_in_a_transaction_of_an_auto_commit_doc_is_rejected() {
    let other = writer(5);
    other.get_map("m").insert("k", 2).unwrap();
    other.commit_then_renew();
    let updates = all_updates(&other);

    let doc = auto_commit_doc_without_its_transaction();
    let txn = doc.txn().unwrap();
    assert_import_rejected(doc.import(&updates));
    assert_import_rejected(doc.import_batch(&[updates.clone(), updates.clone()]));
    match doc.get_map("m").insert("k", 1) {
        Err(LoroError::DuplicatedTransactionError) => {}
        other => panic!("expected DuplicatedTransactionError, got {other:?}"),
    }
    txn.commit().unwrap();

    doc.import(&updates).unwrap();
    doc.get_map("n").insert("k", 1).unwrap();
    doc.commit_then_renew();
}

/// The batch used to detach and open its rollback scope first. Closing it renewed the
/// peer id for detached editing, which a transaction forbids: a panic with the scope
/// still open.
#[test]
fn batch_in_a_transaction_with_detached_editing_is_rejected() {
    let other = writer(5);
    other.get_map("m").insert("k", 2).unwrap();
    other.commit_then_renew();
    let updates = all_updates(&other);

    for doc in [LoroDoc::new(), auto_commit_doc_without_its_transaction()] {
        doc.set_detached_editing(true);
        let txn = doc.txn().unwrap();
        assert_import_rejected(doc.import_batch(&[updates.clone(), updates.clone()]));
        assert!(!doc.is_detached());
        txn.commit().unwrap();
        let status = doc
            .import_batch(&[updates.clone(), updates.clone()])
            .unwrap();
        assert!(status.pending.is_none());
    }
}

/// A transaction cannot start under parked history, as an auto-commit write cannot.
#[test]
fn transaction_cannot_start_under_parked_ids() {
    let Parked { dep, update } = parked();
    let doc = LoroDoc::new();
    doc.set_peer_id(7).unwrap();
    doc.import(&update).unwrap();
    match doc.txn() {
        Err(LoroError::ConcurrentOpsWithSamePeerID { peer: 7, .. }) => {}
        other => panic!(
            "expected ConcurrentOpsWithSamePeerID, got {:?}",
            other.err()
        ),
    }
    doc.import(&dep).unwrap();
    assert!(doc.txn().is_ok());
}

fn permutations(items: &[usize]) -> Vec<Vec<usize>> {
    if items.len() <= 1 {
        return vec![items.to_vec()];
    }
    let mut all = Vec::new();
    for (i, &first) in items.iter().enumerate() {
        let mut rest = items.to_vec();
        rest.remove(i);
        for mut tail in permutations(&rest) {
            tail.insert(0, first);
            all.push(tail);
        }
    }
    all
}

/// A manual transaction, parked changes, a batch with a conflict and the dep, and failed
/// imports in every order, in a doc with and without auto-commit.
/// Whatever each step returns, nothing may panic, and the doc must agree with a replay
/// of its own history. `import_local_change` also asserts in debug builds that no
/// commit lands under parked history.
#[test]
fn manual_transaction_with_imports_in_every_order() {
    let Parked { dep, update } = parked();
    let conflicting = {
        let doc = writer(7);
        doc.get_map("m").insert("k", 1).unwrap();
        doc.commit_then_renew();
        all_updates(&doc)
    };
    let many_deps = {
        let source = writer(9);
        source.import(&dep).unwrap();
        source.import(&conflicting).unwrap();
        source.get_map("m").insert("x", 9).unwrap();
        source.commit_then_renew();
        source
            .export(ExportMode::updates_in_range(vec![IdSpan::new(9, 0, 1)]))
            .unwrap()
    };
    let failing = r#"{"schema_version":1,"start_version":{},"peers":["3"],"changes":[{
        "id":"0@0","timestamp":0,"deps":[],"lamport":0,"msg":null,"ops":[{
        "container":"cid:root-l:List","counter":0,
        "content":{"type":"insert","pos":5,"value":[0]}}]}]}"#;

    // It has to reach the state and roll back, not stop at decoding.
    let error = LoroDoc::new().import_json_updates(failing).unwrap_err();
    assert!(error.to_string().contains("retains"), "{error}");

    let batch = [conflicting.clone(), dep.clone()];
    for (auto_commit, parked_update) in [
        (false, &update),
        (false, &many_deps),
        (true, &update),
        (true, &many_deps),
    ] {
        for order in permutations(&[0, 1, 2, 3, 4, 5, 6]) {
            let doc = if auto_commit {
                auto_commit_doc_without_its_transaction()
            } else {
                LoroDoc::new()
            };
            doc.set_peer_id(7).unwrap();
            let mut txn: Option<Transaction> = None;
            for &step in &order {
                match step {
                    0 => txn = doc.txn().ok(),
                    1 => {
                        if let Some(txn) = txn.as_mut() {
                            doc.get_map("m")
                                .insert_with_txn(txn, "local", LoroValue::from(1))
                                .unwrap();
                        }
                    }
                    2 => {
                        if let Some(txn) = txn.take() {
                            txn.commit().unwrap();
                        }
                    }
                    3 => {
                        let _ = doc.import(parked_update);
                    }
                    4 => {
                        let _ = doc.import_batch(&batch);
                    }
                    5 => {
                        let _ = doc.import(&dep);
                    }
                    _ => {
                        let _ = doc.import_json_updates(failing);
                    }
                }
            }
            if let Some(txn) = txn.take() {
                txn.commit().unwrap();
            }
            if auto_commit {
                let _ = doc.get_map("m").insert("auto", 1);
                doc.commit_then_renew();
            }

            let _ = doc.import(parked_update);
            let _ = doc.import(&dep);
            let replay = LoroDoc::new();
            replay.import(&all_updates(&doc)).unwrap();
            for doc in [&replay, &doc] {
                doc.get_map("m");
                doc.get_list("l");
                doc.get_text("t");
            }
            assert_eq!(
                replay.get_deep_value().to_json_value(),
                doc.get_deep_value().to_json_value(),
                "{order:?} auto_commit={auto_commit}"
            );
        }
    }
}

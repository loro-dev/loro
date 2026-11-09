//! `try_checkout_to_latest`, `try_attach` and `checkout` return errors where the old
//! calls panicked under the doc's locks. See `context/version-input-contract.md`.

use loro::{ExportMode, LoroDoc, LoroError, LoroValue, ToJson};
use std::panic::{catch_unwind, AssertUnwindSafe};

/// Decodes, and is rejected by the state: a list insert past the end. A detached import
/// does not check it, so it waits in the op log for the next checkout.
const OUT_OF_BOUNDS: &str = r#"{"schema_version":1,"start_version":{},"peers":["3"],"changes":[{
    "id":"0@0","timestamp":0,"deps":[],"lamport":0,"msg":null,"ops":[{
    "container":"cid:root-l:List","counter":0,
    "content":{"type":"insert","pos":5,"value":[0]}}]}]}"#;

fn detached_with_rejected_history() -> LoroDoc {
    let doc = LoroDoc::new();
    doc.get_text("t").insert(0, "a").unwrap();
    doc.commit();
    doc.detach();
    doc.import_json_updates(OUT_OF_BOUNDS).unwrap();
    doc
}

fn assert_usable_at_its_version(doc: &LoroDoc) {
    assert!(doc.is_detached());
    assert_eq!(doc.get_text("t").to_string(), "a");
    let frontiers = doc.state_frontiers();
    doc.checkout(&frontiers).unwrap();
    assert!(doc.export(ExportMode::all_updates()).is_ok());
    // A snapshot holds the latest state, which cannot be built: an error, not a panic
    // under the doc's locks.
    assert!(doc.export(ExportMode::Snapshot).is_err());
    assert_eq!(doc.state_frontiers(), frontiers);
}

#[test]
fn try_checkout_to_latest_returns_the_state_error() {
    let doc = detached_with_rejected_history();
    let before = doc.state_frontiers();
    assert!(doc.try_checkout_to_latest().is_err());
    assert_eq!(doc.state_frontiers(), before);
    assert_usable_at_its_version(&doc);
    assert!(doc.try_attach().is_err());
    assert_usable_at_its_version(&doc);
}

/// The deprecated calls still panic, but with no lock held, so the doc is not poisoned.
#[test]
#[allow(deprecated)]
fn deprecated_calls_panic_after_releasing_the_locks() {
    let doc = detached_with_rejected_history();
    assert!(catch_unwind(AssertUnwindSafe(|| doc.checkout_to_latest())).is_err());
    assert_usable_at_its_version(&doc);
    assert!(catch_unwind(AssertUnwindSafe(|| doc.attach())).is_err());
    assert_usable_at_its_version(&doc);
}

#[test]
fn try_checkout_to_latest_attaches() {
    let doc = LoroDoc::new();
    doc.get_text("t").insert(0, "a").unwrap();
    doc.commit();
    let base = doc.state_frontiers();
    doc.get_text("t").insert(1, "b").unwrap();
    doc.commit();
    doc.checkout(&base).unwrap();
    doc.try_checkout_to_latest().unwrap();
    assert!(!doc.is_detached());
    doc.checkout(&base).unwrap();
    doc.try_attach().unwrap();
    assert!(!doc.is_detached());
    assert_eq!(doc.get_text("t").to_string(), "ab");
}

/// The transaction's ops are in the DAG and not in the change store, and reading that
/// history panicked with `DAG node must have a change`, poisoning the doc.
#[test]
fn checkout_with_an_uncommitted_transaction_is_an_error() {
    let doc = LoroDoc::new();
    doc.get_text("t").insert(0, "a").unwrap();
    doc.commit();
    let base = doc.state_frontiers();
    doc.get_text("t").insert(1, "b").unwrap();
    doc.commit_with(loro::CommitOptions::new().immediate_renew(false));
    doc.checkout(&base).unwrap();
    doc.try_attach().unwrap();
    // A checkout clears the undo stack, so the manager starts after it.
    let mut undo = loro::UndoManager::new(&doc);
    doc.get_text("t").insert(2, "c").unwrap();
    doc.commit_with(loro::CommitOptions::new().immediate_renew(false));

    let inner = doc.inner();
    let mut txn = inner.txn().unwrap();
    inner
        .get_map("m")
        .insert_with_txn(&mut txn, "k", LoroValue::from(1))
        .unwrap();
    match doc.checkout(&base) {
        Err(LoroError::TransactionError(_)) => {}
        other => panic!("expected TransactionError, got {other:?}"),
    }
    assert!(!doc.is_detached());
    // Undo checks out earlier versions too.
    assert!(undo.can_undo());
    match undo.undo() {
        Err(LoroError::DuplicatedTransactionError) => {}
        other => panic!("expected DuplicatedTransactionError, got {other:?}"),
    }
    txn.commit().unwrap();

    doc.checkout(&base).unwrap();
    doc.try_checkout_to_latest().unwrap();
    assert_eq!(doc.get_text("t").to_string(), "abc");
    assert_eq!(doc.get_map("m").get_deep_value().to_json_value()["k"], 1);
}

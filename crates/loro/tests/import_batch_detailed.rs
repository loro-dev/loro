//! `import_batch_detailed` changes the document as `import_batch` does and says what
//! happened to each blob. See `context/import-batch-atomicity.md`.

use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

use loro::{
    ExportMode, ImportBatchDetail, ImportBlobOutcome, ImportStatus, JsonListOp, JsonOpContent,
    LoroDoc, LoroError, LoroResult, ToJson, VersionRange, VersionVector, ID,
};
use pretty_assertions::assert_eq;

fn range(spans: &[(u64, i32, i32)]) -> VersionRange {
    let mut range = VersionRange::new();
    for &(peer, start, end) in spans {
        range.insert(peer, start, end);
    }
    range
}

fn status(success: &[(u64, i32, i32)], pending: Option<&[(u64, i32, i32)]>) -> ImportStatus {
    ImportStatus {
        success: range(success),
        pending: pending.map(range),
    }
}

fn imported(success: &[(u64, i32, i32)]) -> ImportBlobOutcome {
    ImportBlobOutcome::Imported(status(success, None))
}

fn used_op_id(peer: u64, counter: i32) -> ImportBlobOutcome {
    ImportBlobOutcome::Failed(LoroError::UsedOpID {
        id: ID::new(peer, counter),
    })
}

fn deep_json(doc: &LoroDoc) -> serde_json::Value {
    doc.get_deep_value().to_json_value()
}

/// `1@0` writes "x". Each `update` is `7@0` on top of it, so two of them conflict.
struct Fork {
    dep: Vec<u8>,
    dep_vv: VersionVector,
    dep_doc: LoroDoc,
}

impl Fork {
    fn new() -> LoroResult<Self> {
        let dep_doc = LoroDoc::new();
        dep_doc.set_peer_id(1)?;
        dep_doc.get_text("t").insert(0, "x")?;
        dep_doc.commit();
        Ok(Self {
            dep: dep_doc.export(ExportMode::all_updates())?,
            dep_vv: dep_doc.oplog_vv(),
            dep_doc,
        })
    }

    fn update(&self, text: &str) -> LoroResult<Vec<u8>> {
        let doc = self.dep_doc.fork();
        doc.set_peer_id(7)?;
        doc.get_text("t").insert(1, text)?;
        doc.commit();
        Ok(doc.export(ExportMode::updates(&self.dep_vv))?)
    }

    /// A doc of `peer` that has the dep and one map entry of its own.
    fn other(&self, peer: u64) -> LoroResult<LoroDoc> {
        let doc = self.dep_doc.fork();
        doc.set_peer_id(peer)?;
        doc.get_map("m").insert(&peer.to_string(), true)?;
        doc.commit();
        Ok(doc)
    }

    fn other_update(&self, peer: u64) -> LoroResult<Vec<u8>> {
        Ok(self
            .other(peer)?
            .export(ExportMode::updates(&self.dep_vv))?)
    }

    /// A doc that has the dep and `7@0` writing "A".
    fn doc_with_a(&self) -> LoroResult<LoroDoc> {
        let doc = LoroDoc::new();
        doc.set_peer_id(100)?;
        doc.import(&self.dep)?;
        doc.import(&self.update("A")?)?;
        Ok(doc)
    }
}

/// Binary updates of `peer` with a list insert far past the end of the list. They
/// decode into the op log, and only the state rejects them.
fn update_the_state_rejects(peer: u64) -> LoroResult<Vec<u8>> {
    let doc = LoroDoc::new();
    doc.set_peer_id(peer)?;
    doc.get_list("list").insert(0, "seed")?;
    doc.get_list("list").insert(1, "tail")?;
    doc.commit();
    let mut json = doc.export_json_updates(&Default::default(), &doc.oplog_vv())?;
    let op = json.changes.last_mut().unwrap().ops.last_mut().unwrap();
    match &mut op.content {
        JsonOpContent::List(JsonListOp::Insert { pos, .. }) => *pos = 1_000,
        other => panic!("expected a list insert, got {other:?}"),
    }

    let carrier = LoroDoc::new();
    carrier.detach();
    carrier.import_json_updates(serde_json::to_string(&json).unwrap())?;
    Ok(carrier.export(ExportMode::all_updates())?)
}

/// The metadata still decodes, so the blob is tried and fails in its own import. The
/// checksum is in bytes 4..20, after the magic bytes.
fn with_bad_checksum(mut blob: Vec<u8>) -> Vec<u8> {
    blob[19] ^= 0xff;
    blob
}

/// The blob in the middle reuses `7@0` with other content. The snapshot is last in the
/// input and imported first, so the outcomes have to follow the input, not the import.
#[test]
fn a_conflicting_blob_fails_alone_and_the_outcomes_follow_the_input() -> LoroResult<()> {
    let fork = Fork::new()?;
    let batch = [
        fork.other_update(8)?,
        fork.update("B")?,
        fork.other(9)?.export(ExportMode::Snapshot)?,
    ];

    let doc = fork.doc_with_a()?;
    let detail = doc.import_batch_detailed(&batch)?;
    assert_eq!(
        detail,
        ImportBatchDetail {
            blobs: vec![
                imported(&[(8, 0, 1)]),
                used_op_id(7, 0),
                imported(&[(9, 0, 1)])
            ],
            status: status(&[(8, 0, 1), (9, 0, 1)], None),
            rollback_error: None,
        }
    );
    assert!(!doc.is_detached());
    assert_eq!(doc.get_text("t").to_string(), "xA");
    assert_eq!(
        deep_json(&doc)["m"],
        serde_json::json!({"8": true, "9": true})
    );

    // `import_batch` leaves the same document and reports only the error.
    let twin = fork.doc_with_a()?;
    assert_eq!(
        twin.import_batch(&batch).unwrap_err(),
        LoroError::UsedOpID { id: ID::new(7, 0) }
    );
    assert_eq!(twin.oplog_vv(), doc.oplog_vv());
    assert_eq!(deep_json(&twin), deep_json(&doc));
    Ok(())
}

/// A status is what that blob's import returned. The first blob waits for the dep, and
/// the dep's status counts what it let through.
#[test]
fn a_blob_that_waits_is_pending_and_the_dep_reports_what_it_unlocked() -> LoroResult<()> {
    let fork = Fork::new()?;
    let doc = LoroDoc::new();
    let detail =
        doc.import_batch_detailed(&[fork.update("A")?, fork.update("B")?, fork.dep.clone()])?;
    assert_eq!(
        detail,
        ImportBatchDetail {
            blobs: vec![
                ImportBlobOutcome::Imported(status(&[], Some(&[(7, 0, 1)]))),
                used_op_id(7, 0),
                imported(&[(1, 0, 1), (7, 0, 1)]),
            ],
            status: status(&[(1, 0, 1), (7, 0, 1)], None),
            rollback_error: None,
        }
    );
    assert_eq!(doc.get_text("t").to_string(), "xA");
    Ok(())
}

/// `status.pending` is everything the document still waits for when the batch ends,
/// also what was parked before it, in a batch of one too.
#[test]
fn the_batch_status_covers_updates_parked_before_the_batch() -> LoroResult<()> {
    let fork = Fork::new()?;
    let other = LoroDoc::new();
    other.set_peer_id(8)?;
    other.get_map("m").insert("k", 1)?;
    other.commit();
    let other = other.export(ExportMode::all_updates())?;

    for batch in [
        vec![fork.update("B")?, other.clone()],
        vec![other.clone()],
        vec![fork.update("B")?],
    ] {
        let doc = LoroDoc::new();
        assert!(doc.import(&fork.update("A")?)?.pending.is_some());
        let detail = doc.import_batch_detailed(&batch)?;
        let expected: Vec<_> = batch
            .iter()
            .map(|blob| match blob == &other {
                true => imported(&[(8, 0, 1)]),
                false => used_op_id(7, 0),
            })
            .collect();
        assert_eq!(detail.blobs, expected);
        assert_eq!(detail.status.pending, Some(range(&[(7, 0, 1)])));
        assert_eq!(detail.rollback_error, None);
    }
    Ok(())
}

/// Only the closing checkout applies a batch to the state. When it fails, nothing of the
/// batch stays, so no blob may be reported as imported.
#[test]
fn a_failed_closing_checkout_reports_every_blob_as_rolled_back() -> LoroResult<()> {
    let fork = Fork::new()?;
    let good = [fork.other_update(8)?, fork.other_update(9)?];
    let batch = [
        good[0].clone(),
        update_the_state_rejects(5)?,
        with_bad_checksum(fork.other_update(6)?),
        good[1].clone(),
    ];

    let doc = fork.doc_with_a()?;
    doc.get_list("list").push("seed")?;
    doc.commit();
    let vv = doc.oplog_vv();
    let value = deep_json(&doc);
    let events = Arc::new(AtomicUsize::new(0));
    let counter = events.clone();
    let _sub = doc.subscribe_root(Arc::new(move |_| {
        counter.fetch_add(1, Ordering::SeqCst);
    }));

    let detail = doc.import_batch_detailed(&batch)?;
    let error = detail.rollback_error.expect("the state rejects the batch");
    assert!(error.to_string().contains("list diff"), "{error}");
    assert_eq!(
        detail.blobs,
        vec![
            ImportBlobOutcome::RolledBack,
            ImportBlobOutcome::RolledBack,
            // Its own error: this blob was never in the batch that was rolled back.
            ImportBlobOutcome::Failed(LoroError::DecodeChecksumMismatchError),
            ImportBlobOutcome::RolledBack,
        ]
    );
    assert_eq!(detail.status, ImportStatus::default());
    assert!(!doc.is_detached());
    assert_eq!(doc.oplog_vv(), vv);
    assert_eq!(deep_json(&doc), value);
    assert_eq!(events.load(Ordering::SeqCst), 0);

    // `import_batch` reports the same rollback as its error.
    assert!(doc
        .import_batch(&batch)
        .unwrap_err()
        .to_string()
        .contains("list diff"));
    assert_eq!(doc.oplog_vv(), vv);

    // The doc works, and the blobs that were rolled back import without the bad one.
    doc.get_text("t").insert(0, "ok ")?;
    doc.commit();
    let detail = doc.import_batch_detailed(&good)?;
    assert_eq!(
        detail.blobs,
        vec![imported(&[(8, 0, 1)]), imported(&[(9, 0, 1)])]
    );
    assert_eq!(detail.rollback_error, None);
    assert_eq!(
        deep_json(&doc)["m"],
        serde_json::json!({"8": true, "9": true})
    );
    Ok(())
}

/// `import_batch` decodes the metadata of every blob first and imports nothing when one
/// does not decode. The detailed call fails that blob and imports the others.
#[test]
fn a_blob_with_unreadable_metadata_fails_alone() -> LoroResult<()> {
    let fork = Fork::new()?;
    let batch = [fork.other_update(8)?, vec![1, 2, 3], fork.other_update(9)?];

    let doc = fork.doc_with_a()?;
    let detail = doc.import_batch_detailed(&batch)?;
    assert_eq!(detail.blobs[0], imported(&[(8, 0, 1)]));
    assert!(
        matches!(
            detail.blobs[1],
            ImportBlobOutcome::Failed(LoroError::DecodeError(_))
        ),
        "{:?}",
        detail.blobs[1]
    );
    assert_eq!(detail.blobs[2], imported(&[(9, 0, 1)]));
    assert_eq!(detail.status, status(&[(8, 0, 1), (9, 0, 1)], None));
    assert_eq!(detail.rollback_error, None);

    // No readable blob at all.
    let detail = doc.import_batch_detailed(&[vec![1, 2, 3], vec![]])?;
    assert!(detail
        .blobs
        .iter()
        .all(|blob| matches!(blob, ImportBlobOutcome::Failed(LoroError::DecodeError(_)))));
    assert_eq!(detail.blobs.len(), 2);
    assert_eq!(detail.status, ImportStatus::default());

    let twin = fork.doc_with_a()?;
    let vv = twin.oplog_vv();
    assert!(matches!(
        twin.import_batch(&batch),
        Err(LoroError::DecodeError(_))
    ));
    assert_eq!(twin.oplog_vv(), vv);
    Ok(())
}

/// A batch of one is a plain import, as in `import_batch`: the state checks the blob
/// when it is imported, so its failure is its own and not a rollback.
#[test]
fn empty_and_single_blob_batches() -> LoroResult<()> {
    let fork = Fork::new()?;
    let doc = fork.doc_with_a()?;
    doc.get_list("list").push("seed")?;
    doc.commit();
    let vv = doc.oplog_vv();

    assert_eq!(
        doc.import_batch_detailed(&[])?,
        ImportBatchDetail {
            blobs: vec![],
            status: ImportStatus::default(),
            rollback_error: None,
        }
    );

    let detail = doc.import_batch_detailed(&[fork.update("B")?])?;
    assert_eq!(
        detail,
        ImportBatchDetail {
            blobs: vec![used_op_id(7, 0)],
            status: ImportStatus::default(),
            rollback_error: None,
        }
    );

    let detail = doc.import_batch_detailed(&[update_the_state_rejects(5)?])?;
    assert_eq!(detail.rollback_error, None);
    match &detail.blobs[..] {
        [ImportBlobOutcome::Failed(error)] => {
            assert!(error.to_string().contains("retains"), "{error}")
        }
        other => panic!("expected one failed blob, got {other:?}"),
    }
    assert_eq!(doc.oplog_vv(), vv);

    let detail = doc.import_batch_detailed(&[fork.other_update(8)?])?;
    assert_eq!(
        detail,
        ImportBatchDetail {
            blobs: vec![imported(&[(8, 0, 1)])],
            status: status(&[(8, 0, 1)], None),
            rollback_error: None,
        }
    );
    Ok(())
}

/// A detached doc has no closing checkout, so there is nothing to roll back: each blob
/// stands alone and the doc stays detached.
#[test]
fn a_detached_doc_stays_detached_and_never_rolls_back() -> LoroResult<()> {
    let fork = Fork::new()?;
    let doc = fork.doc_with_a()?;
    doc.detach();
    let detail = doc.import_batch_detailed(&[
        fork.other_update(8)?,
        fork.update("B")?,
        update_the_state_rejects(5)?,
    ])?;
    assert_eq!(
        detail.blobs,
        vec![
            imported(&[(8, 0, 1)]),
            used_op_id(7, 0),
            imported(&[(5, 0, 2)])
        ]
    );
    assert_eq!(detail.rollback_error, None);
    assert!(doc.is_detached());
    Ok(())
}

fn failed_to_decode(outcome: &ImportBlobOutcome) -> bool {
    matches!(
        outcome,
        ImportBlobOutcome::Failed(LoroError::DecodeError(_))
    )
}

/// Two or more blobs, and none of them decode. `import_batch` returns before the
/// batch critical section, so an uncommitted local change stays uncommitted.
/// `import_batch_detailed` must do the same and only report `Failed`.
#[test]
fn undecodable_blobs_do_not_commit_a_local_draft() -> LoroResult<()> {
    // The op is already in the oplog version vector. Committing it is visible as
    // the pending transaction closing and a local update being published.
    let doc = LoroDoc::new();
    let emitted = Arc::new(AtomicUsize::new(0));
    let emitted_in_callback = emitted.clone();
    let _sub = doc.subscribe_local_update(Box::new(move |_| {
        emitted_in_callback.fetch_add(1, Ordering::SeqCst);
        true
    }));
    doc.get_text("t").insert(0, "draft")?;
    let vv = doc.oplog_vv();
    let pending = doc.get_pending_txn_len();
    assert!(pending > 0);
    assert_eq!(emitted.load(Ordering::SeqCst), 0);

    let detail = doc.import_batch_detailed(&[vec![1, 2, 3], vec![]])?;
    assert_eq!(detail.blobs.len(), 2);
    assert!(
        detail.blobs.iter().all(failed_to_decode),
        "{:?}",
        detail.blobs
    );
    assert_eq!(detail.status, ImportStatus::default());
    assert_eq!(detail.rollback_error, None);
    assert_eq!(doc.oplog_vv(), vv);
    assert_eq!(doc.get_pending_txn_len(), pending);
    assert_eq!(doc.get_text("t").to_string(), "draft");
    assert!(!doc.is_detached());
    assert_eq!(emitted.load(Ordering::SeqCst), 0);

    let doc2 = LoroDoc::new();
    let emitted2 = Arc::new(AtomicUsize::new(0));
    let emitted2_in_callback = emitted2.clone();
    let _sub2 = doc2.subscribe_local_update(Box::new(move |_| {
        emitted2_in_callback.fetch_add(1, Ordering::SeqCst);
        true
    }));
    doc2.get_text("t").insert(0, "draft")?;
    let pending2 = doc2.get_pending_txn_len();
    assert!(matches!(
        doc2.import_batch(&[vec![1, 2, 3], vec![]]),
        Err(LoroError::DecodeError(_))
    ));
    assert_eq!(doc2.get_pending_txn_len(), pending2);
    assert_eq!(emitted2.load(Ordering::SeqCst), 0);
    Ok(())
}

/// The closing checkout of a batch renews the peer id when detached editing is on.
/// A batch whose metadata never decodes must not reach that checkout.
#[test]
fn undecodable_blobs_do_not_renew_the_peer_id() -> LoroResult<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(7)?;
    doc.set_detached_editing(true);
    assert_eq!(doc.peer_id(), 7);
    assert!(!doc.is_detached());

    let detail = doc.import_batch_detailed(&[vec![1], vec![2]])?;
    assert_eq!(detail.blobs.len(), 2);
    assert!(
        detail.blobs.iter().all(failed_to_decode),
        "{:?}",
        detail.blobs
    );
    assert_eq!(detail.status, ImportStatus::default());
    assert_eq!(detail.rollback_error, None);
    assert_eq!(doc.peer_id(), 7);
    assert!(!doc.is_detached());
    Ok(())
}

/// One blob that decodes is still imported, and the batch still commits a local
/// draft. `import_batch` keeps refusing the whole call before that.
#[test]
fn one_decodable_blob_still_imports_the_rest_and_commits() -> LoroResult<()> {
    let other = LoroDoc::new();
    other.set_peer_id(8)?;
    other.get_map("m").insert("k", true)?;
    other.commit();
    let good = other.export(ExportMode::all_updates())?;

    let doc = LoroDoc::new();
    let emitted = Arc::new(AtomicUsize::new(0));
    let emitted_in_callback = emitted.clone();
    let _sub = doc.subscribe_local_update(Box::new(move |_| {
        emitted_in_callback.fetch_add(1, Ordering::SeqCst);
        true
    }));
    doc.set_peer_id(7)?;
    doc.set_detached_editing(true);
    doc.get_text("t").insert(0, "draft")?;
    let pending = doc.get_pending_txn_len();
    assert!(pending > 0);
    assert_eq!(doc.peer_id(), 7);
    assert_eq!(emitted.load(Ordering::SeqCst), 0);

    let detail = doc.import_batch_detailed(&[good.clone(), vec![1, 2, 3]])?;
    assert!(
        matches!(detail.blobs[0], ImportBlobOutcome::Imported(_)),
        "{:?}",
        detail.blobs[0]
    );
    assert!(failed_to_decode(&detail.blobs[1]), "{:?}", detail.blobs[1]);
    assert_eq!(detail.rollback_error, None);
    assert_eq!(doc.get_text("t").to_string(), "draft");
    assert_eq!(deep_json(&doc)["m"], serde_json::json!({"k": true}));
    assert_eq!(doc.get_pending_txn_len(), 0);
    assert_eq!(emitted.load(Ordering::SeqCst), 1);
    // Still the batch critical section: detached editing renews the peer id.
    assert_ne!(doc.peer_id(), 7);
    assert!(!doc.is_detached());

    let twin = LoroDoc::new();
    let emitted_twin = Arc::new(AtomicUsize::new(0));
    let emitted_twin_in_callback = emitted_twin.clone();
    let _sub_twin = twin.subscribe_local_update(Box::new(move |_| {
        emitted_twin_in_callback.fetch_add(1, Ordering::SeqCst);
        true
    }));
    twin.set_peer_id(7)?;
    twin.set_detached_editing(true);
    twin.get_text("t").insert(0, "draft")?;
    let twin_pending = twin.get_pending_txn_len();
    assert!(matches!(
        twin.import_batch(&[good, vec![1, 2, 3]]),
        Err(LoroError::DecodeError(_))
    ));
    assert_eq!(twin.get_pending_txn_len(), twin_pending);
    assert_eq!(emitted_twin.load(Ordering::SeqCst), 0);
    assert_eq!(twin.peer_id(), 7);
    assert!(deep_json(&twin).get("m").is_none());
    Ok(())
}

/// A batch of one is the plain import shortcut. It commits first, in both calls.
#[test]
fn a_single_undecodable_blob_still_commits_like_import_batch() -> LoroResult<()> {
    let doc = LoroDoc::new();
    let emitted = Arc::new(AtomicUsize::new(0));
    let emitted_in_callback = emitted.clone();
    let _sub = doc.subscribe_local_update(Box::new(move |_| {
        emitted_in_callback.fetch_add(1, Ordering::SeqCst);
        true
    }));
    doc.set_peer_id(7)?;
    doc.get_text("t").insert(0, "draft")?;
    let vv = doc.oplog_vv();
    let pending = doc.get_pending_txn_len();
    assert!(pending > 0);
    let detail = doc.import_batch_detailed(&[vec![1, 2, 3]])?;
    assert_eq!(detail.blobs.len(), 1);
    assert!(failed_to_decode(&detail.blobs[0]), "{:?}", detail.blobs[0]);
    assert_eq!(detail.rollback_error, None);
    // The shortcut commits. The version vector already held the op.
    assert_eq!(doc.oplog_vv(), vv);
    assert_eq!(doc.get_pending_txn_len(), 0);
    assert_eq!(emitted.load(Ordering::SeqCst), 1);
    assert_eq!(doc.get_text("t").to_string(), "draft");

    let twin = LoroDoc::new();
    let emitted_twin = Arc::new(AtomicUsize::new(0));
    let emitted_twin_in_callback = emitted_twin.clone();
    let _sub_twin = twin.subscribe_local_update(Box::new(move |_| {
        emitted_twin_in_callback.fetch_add(1, Ordering::SeqCst);
        true
    }));
    twin.set_peer_id(7)?;
    twin.get_text("t").insert(0, "draft")?;
    let twin_vv = twin.oplog_vv();
    assert!(matches!(
        twin.import_batch(&[vec![1, 2, 3]]),
        Err(LoroError::DecodeError(_))
    ));
    assert_eq!(twin.oplog_vv(), twin_vv);
    assert_eq!(twin.oplog_vv(), doc.oplog_vv());
    assert_eq!(twin.get_pending_txn_len(), 0);
    assert_eq!(emitted_twin.load(Ordering::SeqCst), 1);
    Ok(())
}

/// Parked updates stay parked, and `status.pending` still names them, when the new
/// blobs all fail to decode.
#[test]
fn an_undecodable_batch_still_reports_updates_parked_before_it() -> LoroResult<()> {
    let fork = Fork::new()?;
    let doc = LoroDoc::new();
    assert!(doc.import(&fork.update("A")?)?.pending.is_some());
    let vv = doc.oplog_vv();
    let detail = doc.import_batch_detailed(&[vec![1, 2, 3], vec![]])?;
    assert_eq!(detail.blobs.len(), 2);
    assert!(
        detail.blobs.iter().all(failed_to_decode),
        "{:?}",
        detail.blobs
    );
    assert!(detail.status.success.is_empty());
    assert_eq!(detail.status.pending, Some(range(&[(7, 0, 1)])));
    assert_eq!(detail.rollback_error, None);
    assert_eq!(doc.oplog_vv(), vv);
    Ok(())
}

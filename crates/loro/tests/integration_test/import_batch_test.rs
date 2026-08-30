//! `import_batch([snapshot, ...updates])` must produce the same
//! document as importing the same blobs sequentially.
//!
//! At base 1.15.1 a shallow snapshot handed to `import_batch` on a FRESH doc is
//! routed through the update lane (`decode_oplog` extracts only the trimmed oplog
//! window; the shallow-root state bytes are never decoded), so every change in the
//! window has unmet deps, everything parks as pending, and the doc stays empty —
//! silently. Upstream #1066 fixed detached-mode stranding in the same path but not
//! this. These tests pin batch ≡ sequential across the failure surface.

use loro::{ExportMode, LoroDoc};

/// Long single-peer history with a movable list — the fixture shape used by the fresh-doc cases below.
/// Single peer ⇒ single-head frontier ⇒ the shallow cut is genuinely well-formed
/// (shared ancestry; independent of the P2 multi-head shallow-root bug).
fn build_long_history_doc(words: usize) -> LoroDoc {
    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    let list = doc.get_movable_list("words");
    for i in 0..words {
        list.insert(i, format!("word{i}")).unwrap();
        if i % 200 == 199 {
            doc.commit();
        }
    }
    doc.commit();
    doc
}

/// Export a snapshot at the doc's current frontier plus `n_tail` single-commit
/// update blobs made after the cut.
fn snapshot_plus_tail(doc: &LoroDoc, shallow: bool, n_tail: usize) -> (Vec<u8>, Vec<Vec<u8>>) {
    let snapshot = if shallow {
        doc.export(ExportMode::shallow_snapshot(&doc.oplog_frontiers()))
            .unwrap()
    } else {
        doc.export(ExportMode::Snapshot).unwrap()
    };
    let mut tail = Vec::new();
    for i in 0..n_tail {
        let vv = doc.oplog_vv();
        doc.get_movable_list("words")
            .set(i, format!("TAIL-{i}"))
            .unwrap();
        doc.commit();
        tail.push(doc.export(ExportMode::updates(&vv)).unwrap());
    }
    (snapshot, tail)
}

/// Batch result must equal the sequential-import oracle, blob for blob.
fn assert_batch_equals_sequential(blobs: &[Vec<u8>], source: &LoroDoc) {
    let batched = LoroDoc::new();
    let status = batched.import_batch(blobs).unwrap();

    let sequential = LoroDoc::new();
    for b in blobs {
        sequential.import(b).unwrap();
    }

    assert_eq!(
        batched.get_deep_value(),
        sequential.get_deep_value(),
        "batched state != sequential state"
    );
    assert_eq!(batched.oplog_frontiers(), sequential.oplog_frontiers());
    assert_eq!(batched.state_frontiers(), sequential.state_frontiers());
    assert_eq!(batched.get_deep_value(), source.get_deep_value());
    assert_eq!(batched.oplog_frontiers(), source.oplog_frontiers());
    assert!(
        status.pending.is_none(),
        "nothing may park as pending, got {:?}",
        status.pending
    );
    assert!(!batched.is_detached(), "import_batch must leave the doc attached");
}

/// The core scenario: fresh doc, batch = [shallow snapshot, tail updates].
/// RED on base 5c6b9f6f: empty doc, empty success, whole tail pending.
#[test]
fn import_batch_shallow_snapshot_plus_tail_into_fresh_doc() {
    let doc = build_long_history_doc(500);
    let (snapshot, tail) = snapshot_plus_tail(&doc, true, 2);
    let mut blobs = vec![snapshot];
    blobs.extend(tail);
    assert_batch_equals_sequential(&blobs, &doc);
}

/// Member order must not matter (`import_batch` docs: "data can be in arbitrary
/// order") — the snapshot placed LAST must still be applied first via sorting.
/// (The oracle is the source doc / snapshot-first batch, not same-order
/// sequential imports: plain sequential `import` is order-sensitive by design.)
#[test]
fn import_batch_shallow_snapshot_last_in_batch() {
    let doc = build_long_history_doc(300);
    let (snapshot, tail) = snapshot_plus_tail(&doc, true, 2);
    let mut blobs = tail;
    blobs.push(snapshot);

    let batched = LoroDoc::new();
    let status = batched.import_batch(&blobs).unwrap();

    assert_eq!(batched.get_deep_value(), doc.get_deep_value());
    assert_eq!(batched.oplog_frontiers(), doc.oplog_frontiers());
    assert!(status.pending.is_none());
    assert!(!batched.is_detached());
}

/// Full (non-shallow) snapshot + tail into a fresh doc — does the bug hit full
/// snapshots too? (Full snapshots carry the whole oplog, so the update lane can
/// reconstruct them; this arm maps the failure surface either way.)
#[test]
fn import_batch_full_snapshot_plus_tail_into_fresh_doc() {
    let doc = build_long_history_doc(300);
    let (snapshot, tail) = snapshot_plus_tail(&doc, false, 2);
    let mut blobs = vec![snapshot];
    blobs.extend(tail);
    assert_batch_equals_sequential(&blobs, &doc);
}

/// Mixed batch into a NON-empty doc that already holds the history below the cut:
/// the shallow window's deps are satisfied, so batch must equal sequential here too.
#[test]
fn import_batch_shallow_snapshot_plus_tail_into_doc_with_history() {
    let doc = build_long_history_doc(300);
    let pre_cut = doc.export(ExportMode::Snapshot).unwrap();
    let (snapshot, tail) = snapshot_plus_tail(&doc, true, 2);
    let mut blobs = vec![snapshot];
    blobs.extend(tail);

    let batched = LoroDoc::new();
    batched.import(&pre_cut).unwrap();
    let status = batched.import_batch(&blobs).unwrap();

    let sequential = LoroDoc::new();
    sequential.import(&pre_cut).unwrap();
    for b in &blobs {
        sequential.import(b).unwrap();
    }

    assert_eq!(batched.get_deep_value(), sequential.get_deep_value());
    assert_eq!(batched.oplog_frontiers(), sequential.oplog_frontiers());
    assert_eq!(batched.get_deep_value(), doc.get_deep_value());
    assert!(status.pending.is_none());
    assert!(!batched.is_detached());
}

/// Sequential import stays equivalent: single snapshot import, then a pure-update
/// batch — and that equals the mixed batch after the fix.
#[test]
fn import_batch_snapshot_then_update_batch_equivalence() {
    let doc = build_long_history_doc(300);
    let (snapshot, tail) = snapshot_plus_tail(&doc, true, 3);

    let hydrated = LoroDoc::new();
    hydrated.import(&snapshot).unwrap();
    let status = hydrated.import_batch(&tail).unwrap();
    assert!(status.pending.is_none());

    let mut blobs = vec![snapshot];
    blobs.extend(tail);
    let batched = LoroDoc::new();
    batched.import_batch(&blobs).unwrap();

    assert_eq!(batched.get_deep_value(), hydrated.get_deep_value());
    assert_eq!(batched.oplog_frontiers(), hydrated.oplog_frontiers());
    assert_eq!(
        batched.export(ExportMode::Snapshot).unwrap(),
        hydrated.export(ExportMode::Snapshot).unwrap()
    );
}

/// Pure-update batches are the hot tail path for incremental sync: bytes and semantics must
/// not move. Same ops through the batch lane on two docs ⇒ identical exports.
#[test]
fn import_batch_pure_updates_unchanged_and_deterministic() {
    let doc = build_long_history_doc(50);
    let mut updates = Vec::new();
    for i in 0..3 {
        let vv = doc.oplog_vv();
        doc.get_movable_list("words")
            .set(i, format!("edit{i}"))
            .unwrap();
        doc.commit();
        updates.push(doc.export(ExportMode::updates(&vv)).unwrap());
    }

    let a = LoroDoc::new();
    let base = doc.export(ExportMode::updates(&Default::default())).unwrap();
    // History below the tail delivered as one update blob, then the tail batch —
    // updates-only batches, both docs.
    let b = LoroDoc::new();
    for d in [&a, &b] {
        d.import(&base).unwrap();
        let st = d.import_batch(&updates).unwrap();
        assert!(st.pending.is_none());
    }
    assert_eq!(a.get_deep_value(), doc.get_deep_value());
    assert_eq!(
        a.export(ExportMode::Snapshot).unwrap(),
        b.export(ExportMode::Snapshot).unwrap()
    );
    assert_eq!(
        a.export(ExportMode::updates(&Default::default())).unwrap(),
        b.export(ExportMode::updates(&Default::default())).unwrap()
    );
}

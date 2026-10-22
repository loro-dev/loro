use loro::{ExportMode, IdSpan, LoroDoc, VersionVector};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

fn assert_no_pending(doc: &LoroDoc) -> anyhow::Result<()> {
    assert!(doc.export_pending_updates()?.is_none());
    Ok(())
}

fn restore_with_pending(doc: &LoroDoc) -> anyhow::Result<LoroDoc> {
    let pending = doc
        .export_pending_updates()?
        .expect("remote pending updates");
    let restored = LoroDoc::new();
    restored.import(&doc.export(ExportMode::Snapshot)?)?;
    let status = restored.import(&pending)?;
    assert!(status.success.is_empty());
    assert!(status.pending.is_some());
    Ok(restored)
}

#[test]
fn no_pending_updates_with_empty_or_applied_history() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    assert_no_pending(&doc)?;
    doc.get_text("text").insert(0, "committed")?;
    doc.commit();
    assert_no_pending(&doc)?;

    let restored = LoroDoc::new();
    restored.import(&doc.export(ExportMode::all_updates())?)?;
    assert_no_pending(&restored)?;
    Ok(())
}

fn out_of_order_roundtrip(base: &str) -> anyhow::Result<()> {
    let source = LoroDoc::new();
    source.set_peer_id(1)?;
    source.get_text("text").insert(0, base)?;
    source.commit();
    let base_snapshot = source.export(ExportMode::Snapshot)?;
    let base_vv = source.oplog_vv();

    source.get_text("text").insert(base.len(), "A")?;
    source.commit();
    let a = source.export(ExportMode::updates(&base_vv))?;
    let after_a = source.oplog_vv();
    source.get_text("text").insert(base.len() + 1, "B")?;
    source.commit();
    let b = source.export(ExportMode::updates(&after_a))?;

    let doc = LoroDoc::new();
    doc.import(&base_snapshot)?;
    let status = doc.import(&b)?;
    assert!(status.success.is_empty());
    assert!(status.pending.is_some());
    assert_eq!(doc.get_text("text").to_string(), base);

    let vv = doc.oplog_vv();
    let frontiers = doc.oplog_frontiers();
    let state_frontiers = doc.state_frontiers();
    let events = Arc::new(AtomicUsize::new(0));
    let event_count = events.clone();
    let _subscription = doc.subscribe_root(Arc::new(move |_| {
        event_count.fetch_add(1, Ordering::SeqCst);
    }));
    let sidecar = doc.export_pending_updates()?.unwrap();
    assert_eq!(doc.oplog_vv(), vv);
    assert_eq!(doc.oplog_frontiers(), frontiers);
    assert_eq!(doc.state_frontiers(), state_frontiers);
    assert_eq!(doc.get_text("text").to_string(), base);
    assert_eq!(events.load(Ordering::SeqCst), 0);
    assert!(doc.export_pending_updates()?.is_some());

    // The ordinary exports keep their existing applied-history behavior.
    let without_sidecar = LoroDoc::new();
    without_sidecar.import(&doc.export(ExportMode::Snapshot)?)?;
    without_sidecar.import(&a)?;
    assert_eq!(
        without_sidecar.get_text("text").to_string(),
        format!("{base}A")
    );
    let updates_only = LoroDoc::new();
    updates_only.import(&doc.export(ExportMode::all_updates())?)?;
    updates_only.import(&a)?;
    assert_eq!(
        updates_only.get_text("text").to_string(),
        format!("{base}A")
    );

    let restored = restore_with_pending(&doc)?;
    assert_eq!(restored.get_text("text").to_string(), base);
    // Saving a restored, still-pending document must keep the same information.
    let restored_again = restore_with_pending(&restored)?;
    assert_eq!(restored_again.get_text("text").to_string(), base);
    let repeated = restored_again.import(&sidecar)?;
    assert!(repeated.success.is_empty());
    assert!(repeated.pending.is_some());
    assert_eq!(restored_again.oplog_vv(), vv);

    for target in [&doc, &restored, &restored_again] {
        let status = target.import(&a)?;
        assert!(status.pending.is_none());
        assert_eq!(target.get_text("text").to_string(), format!("{base}AB"));
        assert_no_pending(target)?;
        target.import(&sidecar)?;
        target.import(&a)?;
        target.import(&b)?;
        assert_eq!(target.get_text("text").to_string(), format!("{base}AB"));
        assert_no_pending(target)?;
    }
    Ok(())
}

#[test]
fn out_of_order_pending_roundtrip_from_empty_document() -> anyhow::Result<()> {
    out_of_order_roundtrip("")
}

#[test]
fn out_of_order_pending_roundtrip_with_nonempty_base() -> anyhow::Result<()> {
    out_of_order_roundtrip("base|")
}

#[test]
fn overlapping_multiple_peer_pending_changes_resolve_in_stages() -> anyhow::Result<()> {
    let a = LoroDoc::new();
    a.set_peer_id(10)?;
    for i in 0..3 {
        a.get_map("root").insert(&format!("a{i}"), i)?;
        a.commit();
    }
    let a0 = a.export(ExportMode::updates_in_range(vec![IdSpan::new(10, 0, 1)]))?;
    let a1 = a.export(ExportMode::updates_in_range(vec![IdSpan::new(10, 1, 2)]))?;
    let a2 = a.export(ExportMode::updates_in_range(vec![IdSpan::new(10, 2, 3)]))?;
    let a12 = a.export(ExportMode::updates_in_range(vec![IdSpan::new(10, 1, 3)]))?;

    let b = LoroDoc::new();
    b.set_peer_id(20)?;
    b.import(&a0)?;
    b.get_map("root").insert("b0", 0)?;
    b.commit();
    let b0 = b.export(ExportMode::updates_in_range(vec![IdSpan::new(20, 0, 1)]))?;
    b.import(&a1)?;
    b.get_map("root").insert("b1", 1)?;
    b.commit();
    let b1 = b.export(ExportMode::updates_in_range(vec![IdSpan::new(20, 1, 2)]))?;

    let c = LoroDoc::new();
    c.set_peer_id(30)?;
    c.import(&b.export(ExportMode::all_updates())?)?;
    c.get_map("root").insert("c0", 0)?;
    c.commit();
    let c0 = c.export(ExportMode::updates_in_range(vec![IdSpan::new(30, 0, 1)]))?;

    let doc = LoroDoc::new();
    for bytes in [&a2, &b1, &c0, &a12, &a2, &b1] {
        let status = doc.import(bytes)?;
        assert!(status.pending.is_some());
        assert!(status.success.is_empty());
    }
    assert_eq!(doc.oplog_vv(), VersionVector::default());
    let restored = restore_with_pending(&doc)?;
    // The sidecar has exact coverage even with duplicates and overlapping inputs.
    let initial_sidecar = restored.export_pending_updates()?.unwrap();
    let initial_meta = LoroDoc::decode_import_blob_meta(&initial_sidecar, true)?;
    assert_eq!(initial_meta.partial_start_vv.get(&10), Some(&1));
    assert_eq!(initial_meta.partial_end_vv.get(&10), Some(&3));
    assert_eq!(initial_meta.partial_start_vv.get(&20), Some(&1));
    assert_eq!(initial_meta.partial_end_vv.get(&20), Some(&2));
    assert_eq!(initial_meta.partial_start_vv.get(&30), Some(&0));
    assert_eq!(initial_meta.partial_end_vv.get(&30), Some(&1));

    restored.import(&a0)?;
    assert_eq!(restored.oplog_vv().get(&10), Some(&3));
    assert!(restored.oplog_vv().get(&20).is_none());
    let remaining = restored.export_pending_updates()?.unwrap();
    let remaining_meta = LoroDoc::decode_import_blob_meta(&remaining, true)?;
    assert!(remaining_meta.partial_end_vv.get(&10).is_none());
    assert_eq!(remaining_meta.partial_start_vv.get(&20), Some(&1));
    assert_eq!(remaining_meta.partial_end_vv.get(&30), Some(&1));
    // A new snapshot and sidecar can be saved halfway through dependency recovery.
    let restored_again = restore_with_pending(&restored)?;
    for target in [&restored, &restored_again] {
        target.import(&remaining)?;
        target.import(&b0)?;
        assert_no_pending(target)?;
        assert_eq!(target.oplog_vv().get(&10), Some(&3));
        assert_eq!(target.oplog_vv().get(&20), Some(&2));
        assert_eq!(target.oplog_vv().get(&30), Some(&1));
        let expected = serde_json::json!({"root": {
            "a0": 0, "a1": 1, "a2": 2, "b0": 0, "b1": 1, "c0": 0
        }});
        use loro::ToJson;
        assert_eq!(target.get_deep_value().to_json_value(), expected);
        target.import(&initial_sidecar)?;
        assert_no_pending(target)?;
    }
    Ok(())
}

#[test]
fn disjoint_pending_ranges_do_not_export_the_missing_gap() -> anyhow::Result<()> {
    let source = LoroDoc::new();
    source.set_peer_id(1)?;
    for (i, letter) in ["A", "B", "C", "D"].into_iter().enumerate() {
        source.get_text("text").insert(i, letter)?;
        source.commit();
    }
    let updates = (0..4)
        .map(|counter| {
            source.export(ExportMode::updates_in_range(vec![IdSpan::new(
                1,
                counter,
                counter + 1,
            )]))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let doc = LoroDoc::new();
    doc.import(&updates[1])?;
    doc.import(&updates[3])?;
    let restored = restore_with_pending(&doc)?;
    restored.import(&updates[0])?;
    assert_eq!(restored.get_text("text").to_string(), "AB");
    assert_eq!(restored.oplog_vv().get(&1), Some(&2));
    let sidecar = restored.export_pending_updates()?.unwrap();
    let meta = LoroDoc::decode_import_blob_meta(&sidecar, true)?;
    assert_eq!(meta.partial_start_vv.get(&1), Some(&3));
    assert_eq!(meta.partial_end_vv.get(&1), Some(&4));
    let restored_again = restore_with_pending(&restored)?;
    assert_eq!(restored_again.get_text("text").to_string(), "AB");
    restored_again.import(&updates[2])?;
    assert_eq!(restored_again.get_text("text").to_string(), "ABCD");
    assert_no_pending(&restored_again)?;
    Ok(())
}

#[test]
fn local_uncommitted_operations_are_not_remote_pending() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    doc.get_text("text").insert(0, "local")?;
    let txn_len = doc.get_pending_txn_len();
    assert!(txn_len > 0);
    let vv = doc.oplog_vv();
    let frontiers = doc.state_frontiers();
    assert_no_pending(&doc)?;
    assert_eq!(doc.get_pending_txn_len(), txn_len);
    assert_eq!(doc.oplog_vv(), vv);
    assert_eq!(doc.state_frontiers(), frontiers);
    doc.commit();
    assert_eq!(doc.get_text("text").to_string(), "local");
    Ok(())
}

#[test]
fn export_remote_pending_does_not_commit_a_local_transaction() -> anyhow::Result<()> {
    let source = LoroDoc::new();
    source.set_peer_id(1)?;
    source.get_text("text").insert(0, "A")?;
    source.commit();
    let a = source.export(ExportMode::all_updates())?;
    let vv_a = source.oplog_vv();
    source.get_text("text").insert(1, "B")?;
    source.commit();
    let b = source.export(ExportMode::updates(&vv_a))?;

    let doc = LoroDoc::new();
    doc.set_peer_id(2)?;
    doc.import(&b)?;
    doc.get_map("local").insert("draft", "uncommitted")?;
    let txn_len = doc.get_pending_txn_len();
    let vv = doc.oplog_vv();
    let state_frontiers = doc.state_frontiers();
    let oplog_frontiers = doc.oplog_frontiers();
    assert!(txn_len > 0);
    let sidecar = doc.export_pending_updates()?.unwrap();
    assert_eq!(doc.get_pending_txn_len(), txn_len);
    assert_eq!(doc.oplog_vv(), vv);
    assert_eq!(doc.state_frontiers(), state_frontiers);
    assert_eq!(doc.oplog_frontiers(), oplog_frontiers);
    let restored = LoroDoc::new();
    restored.import(&sidecar)?;
    restored.import(&a)?;
    assert_eq!(restored.get_text("text").to_string(), "AB");
    assert!(restored.get_map("local").get("draft").is_none());
    doc.commit();
    assert_eq!(doc.get_pending_txn_len(), 0);
    assert!(doc.get_map("local").get("draft").is_some());
    doc.import(&a)?;
    assert_eq!(doc.get_text("text").to_string(), "AB");
    assert_no_pending(&doc)?;
    Ok(())
}

#[test]
fn exporting_pending_preserves_detached_state() -> anyhow::Result<()> {
    let source = LoroDoc::new();
    source.set_peer_id(1)?;
    source.get_text("text").insert(0, "base|")?;
    source.commit();
    let base = source.export(ExportMode::Snapshot)?;
    let base_vv = source.oplog_vv();
    source.get_text("text").insert(5, "A")?;
    source.commit();
    let a = source.export(ExportMode::updates(&base_vv))?;
    let a_vv = source.oplog_vv();
    source.get_text("text").insert(6, "B")?;
    source.commit();
    let b = source.export(ExportMode::updates(&a_vv))?;

    let doc = LoroDoc::new();
    doc.import(&base)?;
    doc.detach();
    doc.import(&b)?;
    let vv = doc.oplog_vv();
    let state_frontiers = doc.state_frontiers();
    let sidecar = doc.export_pending_updates()?.unwrap();
    assert!(doc.is_detached());
    assert_eq!(doc.oplog_vv(), vv);
    assert_eq!(doc.state_frontiers(), state_frontiers);
    assert_eq!(doc.get_text("text").to_string(), "base|");
    doc.import(&a)?;
    assert!(doc.is_detached());
    assert_eq!(doc.get_text("text").to_string(), "base|");
    assert_no_pending(&doc)?;
    doc.checkout_to_latest();
    assert_eq!(doc.get_text("text").to_string(), "base|AB");

    let restored = LoroDoc::new();
    restored.import(&base)?;
    restored.import(&sidecar)?;
    restored.import(&a)?;
    assert_eq!(restored.get_text("text").to_string(), "base|AB");
    assert_no_pending(&restored)?;
    Ok(())
}

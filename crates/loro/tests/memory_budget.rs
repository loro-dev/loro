use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

use loro::{
    CommitOptions, DocMemoryStats, ExportMode, ImportBlobOutcome, LoroDoc, LoroError, LoroMap,
    MemoryBudget, MemoryLimitExceeded, MemoryLimitKind, UndoManager,
};

fn sum_ram_components(stats: &DocMemoryStats) -> usize {
    stats.state_bytes
        + stats.value_cache_bytes
        + stats.kv_bytes
        + stats.block_cache_bytes
        + stats.change_cache_bytes
        + stats.dag_bytes
        + stats.arena_bytes
        + stats.pending_bytes
        + stats.history_cache_bytes
        + stats.diff_calc_bytes
        + stats.undo_bytes
        + stats.uncommitted_txn_bytes
}

fn assert_limit_err(err: &LoroError, expected_kind: MemoryLimitKind) -> MemoryLimitExceeded {
    assert!(
        err.is_memory_limit_exceeded(),
        "expected MemoryLimitExceeded, got {err:?}"
    );
    let info = err
        .memory_limit_info()
        .expect("memory_limit_info should parse");
    assert_eq!(info.kind, expected_kind);
    assert!(info.used_bytes > info.limit_bytes);
    info
}

#[test]
fn memory_stats_breakdown_and_no_double_counting() {
    let src = LoroDoc::new();
    src.set_peer_id(1).unwrap();
    for i in 0..16 {
        let map = src.get_map(format!("map_{i}"));
        map.insert("payload", format!("value_{i}_{}", "x".repeat(128)))
            .unwrap();
    }
    let text = src.get_text("body");
    text.insert(0, &"hello world ".repeat(50)).unwrap();
    let list = src.get_list("items");
    list.push(1i64).unwrap();
    list.push("two").unwrap();
    let mlist = src.get_movable_list("mv");
    mlist.push("alpha").unwrap();
    let tree = src.get_tree("tree");
    let root = tree.create(None).unwrap();
    let meta = tree.get_meta(root).unwrap();
    meta.insert("label", "root").unwrap();
    src.commit();

    let stats_src = src.memory_stats();
    assert_eq!(stats_src.spilled_disk_bytes, 0);
    assert_eq!(stats_src.total_bytes, sum_ram_components(&stats_src));
    assert!(stats_src.state_bytes > 0);
    assert!(stats_src.change_cache_bytes > 0);
    assert!(stats_src.dag_bytes > 0);
    assert!(stats_src.arena_bytes > 0);
    // Active materialized containers have no duplicate value-only wrapper cache.
    assert_eq!(stats_src.value_cache_bytes, 0);

    // Reading cold containers via shallow value access populates decoded wrappers
    // in `value_cache_bytes` without double-counting them in `state_bytes`.
    let snap = src.export(ExportMode::Snapshot).unwrap();
    let dst = LoroDoc::new();
    dst.import(&snap).unwrap();

    let before_read = dst.memory_stats();
    assert_eq!(before_read.total_bytes, sum_ram_components(&before_read));
    assert!(before_read.kv_bytes > 0);

    let deep = dst.get_deep_value();
    assert_eq!(deep, src.get_deep_value());
    let after_read = dst.memory_stats();
    assert_eq!(after_read.total_bytes, sum_ram_components(&after_read));

    // Reading deep value caches value wrappers cleanly without inflating `state_bytes`
    // for containers that remained value-only, and capping value cache bounds it.
    dst.set_memory_budget(MemoryBudget::new().with_max_decoded_value_cache_bytes(1));
    dst.reclaim_memory();
    let after_evict = dst.memory_stats();
    assert_eq!(after_evict.total_bytes, sum_ram_components(&after_evict));
    assert!(after_evict.value_cache_bytes <= after_read.value_cache_bytes);
    assert_eq!(dst.get_deep_value(), src.get_deep_value());
}

#[test]
fn stage1_2_watermark_hysteresis_cold_demotion_rehydration_and_empty_deleted_exclusion() {
    let src = LoroDoc::new();
    src.set_peer_id(1).unwrap();
    for i in 0..32 {
        let m = src.get_map(format!("hot_cold_{i}"));
        m.insert("data", format!("container_{i}_{}", "a".repeat(256)))
            .unwrap();
    }
    // Empty root container: handle acquired but no entries.
    let _empty = src.get_map("empty_root");
    // Deleted child container: created under parent map and then deleted.
    let parent = src.get_map("parent_with_deleted");
    let deleted_child = parent
        .insert_container("ephemeral", LoroMap::new())
        .unwrap();
    deleted_child.insert("temp", "z".repeat(512)).unwrap();
    parent.delete("ephemeral").unwrap();
    src.commit();

    let expected = src.get_deep_value();
    let snap = src.export(ExportMode::Snapshot).unwrap();

    let doc = LoroDoc::new();
    doc.set_peer_id(2).unwrap();
    doc.import(&snap).unwrap();

    let ref_doc = LoroDoc::new();
    ref_doc.set_peer_id(2).unwrap();
    ref_doc.import(&snap).unwrap();

    // Materialize all 32 cold containers into full state + dirty writes so they
    // start as materialized states in `DocState`.
    for i in 0..32 {
        doc.get_map(format!("hot_cold_{i}"))
            .insert("touched", i as i64)
            .unwrap();
        ref_doc
            .get_map(format!("hot_cold_{i}"))
            .insert("touched", i as i64)
            .unwrap();
    }
    // Touch empty_root so handle exists.
    let _empty_handle = doc.get_map("empty_root");
    doc.commit();
    ref_doc.commit();

    assert_ne!(doc.get_deep_value(), expected);

    let full_stats = doc.memory_stats();
    assert!(full_stats.state_bytes > 0);

    // Hysteresis check: while total_bytes <= high_watermark_bytes, committing does NOT demote state.
    doc.set_memory_budget(
        MemoryBudget::new()
            .with_max_doc_bytes(full_stats.total_bytes.saturating_mul(4))
            .with_watermarks(
                full_stats.total_bytes + 50_000,
                full_stats.total_bytes + 25_000,
            ),
    );
    doc.get_map("hot_cold_0")
        .insert("hysteresis_probe", 1i64)
        .unwrap();
    doc.commit();
    ref_doc
        .get_map("hot_cold_0")
        .insert("hysteresis_probe", 1i64)
        .unwrap();
    ref_doc.commit();
    let before_crossing = doc.memory_stats();
    assert!(
        before_crossing.state_bytes >= full_stats.state_bytes,
        "expected no premature demotion below high watermark: before={} probe={}",
        full_stats.state_bytes,
        before_crossing.state_bytes
    );

    // Configure watermarks with `target` well below post-Stage-1 bytes so Stage 2 must
    // demote cold containers into KV, while keeping `high` above post-demotion size for hysteresis.
    let post_stage1_non_state = before_crossing.kv_bytes
        + before_crossing.change_cache_bytes
        + before_crossing.dag_bytes
        + before_crossing.arena_bytes;
    let target = post_stage1_non_state + before_crossing.state_bytes / 3;
    let high = (post_stage1_non_state + before_crossing.state_bytes * 9 / 10).max(target);
    doc.set_memory_budget(
        MemoryBudget::new()
            .with_max_doc_bytes(before_crossing.total_bytes.saturating_mul(4))
            .with_watermarks(high, target)
            .with_max_kv_block_cache_bytes(1024),
    );

    // Commit after crossing high watermark keeps cold state demoted cleanly.
    doc.get_map("hot_cold_0").insert("bump", 1i64).unwrap();
    doc.commit();
    ref_doc.get_map("hot_cold_0").insert("bump", 1i64).unwrap();
    ref_doc.commit();

    let demoted_stats = doc.memory_stats();
    assert!(
        demoted_stats.state_bytes < before_crossing.state_bytes,
        "expected cold-state demotion to lower state_bytes: before={} after={}",
        before_crossing.state_bytes,
        demoted_stats.state_bytes
    );

    // If demotion brought total_bytes below `high_watermark_bytes`, a tiny reclaim
    // is already satisfied cleanly. Either way, verify explicit reclaim respects budget
    // and direct re-hydration works identically on demoted containers.
    let final_stats = doc.try_reclaim_to_budget().unwrap();
    assert_eq!(final_stats.total_bytes, sum_ram_components(&final_stats));

    // Reading both an individual cold container and the full document re-hydrates accurately.
    let m15_val = doc.get_map("hot_cold_15").get_value();
    assert!(m15_val.as_map().is_some());
    assert_eq!(
        m15_val.as_map().unwrap().get("touched").unwrap(),
        &loro::LoroValue::I64(15)
    );
    assert_eq!(doc.get_deep_value(), ref_doc.get_deep_value());

    // Empty root and deleted child must not appear with phantom data.
    let deep_map = doc.get_deep_value();
    let root_m = deep_map.as_map().unwrap();
    assert!(root_m
        .get("empty_root")
        .unwrap()
        .as_map()
        .unwrap()
        .is_empty());
    assert!(root_m
        .get("parent_with_deleted")
        .unwrap()
        .as_map()
        .unwrap()
        .is_empty());
}

#[test]
fn stage3_import_blob_rejection() {
    let peer = LoroDoc::new();
    peer.set_peer_id(1).unwrap();
    peer.get_text("t").insert(0, "ok").unwrap();
    peer.commit();
    let small_blob = peer.export(ExportMode::all_updates()).unwrap();
    let small_vv = peer.oplog_vv();

    peer.get_text("t")
        .insert(2, &"big_payload_".repeat(40))
        .unwrap();
    peer.commit();
    let big_blob = peer.export(ExportMode::updates(&small_vv)).unwrap();
    assert!(big_blob.len() > small_blob.len());

    let json_schema = peer
        .export_json_updates(&small_vv, &peer.oplog_vv())
        .unwrap();
    let json_str = serde_json::to_string(&json_schema).unwrap();
    assert!(json_str.len() > 64);

    let limit = small_blob.len().max(16);
    assert!(big_blob.len() > limit);

    let doc = LoroDoc::new();
    doc.set_peer_id(2).unwrap();
    doc.set_memory_budget(MemoryBudget::new().with_max_import_blob_bytes(limit));

    let events = Arc::new(AtomicUsize::new(0));
    let events_c = Arc::clone(&events);
    let _sub = doc.subscribe_root(Arc::new(move |_| {
        events_c.fetch_add(1, Ordering::SeqCst);
    }));

    // (a) Single import rejects oversized blob before decode.
    let err = doc.import(&big_blob).unwrap_err();
    let info = assert_limit_err(&err, MemoryLimitKind::ImportBlob);
    assert_eq!(info.used_bytes, big_blob.len());
    assert_eq!(info.limit_bytes, limit);
    assert_eq!(doc.get_text("t").to_string(), "");
    assert_eq!(events.load(Ordering::SeqCst), 0);

    // (b) Batch import rolls back the entire batch on ImportBlob error.
    let err = doc
        .import_batch(&[small_blob.clone(), big_blob.clone()])
        .unwrap_err();
    assert_limit_err(&err, MemoryLimitKind::ImportBlob);
    assert_eq!(doc.get_text("t").to_string(), "");
    assert!(doc.oplog_vv().is_empty());
    assert_eq!(events.load(Ordering::SeqCst), 0);

    // (c) Detailed batch import isolates per-blob ImportBlob failure and keeps small_blob.
    let detail = doc
        .import_batch_detailed(&[small_blob.clone(), big_blob.clone()])
        .unwrap();
    assert!(detail.rollback_error.is_none());
    assert!(matches!(detail.blobs[0], ImportBlobOutcome::Imported(_)));
    match &detail.blobs[1] {
        ImportBlobOutcome::Failed(e) => {
            assert_limit_err(e, MemoryLimitKind::ImportBlob);
        }
        other => panic!("expected Failed(ImportBlob), got {other:?}"),
    }
    assert_eq!(doc.get_text("t").to_string(), "ok");
    assert!(events.load(Ordering::SeqCst) > 0);

    // (d) JSON updates import respects max_import_blob_bytes before parsing/applying ops.
    let events_before = events.load(Ordering::SeqCst);
    let err = doc.import_json_updates(json_str.as_str()).unwrap_err();
    assert_limit_err(&err, MemoryLimitKind::ImportBlob);
    assert_eq!(doc.get_text("t").to_string(), "ok");
    assert_eq!(events.load(Ordering::SeqCst), events_before);
}

#[test]
fn stage3_pending_pool_rejection_and_rollback() {
    let peer = LoroDoc::new();
    peer.set_peer_id(1).unwrap();
    peer.get_text("t").insert(0, "base ").unwrap();
    peer.commit();
    let v0 = peer.oplog_vv();
    let u0 = peer.export(ExportMode::all_updates()).unwrap();

    peer.get_text("t").insert(5, "small ").unwrap();
    peer.commit();
    let v1 = peer.oplog_vv();
    let u1_small = peer.export(ExportMode::updates(&v0)).unwrap();

    // Build a multi-change blob whose second change pushes pending_bytes over limit mid-blob.
    for i in 0..8 {
        peer.get_text("t")
            .insert(11 + i, &"big_chunk_".repeat(16))
            .unwrap();
        peer.commit();
    }
    let u2_multi_big = peer.export(ExportMode::updates(&v1)).unwrap();

    // Also build a tiny u2_small from a separate doc starting at v1? Simpler: first sync up to u1,
    // verify rollback on u2_multi_big preserves u1 cleanly!
    let receiver = LoroDoc::new();
    receiver.set_peer_id(2).unwrap();
    // Configure generous initial budget to measure u1_small pending cost, then tighten before u2_multi_big.
    let events = Arc::new(AtomicUsize::new(0));
    let events_c = Arc::clone(&events);
    let _sub = receiver.subscribe_root(Arc::new(move |_| {
        events_c.fetch_add(1, Ordering::SeqCst);
    }));

    // Import u1 without u0 so u1 is parked as pending.
    let status1 = receiver.import(&u1_small).unwrap();
    assert!(status1.pending.is_some());
    let pending_after_u1 = receiver.memory_stats().pending_bytes;
    assert!(pending_after_u1 > 0);
    assert_eq!(events.load(Ordering::SeqCst), 0);

    // Now set max_pending_bytes slightly above pending_after_u1 so u2_multi_big triggers PendingPool.
    let pending_limit = pending_after_u1 + 64;
    receiver.set_memory_budget(MemoryBudget::new().with_max_pending_bytes(pending_limit));

    let err = receiver.import(&u2_multi_big).unwrap_err();
    assert_limit_err(&err, MemoryLimitKind::PendingPool);

    // Mid-blob rollback must restore pending pool & vv back to just u1_small!
    assert_eq!(receiver.memory_stats().pending_bytes, pending_after_u1);
    assert_eq!(events.load(Ordering::SeqCst), 0);

    // Now missing u0 arrives -> converges to u0 + u1_small (nothing from rolled-back u2_multi_big).
    receiver.import(&u0).unwrap();
    assert_eq!(receiver.get_text("t").to_string(), "base small ");
    assert_eq!(receiver.memory_stats().pending_bytes, 0);
    assert!(events.load(Ordering::SeqCst) > 0);
}

#[test]
fn stage3_doc_budget_import_and_initial_snapshot_atomic_rollback() {
    let peer_p = LoroDoc::new();
    peer_p.set_peer_id(10).unwrap();
    peer_p.get_text("p").insert(0, "p_base").unwrap();
    peer_p.commit();
    let vp = peer_p.oplog_vv();
    peer_p.get_text("p").insert(6, "p_pending").unwrap();
    peer_p.commit();
    let parked_from_p = peer_p.export(ExportMode::updates(&vp)).unwrap();

    // Construct source doc with substantial payload for Snapshot & ShallowSnapshot.
    let src = LoroDoc::new();
    src.set_peer_id(1).unwrap();
    for i in 0..20 {
        src.get_map(format!("s_{i}"))
            .insert("v", "payload_".repeat(64))
            .unwrap();
    }
    src.commit();
    let f_mid = src.oplog_frontiers();
    src.get_text("extra")
        .insert(0, &"tail_".repeat(128))
        .unwrap();
    src.commit();

    let full_snap = src.export(ExportMode::Snapshot).unwrap();
    let shallow_snap = src.export(ExportMode::shallow_snapshot(&f_mid)).unwrap();
    let big_update = src.export(ExportMode::all_updates()).unwrap();

    // (a) Non-empty doc `import(&big_update)` rollback under tight `max_doc_bytes`.
    {
        let doc = LoroDoc::new();
        doc.set_peer_id(2).unwrap();
        let keep_txt = doc.get_text("keep");
        keep_txt.insert(0, "baseline").unwrap();
        doc.commit();
        let baseline_stats = doc.memory_stats();
        let baseline_val = doc.get_deep_value();
        let baseline_frontiers = doc.oplog_frontiers();

        let tight = baseline_stats.total_bytes + 512;
        doc.set_memory_budget(MemoryBudget::new().with_max_doc_bytes(tight));

        let events = Arc::new(AtomicUsize::new(0));
        let events_c = Arc::clone(&events);
        let _sub = doc.subscribe_root(Arc::new(move |_| {
            events_c.fetch_add(1, Ordering::SeqCst);
        }));

        let err = doc.import(&big_update).unwrap_err();
        assert_limit_err(&err, MemoryLimitKind::DocBudget);
        assert_eq!(doc.get_deep_value(), baseline_val);
        assert_eq!(doc.oplog_frontiers(), baseline_frontiers);
        assert_eq!(keep_txt.to_string(), "baseline");
        assert_eq!(events.load(Ordering::SeqCst), 0);
    }

    // (b) Initial Snapshot & ShallowSnapshot preserving pre-existing empty handles and parked pendings.
    for blob in [&full_snap, &shallow_snap] {
        let doc = LoroDoc::new();
        doc.set_peer_id(3).unwrap();
        let keep_handle = doc.get_text("keep_empty");
        doc.import(&parked_from_p).unwrap();
        let pending_before = doc.memory_stats().pending_bytes;
        assert!(pending_before > 0);

        let tight = doc.memory_stats().total_bytes + 512;
        doc.set_memory_budget(MemoryBudget::new().with_max_doc_bytes(tight));

        let events = Arc::new(AtomicUsize::new(0));
        let events_c = Arc::clone(&events);
        let _sub = doc.subscribe_root(Arc::new(move |_| {
            events_c.fetch_add(1, Ordering::SeqCst);
        }));

        let err = doc.import(blob).unwrap_err();
        assert_limit_err(&err, MemoryLimitKind::DocBudget);
        assert_eq!(doc.memory_stats().pending_bytes, pending_before);
        assert!(doc.oplog_vv().is_empty());
        assert_eq!(events.load(Ordering::SeqCst), 0);

        // Pre-existing handle stays valid and usable!
        doc.set_memory_budget(MemoryBudget::new());
        keep_handle.insert(0, "still_valid").unwrap();
        doc.commit();
        assert_eq!(doc.get_text("keep_empty").to_string(), "still_valid");
    }
}

#[test]
fn stage3_doc_budget_import_batch_and_detailed_rollback() {
    let peer = LoroDoc::new();
    peer.set_peer_id(1).unwrap();
    peer.get_text("t").insert(0, "tiny").unwrap();
    peer.commit();
    let v1 = peer.oplog_vv();
    let u1 = peer.export(ExportMode::all_updates()).unwrap();

    for i in 0..16 {
        peer.get_map(format!("big_{i}"))
            .insert("k", "large_value_".repeat(80))
            .unwrap();
    }
    peer.commit();
    let u2 = peer.export(ExportMode::updates(&v1)).unwrap();

    // import_batch rollback
    {
        let doc = LoroDoc::new();
        doc.set_peer_id(2).unwrap();
        doc.get_text("local").insert(0, "init").unwrap();
        doc.commit();
        let before = doc.get_deep_value();
        let tight = doc.memory_stats().total_bytes + u1.len() + 1024;
        doc.set_memory_budget(MemoryBudget::new().with_max_doc_bytes(tight));

        let events = Arc::new(AtomicUsize::new(0));
        let events_c = Arc::clone(&events);
        let _sub = doc.subscribe_root(Arc::new(move |_| {
            events_c.fetch_add(1, Ordering::SeqCst);
        }));

        let err = doc.import_batch(&[u1.clone(), u2.clone()]).unwrap_err();
        assert_limit_err(&err, MemoryLimitKind::DocBudget);
        assert!(!doc.is_detached());
        assert_eq!(doc.get_deep_value(), before);
        assert_eq!(events.load(Ordering::SeqCst), 0);
    }

    // import_batch_detailed rollback
    {
        let doc = LoroDoc::new();
        doc.set_peer_id(3).unwrap();
        doc.get_text("local").insert(0, "init").unwrap();
        doc.commit();
        let before = doc.get_deep_value();
        let tight = doc.memory_stats().total_bytes + u1.len() + 1024;
        doc.set_memory_budget(MemoryBudget::new().with_max_doc_bytes(tight));

        let events = Arc::new(AtomicUsize::new(0));
        let events_c = Arc::clone(&events);
        let _sub = doc.subscribe_root(Arc::new(move |_| {
            events_c.fetch_add(1, Ordering::SeqCst);
        }));

        let detail = doc.import_batch_detailed(&[u1, u2]).unwrap();
        let rb_err = detail
            .rollback_error
            .as_ref()
            .expect("rollback_error expected");
        assert_limit_err(rb_err, MemoryLimitKind::DocBudget);
        assert!(detail.status.success.is_empty());
        assert_eq!(detail.blobs.len(), 2);
        assert!(detail
            .blobs
            .iter()
            .all(|b| matches!(b, ImportBlobOutcome::RolledBack)));
        assert!(!doc.is_detached());
        assert_eq!(doc.get_deep_value(), before);
        assert_eq!(events.load(Ordering::SeqCst), 0);
    }
}

#[test]
fn stage3_doc_budget_try_commit_and_manual_txn_rollback() {
    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    let t = doc.get_text("t");
    t.insert(0, "start").unwrap();
    doc.try_commit().unwrap();

    let baseline = doc.memory_stats();
    let tight = baseline.total_bytes + 1024;
    doc.set_memory_budget(MemoryBudget::new().with_max_doc_bytes(tight));

    let events = Arc::new(AtomicUsize::new(0));
    let events_c = Arc::clone(&events);
    let _sub = doc.subscribe_root(Arc::new(move |_| {
        events_c.fetch_add(1, Ordering::SeqCst);
    }));

    // Over-budget uncommitted edit rolled back cleanly on try_commit / try_commit_with.
    t.insert(5, &"too_large_".repeat(128)).unwrap();
    let err = doc
        .try_commit_with(CommitOptions::new().origin("batch"))
        .unwrap_err();
    assert_limit_err(&err, MemoryLimitKind::DocBudget);
    assert_eq!(t.to_string(), "start");
    assert_eq!(doc.memory_stats().uncommitted_txn_bytes, 0);
    assert_eq!(events.load(Ordering::SeqCst), 0);

    // Retry with a smaller edit within budget -> succeeds and notifies subscriber.
    t.insert(5, "!").unwrap();
    doc.try_commit().unwrap();
    assert_eq!(t.to_string(), "start!");
    assert_eq!(events.load(Ordering::SeqCst), 1);

    // Manual `loro_internal::LoroDoc::txn()` + `Transaction::commit()` rolls back on DocBudget.
    let idoc = loro_internal::LoroDoc::new();
    idoc.set_peer_id(2).unwrap();
    {
        let mut txn = idoc.txn().unwrap();
        let text = idoc.get_text("m");
        text.insert_with_txn(&mut txn, 0, "base", loro_internal::cursor::PosType::Unicode)
            .unwrap();
        txn.commit().unwrap();
    }
    let ibase = idoc.memory_stats().total_bytes;
    idoc.set_memory_budget(MemoryBudget::new().with_max_doc_bytes(ibase + 512));

    let err = {
        let mut txn = idoc.txn().unwrap();
        let text = idoc.get_text("m");
        text.insert_with_txn(
            &mut txn,
            4,
            &"oversized_".repeat(128),
            loro_internal::cursor::PosType::Unicode,
        )
        .unwrap();
        txn.commit().unwrap_err()
    };
    assert_limit_err(&err, MemoryLimitKind::DocBudget);
    assert_eq!(idoc.get_text("m").to_string(), "base");
}

#[test]
fn stage3_doc_budget_nested_apply_diff_failure_preserves_outer_txn() {
    let src = LoroDoc::new();
    src.set_peer_id(10).unwrap();
    let src_t = src.get_text("t");
    src_t.insert(0, "a").unwrap();
    src.commit();
    let f_start = src.oplog_frontiers();
    for i in 0..12 {
        src.get_map(format!("diff_m_{i}"))
            .insert("big", "diff_payload_".repeat(64))
            .unwrap();
    }
    src.commit();
    let f_end = src.oplog_frontiers();
    let big_diff = src.diff(&f_start, &f_end).unwrap();

    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    let t = doc.get_text("t");
    t.insert(0, "base").unwrap();
    doc.try_commit().unwrap();

    let base_bytes = doc.memory_stats().total_bytes;
    doc.set_memory_budget(MemoryBudget::new().with_max_doc_bytes(base_bytes + 2048));

    let events = Arc::new(AtomicUsize::new(0));
    let events_c = Arc::clone(&events);
    let _sub = doc.subscribe_root(Arc::new(move |_| {
        events_c.fetch_add(1, Ordering::SeqCst);
    }));

    // Outer uncommitted edit stays open (`budget_base_scope` active).
    t.insert(4, "+keep").unwrap();

    // Inner `apply_diff` breaches `max_doc_bytes`: inner scope rolls back while outer `"base+keep"` remains.
    let err = doc.apply_diff(big_diff).unwrap_err();
    assert_limit_err(&err, MemoryLimitKind::DocBudget);
    assert_eq!(events.load(Ordering::SeqCst), 0);
    assert_eq!(t.to_string(), "base+keep");

    // Committing outer edit succeeds and fires exactly one event.
    doc.try_commit().unwrap();
    assert_eq!(t.to_string(), "base+keep");
    assert_eq!(events.load(Ordering::SeqCst), 1);
    assert!(doc
        .get_deep_value()
        .as_map()
        .unwrap()
        .get("diff_m_0")
        .is_none());
}

#[test]
fn undo_manager_remote_skip_and_byte_quota() {
    let doc_a = LoroDoc::new();
    doc_a.set_peer_id(1).unwrap();
    let doc_b = LoroDoc::new();
    doc_b.set_peer_id(2).unwrap();

    let text_a = doc_a.get_text("target");
    text_a.insert(0, "init").unwrap();
    doc_a.get_map("unrelated").insert("x", 0i64).unwrap();
    doc_a.commit();
    doc_b
        .import(&doc_a.export(ExportMode::all_updates()).unwrap())
        .unwrap();

    let mut undo_a = UndoManager::new(&doc_a);
    undo_a.set_merge_interval(0);

    // Local step 1 on "target".
    text_a.insert(4, " step1").unwrap();
    doc_a.commit();
    let before_remote = undo_a.used_bytes();
    assert!(before_remote > 0);

    // Peer B performs large edits exclusively on "unrelated" container and syncs to doc_a.
    let v_before = doc_b.oplog_vv();
    for i in 0..20 {
        doc_b
            .get_map("unrelated")
            .insert(&format!("k_{i}"), "remote_noise_".repeat(64))
            .unwrap();
    }
    doc_b.commit();
    doc_a
        .import(&doc_b.export(ExportMode::updates(&v_before)).unwrap())
        .unwrap();

    // UndoManager skips transforming/storing unrelated remote container diffs.
    assert_eq!(undo_a.used_bytes(), before_remote);

    // Multiple local steps + explicit undo byte cap evict older items.
    for i in 0..10 {
        let len = text_a.len_unicode();
        text_a
            .insert(len, &format!(" [s{i}:{}]", "u".repeat(64)))
            .unwrap();
        doc_a.commit();
    }
    let multi_bytes = undo_a.used_bytes();
    assert!(multi_bytes > before_remote);

    let cap = multi_bytes / 3;
    undo_a.set_max_undo_bytes(cap);
    assert_eq!(undo_a.max_undo_bytes(), cap);
    assert!(
        undo_a.used_bytes() <= cap,
        "undo used_bytes={} exceeded cap={}",
        undo_a.used_bytes(),
        cap
    );

    // Document-level reclaim_memory also enforces UndoManager max_undo_bytes via doc.undo_managers.
    let smaller_cap = cap / 2;
    doc_a.set_memory_budget(MemoryBudget::new().with_max_undo_bytes(smaller_cap));
    for i in 10..16 {
        let len = text_a.len_unicode();
        text_a
            .insert(len, &format!(" [d{i}:{}]", "v".repeat(64)))
            .unwrap();
        doc_a.commit();
    }
    doc_a.reclaim_memory();
    assert!(
        undo_a.used_bytes() <= smaller_cap,
        "doc.reclaim_memory failed to trim registered UndoManager: used={} cap={}",
        undo_a.used_bytes(),
        smaller_cap
    );
}

#[test]
fn soft_only_budget_reclaims_single_and_batch_imports_without_manual_reclaim() {
    let source = LoroDoc::new();
    source.set_peer_id(999).unwrap();
    for i in 0..100 {
        source
            .get_map(format!("soft-{i}"))
            .insert("payload", "x".repeat(128))
            .unwrap();
    }
    source.commit();
    let first = source.export(ExportMode::all_updates()).unwrap();
    let vv = source.oplog_vv();
    source.get_map("soft-0").insert("next", true).unwrap();
    source.commit();
    let second = source.export(ExportMode::updates(&vv)).unwrap();
    for batch in [false, true] {
        let doc = LoroDoc::new();
        doc.set_memory_budget(MemoryBudget::new().with_watermarks(0, 0));
        if batch {
            doc.import_batch(&[first.clone(), second.clone()]).unwrap();
        } else {
            doc.import(&first).unwrap();
            doc.import(&second).unwrap();
        }
        let before_read = doc.memory_stats().state_bytes;
        assert_eq!(doc.get_deep_value(), source.get_deep_value());
        // Materialize all containers and show the import had already released
        // their full states, despite there being no hard document quota.
        for i in 0..100 {
            doc.get_map(format!("soft-{i}"))
                .insert("touch", true)
                .unwrap();
        }
        assert!(doc.memory_stats().state_bytes > before_read);
    }
}

#[test]
fn changing_hard_budget_preserves_pending_edits_until_next_transaction() {
    let doc = LoroDoc::new();
    let text = doc.get_text("text");
    text.insert(0, "before").unwrap();
    doc.set_memory_budget(MemoryBudget::new().with_max_doc_bytes(1));
    // The transaction that already accepted these edits keeps its old quota.
    doc.try_commit().unwrap();
    assert_eq!(text.to_string(), "before");
    let before = doc.oplog_frontiers();
    text.insert(6, "after").unwrap();
    assert_limit_err(&doc.try_commit().unwrap_err(), MemoryLimitKind::DocBudget);
    assert_eq!(text.to_string(), "before");
    assert_eq!(doc.oplog_frontiers(), before);
    doc.set_memory_budget(MemoryBudget::default());
    text.insert(6, "ok").unwrap();
    doc.try_commit().unwrap();
    assert_eq!(text.to_string(), "beforeok");
}

#[test]
fn hard_budget_installed_in_commit_callback_applies_to_next_transaction() {
    let doc = LoroDoc::new();
    let callback_doc = doc.clone();
    let _sub = doc.subscribe_pre_commit(Box::new(move |_| {
        callback_doc.set_memory_budget(MemoryBudget::new().with_max_doc_bytes(1));
        true
    }));
    let text = doc.get_text("text");
    text.insert(0, "before").unwrap();
    doc.try_commit().unwrap();
    assert_eq!(text.to_string(), "before");
    text.insert(6, "after").unwrap();
    assert_limit_err(&doc.try_commit().unwrap_err(), MemoryLimitKind::DocBudget);
    assert_eq!(text.to_string(), "before");
}

#[test]
fn forks_inherit_independent_memory_policies_and_enforce_blob_limits() {
    let doc = LoroDoc::new();
    doc.get_text("text").insert(0, "before").unwrap();
    doc.commit();
    let earlier = doc.oplog_frontiers();
    doc.get_text("text").insert(6, "after").unwrap();
    doc.commit();
    let budget = MemoryBudget::new()
        .with_max_doc_bytes(8 * 1024 * 1024)
        .with_watermarks(7 * 1024 * 1024, 6 * 1024 * 1024)
        .with_max_import_blob_bytes(1)
        .with_max_pending_bytes(4096)
        .with_max_kv_block_cache_bytes(1024)
        .with_max_decoded_value_cache_bytes(1024)
        .with_max_undo_bytes(1024);
    doc.set_memory_budget(budget.clone());
    let update = doc.export(ExportMode::all_updates()).unwrap();
    let forks = [doc.fork(), doc.fork_at(&earlier).unwrap()];
    for fork in forks {
        assert_eq!(fork.memory_budget(), budget);
        assert_limit_err(
            &fork.import(&update).unwrap_err(),
            MemoryLimitKind::ImportBlob,
        );
        fork.set_memory_budget(MemoryBudget::default());
        assert_eq!(doc.memory_budget(), budget);
    }
    doc.checkout(&earlier).unwrap();
    let fork = doc.fork();
    assert_eq!(fork.memory_budget(), budget);
    assert_eq!(fork.get_text("text").to_string(), "before");
}

/// Overwritten map values stay alive in the history ops (`Arc<MapSet>`) and in
/// an open transaction. They count toward the hard quota: twenty 512 KiB
/// overwrites of one key in a single transaction used to be estimated at a few
/// KiB, and `try_commit` accepted about 10 MiB under a 2 MiB quota.
#[test]
fn boxed_op_payloads_count_toward_the_hard_quota() {
    let doc = LoroDoc::new();
    let map = doc.get_map("m");
    map.insert("k", "base").unwrap();
    doc.commit();
    doc.set_memory_budget(MemoryBudget {
        max_doc_bytes: Some(2 << 20),
        ..Default::default()
    });
    for i in 0..20 {
        map.insert("k", format!("{i:08}").repeat(64 * 1024))
            .unwrap();
    }
    let pending = doc.memory_stats();
    assert!(
        pending.uncommitted_txn_bytes >= 20 * 512 * 1024,
        "uncommitted payloads: {}",
        pending.uncommitted_txn_bytes
    );
    let err = doc.try_commit().unwrap_err();
    assert_eq!(
        err.memory_limit_info().map(|info| info.kind),
        Some(MemoryLimitKind::DocBudget)
    );
    assert_eq!(map.get("k").unwrap().into_value().unwrap(), "base".into());

    // The rejected transaction is gone; a small edit still fits.
    map.insert("k", "small").unwrap();
    doc.try_commit().unwrap();
    assert_eq!(map.get("k").unwrap().into_value().unwrap(), "small".into());
}

/// The same payloads committed one by one are counted in the parsed history.
#[test]
fn boxed_op_payloads_in_history_are_counted() {
    let doc = LoroDoc::new();
    let map = doc.get_map("m");
    let before = doc.memory_stats().change_cache_bytes;
    for i in 0..8 {
        map.insert("k", format!("{i:08}").repeat(32 * 1024))
            .unwrap();
        doc.commit();
    }
    let after = doc.memory_stats().change_cache_bytes;
    assert!(
        after - before >= 8 * 256 * 1024,
        "history payloads: {before} -> {after}"
    );
}

#[test]
fn hot_container_anti_thrashing_preserves_data_and_reclaim_memory_still_demotes_all() {
    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    // Use non-RLE-collapsing content so materialized `hot` holds substantial `state_bytes`.
    let pattern: String = (0..4096)
        .map(|i| char::from(b'a' + (i % 26) as u8))
        .collect();
    let hot = doc.get_text("hot");
    hot.insert(0, &pattern).unwrap();
    let cold = doc.get_text("cold");
    cold.insert(0, &pattern).unwrap();
    doc.commit();

    let shadow = LoroDoc::new();
    shadow
        .import(&doc.export(ExportMode::Snapshot).unwrap())
        .unwrap();

    // First drop everything with hard reclaim, then load only `hot` to measure
    // the warm single-container baseline (excluding transient block cache).
    let _ = doc.reclaim_memory();
    assert_eq!(doc.memory_stats().state_bytes, 0);

    hot.insert(0, "w").unwrap();
    doc.commit();
    let stats_warm = doc.memory_stats();
    let base_no_block_cache = stats_warm.total_bytes - stats_warm.block_cache_bytes;
    assert!(stats_warm.state_bytes > 0);

    // Trigger one soft watermark sweep so `hot` is evicted softly into `recently_demoted`.
    doc.set_memory_budget(MemoryBudget {
        max_doc_bytes: Some(8 * 1024 * 1024),
        high_watermark_bytes: Some(base_no_block_cache.saturating_sub(4 * 1024)),
        target_watermark_bytes: Some(base_no_block_cache / 2),
        ..Default::default()
    });
    assert_eq!(
        doc.memory_stats().state_bytes,
        0,
        "initial soft sweep with hot idle should demote `hot` into `recently_demoted`"
    );

    // Reload `hot` within the refault window -> `hot` gains refault protection.
    hot.insert(0, "r").unwrap();
    doc.try_commit().unwrap();
    assert!(
        doc.memory_stats().state_bytes > 0,
        "`hot` must gain refault protection and stay warm"
    );

    for step in 0..8 {
        hot.insert(0, &format!("{step}")).unwrap();
        doc.try_commit().unwrap();
        assert!(
            doc.memory_stats().state_bytes > 0,
            "hot container must not ping-pong evict on soft pressure (commit step {step})"
        );

        let peer = LoroDoc::new();
        peer.set_peer_id(2).unwrap();
        let before = peer.oplog_vv();
        peer.get_text("hot").insert(0, "p").unwrap();
        peer.commit();
        let upd = peer.export(ExportMode::updates(&before)).unwrap();
        doc.import(&upd).unwrap();
        shadow.import(&upd).unwrap();
        assert!(
            doc.memory_stats().state_bytes > 0,
            "hot container stayed warm after import step {step}"
        );
    }

    // Check parity with local + remote edits.
    shadow
        .import(&doc.export(ExportMode::Snapshot).unwrap())
        .unwrap();
    assert_eq!(doc.get_deep_value(), shadow.get_deep_value());

    // Explicit `reclaim_memory()` forces full demotion regardless of refault cooldown.
    let _ = doc.reclaim_memory();
    assert_eq!(doc.memory_stats().state_bytes, 0);
    assert_eq!(doc.get_deep_value(), shadow.get_deep_value());
}

#[test]
fn hard_doc_budget_rescues_past_soft_backoff_on_commit_import_and_batch() {
    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    doc.get_map("irred")
        .insert("k", "x".repeat(48 * 1024))
        .unwrap();
    for i in 0..16 {
        doc.get_text(format!("cold_{i}"))
            .insert(0, &"a".repeat(8 * 1024))
            .unwrap();
    }
    doc.commit();
    let _ = doc.reclaim_memory();
    let reclaimed_base = doc.memory_stats().total_bytes;

    // Inflate RAM by reading cold texts so we pick a reachable `max_doc_bytes` well
    // below the un-reclaimed state/cache total (`< reclaimed + 256 KiB` margin).
    for i in 0..16 {
        assert_eq!(doc.get_text(format!("cold_{i}")).len_unicode(), 8 * 1024);
    }
    let bloated_before = doc.memory_stats().total_bytes;
    let max_doc = reclaimed_base + 64 * 1024;
    assert!(bloated_before > max_doc);
    let _ = doc.reclaim_memory();

    doc.set_memory_budget(MemoryBudget {
        max_doc_bytes: Some(max_doc),
        high_watermark_bytes: Some(1024),
        target_watermark_bytes: Some(512),
        ..Default::default()
    });

    // Arm soft_backoff below `max_doc`.
    doc.get_map("tiny").insert("step", 1i64).unwrap();
    doc.try_commit().unwrap();

    // Inflate caches & states above `max_doc`, then verify `try_commit` rescues.
    for i in 0..16 {
        let _ = doc.get_text(format!("cold_{i}")).len_unicode();
    }
    assert!(doc.memory_stats().total_bytes > max_doc);
    doc.get_map("tiny").insert("step", 2i64).unwrap();
    doc.try_commit()
        .expect("try_commit must bypass soft_backoff and rescue cold state");
    assert!(doc.memory_stats().total_bytes <= max_doc);

    // Inflate again, then verify `import` rescues.
    for i in 0..16 {
        let _ = doc.get_text(format!("cold_{i}")).len_unicode();
    }
    assert!(doc.memory_stats().total_bytes > max_doc);
    let peer = LoroDoc::new();
    peer.set_peer_id(2).unwrap();
    peer.get_map("tiny").insert("remote", 1i64).unwrap();
    peer.commit();
    let u1 = peer.export(ExportMode::all_updates()).unwrap();
    doc.import(&u1)
        .expect("import must bypass soft_backoff and rescue cold state");
    assert!(doc.memory_stats().total_bytes <= max_doc);

    // Inflate again, then verify `import_batch` and `import_batch_detailed` rescue.
    for i in 0..16 {
        let _ = doc.get_text(format!("cold_{i}")).len_unicode();
    }
    assert!(doc.memory_stats().total_bytes > max_doc);
    let mut batch = Vec::new();
    for step in 2..5 {
        let from = peer.oplog_vv();
        peer.get_map("tiny").insert("remote", step as i64).unwrap();
        peer.commit();
        batch.push(peer.export(ExportMode::updates(&from)).unwrap());
    }
    doc.import_batch(&batch)
        .expect("import_batch must bypass soft_backoff and rescue cold state");
    assert!(doc.memory_stats().total_bytes <= max_doc);

    let from = peer.oplog_vv();
    peer.get_map("tiny").insert("remote", 5i64).unwrap();
    peer.commit();
    let u5 = peer.export(ExportMode::updates(&from)).unwrap();
    for i in 0..16 {
        let _ = doc.get_text(format!("cold_{i}")).len_unicode();
    }
    assert!(doc.memory_stats().total_bytes > max_doc);
    let detail = doc
        .import_batch_detailed(&[u5])
        .expect("import_batch_detailed must rescue cold state");
    assert!(detail.rollback_error.is_none());
    assert!(matches!(detail.blobs[0], ImportBlobOutcome::Imported(_)));
}

#[test]
fn rejected_try_commit_cleans_transient_caches_so_later_commits_fit() {
    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    for i in 0..16 {
        doc.get_text(format!("t_{i}"))
            .insert(0, &"m".repeat(16 * 1024))
            .unwrap();
        doc.commit();
    }
    let _ = doc.reclaim_memory();
    let base = doc.memory_stats().total_bytes;

    let max_doc = base + 32 * 1024;
    doc.set_memory_budget(MemoryBudget {
        max_doc_bytes: Some(max_doc),
        high_watermark_bytes: Some(max_doc * 85 / 100),
        target_watermark_bytes: Some(max_doc * 70 / 100),
        ..Default::default()
    });

    // Touch multiple cold texts to trigger history diffing + oversized commit payload.
    doc.get_text("t_0").insert(0, "ok").unwrap();
    doc.get_map("big")
        .insert("blob", "z".repeat(256 * 1024))
        .unwrap();
    let err = doc.try_commit().unwrap_err();
    assert_limit_err(&err, MemoryLimitKind::DocBudget);

    let after_reject = doc.memory_stats();
    assert!(
        after_reject.total_bytes <= max_doc,
        "post-rollback stats exceeded budget: total={} > max_doc={}: {:?}",
        after_reject.total_bytes,
        max_doc,
        after_reject
    );
    assert!(doc.get_map("big").get("blob").is_none());

    // Subsequent valid small commit must succeed without hitting stale cache bloat.
    doc.get_map("small").insert("k", 1i64).unwrap();
    doc.try_commit()
        .expect("small valid commit must succeed after rejected commit cleanup");
}

#[test]
fn hot_map_imports_do_not_ping_pong_when_stage1_clears_under_hard_limit() {
    let src = LoroDoc::new();
    src.set_peer_id(1).unwrap();
    let map = src.get_map("hot_map");
    let mut seed = 1u64;
    for i in 0..1000 {
        let mut val = String::with_capacity(96);
        for _ in 0..96 {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            val.push((b'a' + ((seed >> 32) % 26) as u8) as char);
        }
        map.insert(&format!("k_{i:04}"), val).unwrap();
    }
    src.commit();
    let snap = src.export(ExportMode::Snapshot).unwrap();

    let mut updates = Vec::with_capacity(40);
    for i in 0..40 {
        let vv = src.oplog_vv();
        map.insert(&format!("k_{:04}", i % 1000), format!("upd_{i}"))
            .unwrap();
        src.commit();
        updates.push(src.export(ExportMode::updates(&vv)).unwrap());
    }

    let probe = LoroDoc::new();
    probe.import(&snap).unwrap();
    probe.import(&updates[0]).unwrap();
    let hot_state = probe.memory_stats().state_bytes;
    probe.reclaim_memory();
    let reclaimed = probe.memory_stats().total_bytes;
    // Choose `max_doc_bytes` so materialized state + kv fits below `max_doc` (in the
    // soft (85%, 100%) band) while transient read/history caches before Stage 1 cross
    // `max_doc`. Stage 1 clears caches back under `max_doc`, so Stage 2 must stay soft
    // and keep the refault-protected hot map materialized across all 40 imports.
    let max_doc = ((reclaimed + hot_state) as f64 / 0.90) as usize;

    let doc = LoroDoc::new();
    doc.import(&snap).unwrap();
    doc.set_memory_budget(MemoryBudget::new().with_max_doc_bytes(max_doc));

    let mut evictions = 0usize;
    for u in &updates {
        doc.import(u).unwrap();
        if doc.memory_stats().state_bytes == 0 {
            evictions += 1;
        }
    }
    assert!(
        evictions <= 2,
        "hot map ping-pong evicted {evictions}/40 imports"
    );
    assert_eq!(doc.get_deep_value(), src.get_deep_value());
}

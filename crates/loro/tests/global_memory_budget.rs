use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex,
};

use loro::{
    global_memory_budget, global_memory_stats, reclaim_global_memory, set_global_memory_budget,
    try_reclaim_global_to_budget, ExportMode, GlobalMemoryBudget, LoroDoc, LoroError, LoroMap,
    MemoryBudget, MemoryLimitExceeded, MemoryLimitKind, ToJson,
};

// All tests in this file configure process-wide global memory settings, so serialize them.
static GLOBAL_TEST_LOCK: Mutex<()> = Mutex::new(());

struct ResetGlobalBudgetGuard;

impl Drop for ResetGlobalBudgetGuard {
    fn drop(&mut self) {
        set_global_memory_budget(GlobalMemoryBudget::default());
    }
}

fn lock_and_reset() -> (std::sync::MutexGuard<'static, ()>, ResetGlobalBudgetGuard) {
    let guard = GLOBAL_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    set_global_memory_budget(GlobalMemoryBudget::default());
    (guard, ResetGlobalBudgetGuard)
}

fn assert_global_limit_err(err: &LoroError) -> MemoryLimitExceeded {
    assert!(
        err.is_memory_limit_exceeded(),
        "expected MemoryLimitExceeded, got {err:?}"
    );
    let info = err
        .memory_limit_info()
        .expect("memory_limit_info should parse");
    assert_eq!(info.kind, MemoryLimitKind::GlobalBudget);
    assert!(
        info.used_bytes > info.limit_bytes,
        "expected used ({}) > limit ({})",
        info.used_bytes,
        info.limit_bytes
    );
    info
}

fn populate_doc_with_cold_kv(
    seed: u64,
    num_maps: usize,
    value_len: usize,
) -> (LoroDoc, serde_json::Value) {
    let src = LoroDoc::new();
    src.set_peer_id(seed + 1).unwrap();
    for i in 0..num_maps {
        let map = src.get_map(format!("k_{seed}_{i}"));
        let payload = format!("v_{seed}_{i}_{}", "x".repeat(value_len));
        map.insert("payload", payload).unwrap();
    }
    src.commit();
    let expected = src.get_deep_value().to_json_value();
    let snap = src.export(ExportMode::Snapshot).unwrap();
    let dst = LoroDoc::new();
    dst.set_peer_id(seed + 1000).unwrap();
    dst.import(&snap).unwrap();
    // Touch every container to materialize full `DocState` in RAM.
    for i in 0..num_maps {
        let map = dst.get_map(format!("k_{seed}_{i}"));
        map.insert("touched", i as i64).unwrap();
    }
    dst.commit();
    let expected_after = dst.get_deep_value().to_json_value();
    assert_ne!(expected_after, expected);
    (dst, expected_after)
}

#[test]
fn global_memory_stats_tracks_live_docs_drop_cleanup_and_shared_block_cache() {
    let (_lock, _reset) = lock_and_reset();

    let baseline = global_memory_stats();
    let budget = GlobalMemoryBudget::new()
        .with_max_heap_bytes(64 * 1024 * 1024)
        .with_max_kv_block_cache_bytes(2 * 1024 * 1024);
    set_global_memory_budget(budget.clone());
    assert_eq!(global_memory_budget(), budget);

    let (d1, _e1) = populate_doc_with_cold_kv(1, 24, 256);
    let (d2, _e2) = populate_doc_with_cold_kv(2, 24, 256);
    let stats_two = global_memory_stats();
    assert!(
        stats_two.live_doc_count >= baseline.live_doc_count + 2,
        "expected live_doc_count to include both docs: {stats_two:?}"
    );
    assert!(stats_two.docs_estimated_bytes > 0);

    // Trigger reads on d1 after flushing to KV so the shared global block cache populates,
    // while per-doc `DocMemoryStats::block_cache_bytes` stays 0 (unless overridden locally).
    let d1_snap = d1.export(ExportMode::Snapshot).unwrap();
    let d1_cold = LoroDoc::new();
    d1_cold.import(&d1_snap).unwrap();
    let _ = d1_cold.get_deep_value();
    assert_eq!(d1_cold.memory_stats().block_cache_bytes, 0);

    // Per-doc explicit `max_kv_block_cache_bytes` opts out into its own bounded cache.
    d2.set_memory_budget(MemoryBudget::new().with_max_kv_block_cache_bytes(64 * 1024));
    let _ = d2.reclaim_memory();
    let _ = d2.get_deep_value();

    drop(d1_cold);
    drop(d1);
    drop(d2);
    let after_drop = global_memory_stats();
    assert_eq!(after_drop.live_doc_count, baseline.live_doc_count);
}

#[test]
fn cross_doc_lru_reclamation_demotes_idle_docs_first_and_preserves_data() {
    let (_lock, _reset) = lock_and_reset();

    // Start with an unconstrained global budget with 0 global block cache so cache clearing
    // alone doesn't mask Stage 2 state demotion when testing LRU doc eviction.
    set_global_memory_budget(
        GlobalMemoryBudget::new()
            .with_max_heap_bytes(64 * 1024 * 1024)
            .with_watermarks(32 * 1024 * 1024, 16 * 1024 * 1024)
            .with_max_kv_block_cache_bytes(0),
    );

    let (idle_oldest, exp0) = populate_doc_with_cold_kv(10, 32, 300);
    let (idle_middle, exp1) = populate_doc_with_cold_kv(11, 32, 300);
    let (hot_doc, _exp2) = populate_doc_with_cold_kv(12, 32, 300);

    // Refresh `hot_doc` tick so `idle_oldest` and `idle_middle` are strictly older.
    hot_doc.get_map("k_12_0").insert("keep_hot", true).unwrap();
    hot_doc.commit();

    // Trim Stage 1 caches/KV memtables on a cloned probe or directly so our target sits cleanly
    // inside Stage 2 territory while keeping `high` just above current baseline.
    let idle0_state_before = idle_oldest.memory_stats().state_bytes;
    let idle1_state_before = idle_middle.memory_stats().state_bytes;
    let hot_state_before = hot_doc.memory_stats().state_bytes;
    let stats_before = global_memory_stats();
    assert!(idle0_state_before > 0 && idle1_state_before > 0 && hot_state_before > 0);
    assert_eq!(stats_before.global_block_cache_bytes, 0);

    let high = stats_before.docs_estimated_bytes + 512;
    // Below all-3-docs Stage-1 floor (~100 KB), high enough that demoting `idle_oldest` + `idle_middle`
    // brings usage below target without touching `hot_doc`'s materialized state.
    let target = stats_before
        .docs_estimated_bytes
        .saturating_sub((idle0_state_before + idle1_state_before + 30_000) as u64);
    set_global_memory_budget(
        GlobalMemoryBudget::new()
            .with_max_heap_bytes(stats_before.docs_estimated_bytes.saturating_mul(4))
            .with_watermarks(high, target)
            .with_max_kv_block_cache_bytes(0),
    );
    // Nothing crossed `high` yet (hysteresis).
    assert_eq!(idle_oldest.memory_stats().state_bytes, idle0_state_before);

    // Write enough to `hot_doc` to push total across `high` on commit.
    let hot_map = hot_doc.get_map("k_12_1");
    hot_map.insert("bump", "H".repeat(2048)).unwrap();
    hot_doc.commit();

    let idle0_state_after = idle_oldest.memory_stats().state_bytes;
    let hot_state_after = hot_doc.memory_stats().state_bytes;
    assert!(
        idle0_state_after < idle0_state_before,
        "expected oldest idle doc to be demoted: before={idle0_state_before} after={idle0_state_after}"
    );
    assert!(
        hot_state_after >= hot_state_before,
        "expected active hot doc to stay materialized while idle docs satisfied budget: before={hot_state_before} after={hot_state_after}"
    );

    // Explicit `reclaim_global_memory(None)` demotes all remaining idle cold state cleanly.
    let _freed = reclaim_global_memory(None);
    assert_eq!(idle_oldest.get_deep_value().to_json_value(), exp0);
    assert_eq!(idle_middle.get_deep_value().to_json_value(), exp1);

    idle_oldest
        .get_map("k_10_0")
        .insert("after_rehydrate", "ok")
        .unwrap();
    idle_oldest.commit();
    assert_eq!(
        idle_oldest
            .get_map("k_10_0")
            .get("after_rehydrate")
            .unwrap()
            .into_value()
            .unwrap()
            .as_string()
            .unwrap()
            .as_str(),
        "ok"
    );
}

#[test]
fn global_hard_budget_rejects_and_rolls_back_try_commit_import_and_batch_atomically() {
    let (_lock, _reset) = lock_and_reset();

    let idle = LoroDoc::new();
    idle.set_peer_id(1).unwrap();
    for i in 0..16 {
        idle.get_map(format!("idle_{i}"))
            .insert("v", format!("payload_{i}_{}", "a".repeat(256)))
            .unwrap();
    }
    idle.commit();

    let active = LoroDoc::new();
    active.set_peer_id(2).unwrap();
    active.get_map("m").insert("base", 1i64).unwrap();
    active.commit();

    let events = Arc::new(AtomicUsize::new(0));
    let events_cloned = events.clone();
    let _sub = active.subscribe_root(Arc::new(move |_| {
        events_cloned.fetch_add(1, Ordering::SeqCst);
    }));

    let _ = reclaim_global_memory(None);
    let baseline = global_memory_stats();
    let tight_max = baseline.docs_estimated_bytes + 1_024;
    set_global_memory_budget(
        GlobalMemoryBudget::new()
            .with_max_heap_bytes(tight_max)
            .with_watermarks(tight_max - 256, tight_max - 512),
    );
    let snapshot_before = active.get_deep_value().to_json_value();
    let vv_before = active.oplog_vv();

    // 1) try_commit rollback with 0 subscriber events
    active
        .get_text("big")
        .insert(0, &"Z".repeat(8 * 1024))
        .unwrap();
    let commit_err = active
        .try_commit()
        .expect_err("over-budget try_commit should reject");
    assert_global_limit_err(&commit_err);
    assert_eq!(events.load(Ordering::SeqCst), 0);
    assert_eq!(active.get_deep_value().to_json_value(), snapshot_before);
    assert_eq!(active.oplog_vv(), vv_before);

    // 2) remote import rollback with 0 subscriber events
    let sender = LoroDoc::new();
    sender.set_peer_id(99).unwrap();
    sender
        .get_text("remote")
        .insert(0, &"R".repeat(8 * 1024))
        .unwrap();
    sender.commit();
    let big_update = sender.export(ExportMode::all_updates()).unwrap();
    drop(sender);

    let import_err = active
        .import(&big_update)
        .expect_err("over-budget import should reject atomically");
    assert_global_limit_err(&import_err);
    assert_eq!(events.load(Ordering::SeqCst), 0);
    assert_eq!(active.get_deep_value().to_json_value(), snapshot_before);
    assert_eq!(active.oplog_vv(), vv_before);

    // 3) import_batch and import_batch_detailed rollback atomically on GlobalBudget
    let s1 = LoroDoc::new();
    s1.set_peer_id(101).unwrap();
    s1.get_text("t1").insert(0, &"A".repeat(4 * 1024)).unwrap();
    s1.commit();
    let s2 = LoroDoc::new();
    s2.set_peer_id(102).unwrap();
    s2.get_text("t2").insert(0, &"B".repeat(4 * 1024)).unwrap();
    s2.commit();
    let u1 = s1.export(ExportMode::all_updates()).unwrap();
    let u2 = s2.export(ExportMode::all_updates()).unwrap();
    drop(s1);
    drop(s2);

    let batch_err = active
        .import_batch(&[u1.clone(), u2.clone()])
        .expect_err("over-budget import_batch should roll back atomically");
    assert_global_limit_err(&batch_err);
    assert_eq!(events.load(Ordering::SeqCst), 0);
    assert_eq!(active.oplog_vv(), vv_before);

    let detail = active
        .import_batch_detailed(&[u1, u2])
        .expect("import_batch_detailed should report rollback cleanly");
    assert_global_limit_err(
        detail
            .rollback_error
            .as_ref()
            .expect("expected rollback_error on batch global budget overflow"),
    );
    assert!(detail.status.success.is_empty());
    assert_eq!(events.load(Ordering::SeqCst), 0);
    assert_eq!(active.oplog_vv(), vv_before);

    // Dropping the idle doc frees global headroom so a previously failing write succeeds!
    drop(idle);
    let reclaimed = try_reclaim_global_to_budget().unwrap();
    assert!(reclaimed.docs_estimated_bytes <= tight_max);

    let ok_map = active.get_map("after_free");
    ok_map.insert("resumed", true).unwrap();
    active.try_commit().unwrap();
    assert_eq!(events.load(Ordering::SeqCst), 1);

    // Explicit tight check fails `try_reclaim_global_to_budget` honestly.
    set_global_memory_budget(GlobalMemoryBudget::new().with_max_heap_bytes(64));
    let err = try_reclaim_global_to_budget().unwrap_err();
    assert_global_limit_err(&err);
}

#[test]
fn concurrent_multi_doc_writes_under_global_budget_never_deadlock() {
    let (_lock, _reset) = lock_and_reset();

    let docs: Vec<Arc<LoroDoc>> = (0..8)
        .map(|i| {
            let d = Arc::new(LoroDoc::new());
            d.set_peer_id(i as u64 + 1).unwrap();
            let _ = d.subscribe_pre_commit(Box::new(|_| {
                // Calling `global_memory_stats()` from inside a pre-commit callback (while
                // this doc's `Txn` lock is held) must remain completely deadlock-free.
                let _stats = global_memory_stats();
                true
            }));
            d
        })
        .collect();

    set_global_memory_budget(
        GlobalMemoryBudget::new()
            .with_max_heap_bytes(4 * 1024 * 1024)
            .with_watermarks(64 * 1024, 32 * 1024)
            .with_max_kv_block_cache_bytes(32 * 1024),
    );

    let handles: Vec<_> = (0..4)
        .map(|tid| {
            let docs = docs.clone();
            std::thread::spawn(move || {
                for step in 0..25 {
                    let doc = &docs[(tid + step) % docs.len()];
                    let map = doc.get_map(format!("t_{tid}_{step}"));
                    map.insert("payload", "x".repeat(256)).unwrap();
                    let _ = doc.try_commit();
                    if step % 5 == 0 {
                        let _ = global_memory_stats();
                        let _ = reclaim_global_memory(Some(32 * 1024));
                    }
                }
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }

    let stats = global_memory_stats();
    assert!(stats.live_doc_count >= 8);
}

#[test]
fn global_import_blob_limit_rejects_oversized_blob_cleanly() {
    let (_lock, _reset) = lock_and_reset();

    set_global_memory_budget(GlobalMemoryBudget::new().with_max_import_blob_bytes(256));
    let doc = LoroDoc::new();
    let oversized = vec![0u8; 512];
    let err = doc.import(&oversized).unwrap_err();
    let info = err.memory_limit_info().unwrap();
    assert_eq!(info.kind, MemoryLimitKind::ImportBlob);
    assert_eq!(info.used_bytes, 512);
    assert_eq!(info.limit_bytes, 256);

    // Ensure `LoroMap` import wasn't dead
    let _empty: Option<LoroMap> = None;
}

#[test]
fn global_budget_installation_preserves_an_already_edited_transaction() {
    let (_lock, _reset) = lock_and_reset();
    let doc = LoroDoc::new();
    let text = doc.get_text("text");
    text.insert(0, "before").unwrap();
    set_global_memory_budget(GlobalMemoryBudget::new().with_max_heap_bytes(1));
    doc.try_commit().unwrap();
    assert_eq!(text.to_string(), "before");
    text.insert(6, "after").unwrap();
    assert_global_limit_err(&doc.try_commit().unwrap_err());
    assert_eq!(text.to_string(), "before");
}

#[test]
fn reclaim_above_current_usage_preserves_the_shared_block_cache() {
    let (_lock, _reset) = lock_and_reset();
    set_global_memory_budget(GlobalMemoryBudget::new().with_max_kv_block_cache_bytes(1024 * 1024));
    let (source, _) = populate_doc_with_cold_kv(88, 32, 300);
    let snapshot = source.export(ExportMode::Snapshot).unwrap();
    let doc = LoroDoc::new();
    doc.import(&snapshot).unwrap();
    let _ = doc.get_deep_value();
    let before = global_memory_stats().global_block_cache_bytes;
    assert!(before > 0);
    assert_eq!(reclaim_global_memory(Some(u64::MAX)), 0);
    assert_eq!(global_memory_stats().global_block_cache_bytes, before);
}

#[test]
fn retained_map_reads_keep_the_document_recent_for_global_reclamation() {
    let (_lock, _reset) = lock_and_reset();
    for mode in ["field", "value", "deep"] {
        set_global_memory_budget(GlobalMemoryBudget::new().with_max_kv_block_cache_bytes(0));
        let (hot, _) = populate_doc_with_cold_kv(91, 32, 300);
        let retained = hot.get_map("k_91_0");
        let (cold, _) = populate_doc_with_cold_kv(92, 32, 300);
        match mode {
            "field" => {
                let _ = retained.get("payload");
            }
            "value" => {
                let _ = retained.get_value();
            }
            _ => {
                let _ = retained.get_deep_value();
            }
        }
        let hot_before = hot.memory_stats().state_bytes;
        let cold_before = cold.memory_stats().state_bytes;
        let mut demoted = false;
        for _ in 0..8 {
            let target = global_memory_stats().docs_estimated_bytes.saturating_sub(1);
            reclaim_global_memory(Some(target));
            let hot_after = hot.memory_stats().state_bytes;
            let cold_after = cold.memory_stats().state_bytes;
            if hot_after < hot_before || cold_after < cold_before {
                assert!(
                    cold_after < cold_before,
                    "mode={mode}: cold document was skipped"
                );
                assert_eq!(
                    hot_after, hot_before,
                    "mode={mode}: retained read was not tracked"
                );
                demoted = true;
                break;
            }
        }
        assert!(demoted, "mode={mode}: never reached state demotion");
    }
}

/// Dropping the last handle of a document with uncommitted edits while another
/// thread scans the global registry must still commit those edits. Before the
/// drop gate, a scan's temporary reference made the last handle skip its commit
/// and the edits disappeared (about half of the iterations in this loop).
#[test]
fn last_handle_drop_commits_while_another_thread_scans_the_registry() {
    let (_lock, _reset) = lock_and_reset();
    set_global_memory_budget(GlobalMemoryBudget {
        max_heap_bytes: Some(1 << 40),
        ..Default::default()
    });
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let scanner = {
        let stop = stop.clone();
        std::thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                let _ = global_memory_stats();
            }
        })
    };
    let mut lost = 0;
    for i in 0..2000 {
        let doc = LoroDoc::new();
        let updates = Arc::new(AtomicUsize::new(0));
        let counter = updates.clone();
        doc.subscribe_local_update(Box::new(move |_| {
            counter.fetch_add(1, Ordering::SeqCst);
            true
        }))
        .detach();
        doc.get_text("t").insert(0, &format!("x{i}")).unwrap();
        drop(doc);
        if updates.load(Ordering::SeqCst) != 1 {
            lost += 1;
        }
    }
    stop.store(true, Ordering::Relaxed);
    scanner.join().unwrap();
    assert_eq!(lost, 0);
}

/// Two handles of one document dropped at the same time on two threads: the
/// handle released second must still commit. Both handles used to observe the
/// other one in `strong_count` and skip the commit.
#[test]
fn concurrent_drops_of_the_last_two_handles_commit_once() {
    // Documents created here are visible to the other tests' global statistics.
    let (_lock, _reset) = lock_and_reset();
    let mut lost = 0;
    for i in 0..1000 {
        let doc = LoroDoc::new();
        let updates = Arc::new(AtomicUsize::new(0));
        let counter = updates.clone();
        doc.subscribe_local_update(Box::new(move |_| {
            counter.fetch_add(1, Ordering::SeqCst);
            true
        }))
        .detach();
        doc.get_text("t").insert(0, &format!("x{i}")).unwrap();
        let other = doc.clone();
        let barrier = Arc::new(std::sync::Barrier::new(2));
        let b2 = barrier.clone();
        let t = std::thread::spawn(move || {
            b2.wait();
            drop(other);
        });
        barrier.wait();
        drop(doc);
        t.join().unwrap();
        if updates.load(Ordering::SeqCst) != 1 {
            lost += 1;
        }
    }
    assert_eq!(lost, 0);
}

/// The native global estimate sums cached per-document statistics and refreshes
/// only documents changed since (`DocStatsDirty` in loro-internal). After every
/// kind of change it must still equal the sum of fresh per-document statistics.
#[test]
fn global_estimate_stays_exact_after_reads_edits_undo_drops_and_policy_switches() {
    let (_lock, _reset) = lock_and_reset();
    let baseline = global_memory_stats();
    assert_eq!(baseline.live_doc_count, 0, "other documents are alive");
    set_global_memory_budget(GlobalMemoryBudget::new().with_max_heap_bytes(1 << 40));

    let mut docs: Vec<LoroDoc> = Vec::new();
    for seed in 0..6u64 {
        // Imported from a snapshot: state stays encoded until read.
        let src = LoroDoc::new();
        src.set_peer_id(seed + 1).unwrap();
        for i in 0..32 {
            src.get_map(format!("m{i}"))
                .insert("v", "x".repeat(200))
                .unwrap();
        }
        src.commit();
        let doc = LoroDoc::new();
        doc.set_peer_id(seed + 100).unwrap();
        doc.import(&src.export(ExportMode::Snapshot).unwrap())
            .unwrap();
        docs.push(doc);
    }
    let check = |docs: &[LoroDoc], what: &str| {
        let global = global_memory_stats();
        let sum: u64 = docs
            .iter()
            .map(|d| d.memory_stats().total_bytes as u64)
            .sum();
        assert_eq!(global.live_doc_count, docs.len(), "{what}");
        assert_eq!(
            global.docs_estimated_bytes,
            sum + global.global_block_cache_bytes as u64,
            "{what}"
        );
    };
    check(&docs, "after import");

    let _ = docs[1].get_deep_value();
    check(&docs, "after a pure read");

    docs[2]
        .get_map("m0")
        .insert("big", "y".repeat(64 * 1024))
        .unwrap();
    check(&docs, "with an uncommitted edit");
    docs[2].commit();
    docs[3]
        .get_text("t")
        .insert(0, &"z".repeat(10_000))
        .unwrap();
    docs[3].try_commit().unwrap();
    check(&docs, "after commits");

    let _handler_only = docs[4].get_map("registered_by_a_handler_only");
    check(&docs, "after a handler registered a container");

    let undo = loro::UndoManager::new(&docs[5]);
    docs[5].get_text("u").insert(0, "undoable").unwrap();
    docs[5].commit();
    check(&docs, "after an undo step was recorded");
    drop(undo);
    check(&docs, "after the undo manager dropped");

    docs.remove(0);
    check(&docs, "after a drop");

    set_global_memory_budget(GlobalMemoryBudget::default());
    docs[0].get_text("t").insert(0, &"w".repeat(5_000)).unwrap();
    docs[0].commit();
    let _ = docs[1].reclaim_memory();
    set_global_memory_budget(GlobalMemoryBudget::new().with_max_heap_bytes(1 << 40));
    check(&docs, "after changes made while no global budget was set");

    // A busy document keeps its cached value and is refreshed once released.
    let doc = docs[2].clone();
    let handle = std::thread::spawn(move || {
        doc.get_text("busy").insert(0, &"b".repeat(20_000)).unwrap();
        doc.commit();
    });
    handle.join().unwrap();
    check(&docs, "after another thread's edit");
}

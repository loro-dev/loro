#![cfg(not(target_arch = "wasm32"))]

use loro::{ExportMode, LoroDoc, LoroText, MemoryBudget, ToJson};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

struct TempSpillDir(PathBuf);

impl TempSpillDir {
    fn new(prefix: &str) -> Self {
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "loro-ooc-{prefix}-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn sst_file_count(&self) -> usize {
        std::fs::read_dir(&self.0)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().is_some_and(|ext| ext == "sst"))
            .count()
    }
}

impl Drop for TempSpillDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn pseudo_random_text(seed: usize, len: usize) -> String {
    let mut state = (seed as u64).wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut out = String::with_capacity(len);
    for _ in 0..len {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
        let ch = b'a' + ((state >> 32) as u8 % 26);
        out.push(ch as char);
    }
    out
}

fn ram_component_sum(s: &loro::DocMemoryStats) -> usize {
    s.state_bytes
        + s.value_cache_bytes
        + s.kv_bytes
        + s.block_cache_bytes
        + s.change_cache_bytes
        + s.dag_bytes
        + s.arena_bytes
        + s.pending_bytes
        + s.history_cache_bytes
        + s.diff_calc_bytes
        + s.undo_bytes
        + s.uncommitted_txn_bytes
}

#[test]
fn out_of_core_spills_demotes_rehydrates_and_matches_reference_history() {
    let spill = TempSpillDir::new("rehydrate");
    let doc = LoroDoc::new();
    let reference = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    reference.set_peer_id(1).unwrap();

    doc.set_memory_budget(
        MemoryBudget::new()
            .with_spill_dir(spill.path())
            .with_watermarks(32 * 1024, 16 * 1024),
    );

    let mut checkpoints = Vec::new();
    for i in 0..48 {
        let payload = pseudo_random_text(i, 700);
        for target in [&doc, &reference] {
            let map = target.get_map(format!("m_{i}"));
            map.insert("body", payload.as_str()).unwrap();
            let t = map.insert_container("note", LoroText::new()).unwrap();
            t.insert(0, &pseudo_random_text(i + 1000, 350)).unwrap();
            target.get_list("items").push(i as i64).unwrap();
            target.commit();
        }
        if i == 15 || i == 31 {
            checkpoints.push(doc.oplog_frontiers());
        }
    }

    let freed = doc.reclaim_memory();
    assert!(freed > 0, "expected reclamation against out-of-core store");
    let stats = doc.memory_stats();
    assert_eq!(stats.total_bytes, ram_component_sum(&stats));
    assert!(
        stats.spilled_disk_bytes > 20_000,
        "expected significant spilled bytes on disk, got {}",
        stats.spilled_disk_bytes
    );
    assert!(
        spill.sst_file_count() >= 1,
        "expected .sst files in {:?}",
        spill.path()
    );
    assert!(
        stats.kv_bytes < stats.spilled_disk_bytes / 5,
        "expected metadata-only kv_bytes ({}) << spilled_disk_bytes ({})",
        stats.kv_bytes,
        stats.spilled_disk_bytes
    );

    // Reading cold containers, editing again, historical checkout, diff, and snapshot export all match reference.
    assert_eq!(doc.get_deep_value(), reference.get_deep_value());
    doc.get_map("m_0").insert("extra", 123).unwrap();
    reference.get_map("m_0").insert("extra", 123).unwrap();
    doc.commit();
    reference.commit();
    assert_eq!(doc.get_deep_value(), reference.get_deep_value());

    let diff_spilled = doc.diff(&checkpoints[0], &checkpoints[1]).unwrap();
    let diff_ref = reference.diff(&checkpoints[0], &checkpoints[1]).unwrap();
    assert_eq!(diff_spilled.iter().count(), diff_ref.iter().count());
    assert_eq!(format!("{diff_spilled:?}"), format!("{diff_ref:?}"));

    let f_at = &checkpoints[0];
    let forked_at = doc.fork_at(f_at).unwrap();
    let ref_forked_at = reference.fork_at(f_at).unwrap();
    assert_eq!(forked_at.get_deep_value(), ref_forked_at.get_deep_value());

    doc.checkout(f_at).unwrap();
    reference.checkout(f_at).unwrap();
    assert_eq!(doc.get_deep_value(), reference.get_deep_value());
    doc.try_checkout_to_latest().unwrap();
    reference.try_checkout_to_latest().unwrap();

    let snap = doc.export(ExportMode::Snapshot).unwrap();
    let restored = LoroDoc::from_snapshot(&snap).unwrap();
    assert_eq!(restored.get_deep_value(), reference.get_deep_value());
    let updates = doc.export(ExportMode::all_updates()).unwrap();
    let from_updates = LoroDoc::new();
    from_updates.import(&updates).unwrap();
    assert_eq!(from_updates.get_deep_value(), reference.get_deep_value());

    // Switching spill_dir back to None migrates SSTables into RAM and deletes spilled files.
    drop(forked_at);
    doc.set_memory_budget(MemoryBudget::default());
    let back_to_mem = doc.memory_stats();
    assert_eq!(back_to_mem.spilled_disk_bytes, 0);
    assert_eq!(spill.sst_file_count(), 0);
    assert_eq!(doc.get_deep_value(), reference.get_deep_value());
}

#[test]
fn out_of_core_fork_isolation_and_drop_cleanup() {
    let spill = TempSpillDir::new("fork-cleanup");
    let doc = LoroDoc::new();
    doc.set_peer_id(7).unwrap();
    doc.set_memory_budget(MemoryBudget::new().with_spill_dir(spill.path()));

    for i in 0..24 {
        doc.get_map(format!("k_{i}"))
            .insert("v", pseudo_random_text(i + 500, 600))
            .unwrap();
        doc.commit();
    }
    doc.reclaim_memory();
    assert!(doc.memory_stats().spilled_disk_bytes > 0);
    assert!(spill.sst_file_count() >= 1);

    let forked = doc.fork();
    assert!(forked.memory_stats().spilled_disk_bytes > 0);
    let expected = doc.get_deep_value().to_json_value();
    assert_eq!(forked.get_deep_value().to_json_value(), expected);

    // Dropping the original unlinks its temp files while the fork stays intact and writable.
    let files_before_drop = spill.sst_file_count();
    drop(doc);
    let files_after_orig_drop = spill.sst_file_count();
    assert!(files_after_orig_drop < files_before_drop);
    assert!(files_after_orig_drop >= 1);

    forked.get_map("k_0").insert("fork_only", true).unwrap();
    forked.commit();
    assert_eq!(
        forked.get_map("k_0").get_deep_value().to_json_value()["fork_only"],
        true
    );
    assert_eq!(
        forked.get_map("k_10").get_deep_value(),
        forked.get_map("k_10").get_deep_value()
    );

    drop(forked);
    assert_eq!(spill.sst_file_count(), 0);
}

fn text_doc_with_history() -> LoroDoc {
    let doc = LoroDoc::new();
    let text = doc.get_text("t");
    for i in 0..64 {
        text.insert(text.len_unicode(), &pseudo_random_text(i, 256))
            .unwrap();
        doc.commit();
    }
    doc
}

/// A spill directory that cannot be created leaves the document in memory and
/// is not recorded, so setting the same directory again once it is usable
/// switches the document out of core.
#[test]
fn a_failed_spill_directory_switch_is_reported_and_retried() {
    let base = TempSpillDir::new("switch-retry");
    let dir = base.path().join("dir");
    std::fs::write(&dir, b"a file, not a directory").unwrap();
    let doc = text_doc_with_history();
    let value = doc.get_deep_value();

    let budget = MemoryBudget::default().with_spill_dir(&dir);
    assert!(doc.try_set_memory_budget(budget.clone()).is_err());
    assert_eq!(doc.memory_budget().spill_dir, None);

    // The infallible setter applies the other settings and keeps the old storage.
    doc.set_memory_budget(MemoryBudget {
        max_doc_bytes: Some(1 << 30),
        ..budget.clone()
    });
    assert_eq!(doc.memory_budget().spill_dir, None);
    assert_eq!(doc.memory_budget().max_doc_bytes, Some(1 << 30));
    doc.reclaim_memory();
    assert_eq!(doc.memory_stats().spilled_disk_bytes, 0);
    assert_eq!(doc.get_deep_value(), value);

    std::fs::remove_file(&dir).unwrap();
    std::fs::create_dir_all(&dir).unwrap();
    doc.try_set_memory_budget(budget).unwrap();
    assert_eq!(
        doc.memory_budget().spill_dir.as_deref(),
        Some(dir.as_path())
    );
    doc.reclaim_memory();
    assert!(doc.memory_stats().spilled_disk_bytes > 0);
    assert_eq!(doc.get_deep_value(), value);
    doc.get_text("t").insert(0, "after").unwrap();
    doc.commit();
    let restored = LoroDoc::new();
    restored
        .import(&doc.export(ExportMode::Snapshot).unwrap())
        .unwrap();
    assert_eq!(restored.get_deep_value(), doc.get_deep_value());
}

/// A directory that exists but cannot hold files: the new stores cannot write
/// their tables, so the switch fails before either store is replaced.
#[cfg(target_os = "linux")]
#[test]
fn a_spill_directory_that_rejects_writes_keeps_the_current_storage() {
    let doc = text_doc_with_history();
    // Encoded history is what the switch has to write out.
    doc.compact_change_store();
    let value = doc.get_deep_value();
    let err = doc
        .try_set_memory_budget(MemoryBudget::default().with_spill_dir("/proc/self"))
        .unwrap_err();
    assert!(err.to_string().contains("spill"), "{err}");
    assert_eq!(doc.memory_budget().spill_dir, None);
    assert_eq!(doc.memory_stats().spilled_disk_bytes, 0);
    assert_eq!(doc.get_deep_value(), value);
    doc.get_text("t").insert(0, "still writable").unwrap();
    doc.commit();
    let restored = LoroDoc::new();
    restored
        .import(&doc.export(ExportMode::Snapshot).unwrap())
        .unwrap();
    assert_eq!(restored.get_deep_value(), doc.get_deep_value());
}

/// A flush that cannot write after the switch keeps its tables in memory and
/// used only to log; `take_spill_error` now reports it, once.
#[test]
fn a_flush_that_cannot_write_is_reported_by_take_spill_error() {
    let dir = TempSpillDir::new("flush-error");
    let doc = text_doc_with_history();
    doc.set_memory_budget(MemoryBudget::new().with_spill_dir(dir.path()));
    assert_eq!(doc.take_spill_error(), None);
    let expected_before = doc.get_deep_value();

    // Spill files keep open handles; replace the directory by a regular file so
    // the next table cannot be created.
    std::fs::remove_dir_all(dir.path()).unwrap();
    std::fs::write(dir.path(), b"not a directory").unwrap();
    doc.get_text("t")
        .insert(0, &pseudo_random_text(9, 4096))
        .unwrap();
    doc.commit();
    let _ = doc.reclaim_memory();

    let err = doc
        .take_spill_error()
        .expect("the failed flush was not reported");
    assert!(!err.is_empty());
    assert_eq!(doc.take_spill_error(), None, "reported twice");
    // Nothing was lost: the history read from the old files and the new tables
    // kept in memory.
    let value = doc.get_deep_value();
    assert_ne!(value, expected_before);
    let copy = LoroDoc::new();
    copy.import(&doc.export(ExportMode::Snapshot).unwrap())
        .unwrap();
    assert_eq!(copy.get_deep_value(), value);

    std::fs::remove_file(dir.path()).unwrap();
    std::fs::create_dir_all(dir.path()).unwrap();
}

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// Default soft cap (16 MiB) for the shared process-wide KV block cache when
/// global memory management is active.
pub const DEFAULT_GLOBAL_BLOCK_CACHE_BYTES: usize = 16 * 1024 * 1024;

static HEAP_TRACKER_ACTIVE: AtomicBool = AtomicBool::new(false);
static ALLOCATED_HEAP_BYTES: AtomicUsize = AtomicUsize::new(0);
static PEAK_ALLOCATED_HEAP_BYTES: AtomicUsize = AtomicUsize::new(0);

/// Marks global allocator byte tracking as active (used by `loro-wasm` `ReportingAlloc`).
#[inline]
pub fn enable_global_heap_tracker() {
    HEAP_TRACKER_ACTIVE.store(true, Ordering::Release);
}

/// Returns whether a global allocator tracker is actively reporting live heap bytes.
#[inline]
pub fn heap_tracker_active() -> bool {
    HEAP_TRACKER_ACTIVE.load(Ordering::Acquire)
}

/// Records an allocation of `bytes` in the global heap tracker.
#[inline(always)]
pub fn record_global_alloc(bytes: usize) {
    let prev = ALLOCATED_HEAP_BYTES.fetch_add(bytes, Ordering::Relaxed);
    let next = prev.saturating_add(bytes);
    if next > PEAK_ALLOCATED_HEAP_BYTES.load(Ordering::Relaxed) {
        PEAK_ALLOCATED_HEAP_BYTES.fetch_max(next, Ordering::Relaxed);
    }
}

/// Records a deallocation of `bytes` in the global heap tracker.
#[inline(always)]
pub fn record_global_dealloc(bytes: usize) {
    let _ = ALLOCATED_HEAP_BYTES.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |cur| {
        Some(cur.saturating_sub(bytes))
    });
}

/// Records a reallocation resize from `old_bytes` to `new_bytes` in the global heap tracker.
#[inline(always)]
pub fn record_global_realloc(old_bytes: usize, new_bytes: usize) {
    if new_bytes >= old_bytes {
        record_global_alloc(new_bytes - old_bytes);
    } else {
        record_global_dealloc(old_bytes - new_bytes);
    }
}

/// Returns currently tracked live heap bytes (`0` when no allocator hook is active).
#[inline]
pub fn tracked_allocated_heap_bytes() -> u64 {
    if heap_tracker_active() {
        ALLOCATED_HEAP_BYTES.load(Ordering::Relaxed) as u64
    } else {
        0
    }
}

/// Returns peak tracked live heap bytes (`0` when no allocator hook is active).
#[inline]
pub fn tracked_peak_allocated_heap_bytes() -> u64 {
    if heap_tracker_active() {
        PEAK_ALLOCATED_HEAP_BYTES.load(Ordering::Relaxed) as u64
    } else {
        0
    }
}

/// Returns the current WASM linear memory size in bytes (`memory.size(0) * 65536`),
/// or `0` on non-WASM targets.
#[inline]
pub fn wasm_linear_memory_bytes() -> u64 {
    #[cfg(target_arch = "wasm32")]
    {
        (core::arch::wasm32::memory_size(0) as u64).saturating_mul(65_536)
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        0
    }
}

/// Configuration for process-wide (or WASM-instance-wide) memory watermarks,
/// shared block cache bounds, and hard admission limits across all live [`crate::LoroDoc`]s.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GlobalMemoryBudget {
    /// Hard heap limit in bytes. Enforced against live allocator heap bytes when a global
    /// allocator tracker is active (such as `loro-crdt` WASM), or against estimated RAM
    /// across all live documents on native targets without a custom allocator tracker.
    pub max_heap_bytes: Option<u64>,
    /// Hard limit on committed WebAssembly linear memory (`memory.size(0) * 65536`) in bytes.
    /// Ignored on non-WASM targets.
    pub max_wasm_linear_bytes: Option<u64>,
    /// Soft high watermark (in bytes) that triggers cross-document LRU/clock reclamation.
    /// Defaults to ~85% of `max_heap_bytes` (or ~80% of `max_wasm_linear_bytes` on `wasm32`)
    /// when unset.
    pub high_watermark_bytes: Option<u64>,
    /// Soft target watermark (in bytes) aimed at by Stage 1/2 cross-document reclamation.
    /// Defaults to ~70% of `max_heap_bytes` (or ~65% of `max_wasm_linear_bytes` on `wasm32`)
    /// when unset.
    pub target_watermark_bytes: Option<u64>,
    /// Global hard per-blob byte cap enforced before decoding (and before copying JS buffers
    /// into WASM linear memory).
    pub max_import_blob_bytes: Option<usize>,
    /// Max byte capacity of the shared global SSTable decompressed block cache.
    pub max_kv_block_cache_bytes: Option<usize>,
}

impl GlobalMemoryBudget {
    /// Creates an unbounded global memory budget (disabled).
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the hard global heap ceiling (`max_heap_bytes`).
    pub fn with_max_heap_bytes(mut self, max_bytes: u64) -> Self {
        self.max_heap_bytes = Some(max_bytes);
        self
    }

    /// Sets the hard WASM linear memory ceiling (`max_wasm_linear_bytes`).
    pub fn with_max_wasm_linear_bytes(mut self, max_bytes: u64) -> Self {
        self.max_wasm_linear_bytes = Some(max_bytes);
        self
    }

    /// Sets explicit global soft high and target watermarks (in bytes).
    pub fn with_watermarks(
        mut self,
        high_watermark_bytes: u64,
        target_watermark_bytes: u64,
    ) -> Self {
        self.high_watermark_bytes = Some(high_watermark_bytes);
        self.target_watermark_bytes = Some(target_watermark_bytes);
        self
    }

    /// Sets the global per-blob limit (`max_import_blob_bytes`) before decoding/copying.
    pub fn with_max_import_blob_bytes(mut self, max_bytes: usize) -> Self {
        self.max_import_blob_bytes = Some(max_bytes);
        self
    }

    /// Sets the shared global KV block cache capacity (`max_kv_block_cache_bytes`).
    pub fn with_max_kv_block_cache_bytes(mut self, max_bytes: usize) -> Self {
        self.max_kv_block_cache_bytes = Some(max_bytes);
        self
    }

    /// Returns whether any global policy is active.
    pub fn is_enabled(&self) -> bool {
        self.max_heap_bytes.is_some()
            || (cfg!(target_arch = "wasm32") && self.max_wasm_linear_bytes.is_some())
            || self.high_watermark_bytes.is_some()
            || self.target_watermark_bytes.is_some()
            || self.max_import_blob_bytes.is_some()
            || self.max_kv_block_cache_bytes.is_some()
    }

    /// Linear pages stay allocated after free; pressure must also require live heap.
    pub(crate) fn wasm_heap_pressure(&self, linear_bytes: u64, live_heap_bytes: u64) -> bool {
        self.max_wasm_linear_bytes.is_some_and(|max| {
            linear_bytes > max.saturating_mul(80) / 100
                && live_heap_bytes > max.saturating_mul(70) / 100
        })
    }

    /// Effective high watermark in bytes that triggers cross-document reclamation.
    pub fn effective_high_watermark(&self) -> Option<u64> {
        let wasm_limit = if cfg!(target_arch = "wasm32") {
            self.max_wasm_linear_bytes
        } else {
            None
        };
        if let Some(h) = self.high_watermark_bytes {
            return Some(match self.max_heap_bytes {
                Some(max) => h.min(max),
                None => h,
            });
        }
        match (self.max_heap_bytes, wasm_limit) {
            (Some(max_h), Some(max_lin)) => Some(
                (max_h.saturating_sub(max_h.saturating_mul(15) / 100))
                    .min(max_lin.saturating_mul(80) / 100),
            ),
            (Some(max_h), None) => Some(max_h.saturating_sub(max_h.saturating_mul(15) / 100)),
            (None, Some(max_lin)) => Some(max_lin.saturating_mul(80) / 100),
            (None, None) => None,
        }
    }

    /// Effective target watermark in bytes aimed at by cross-document reclamation.
    pub fn effective_target_watermark(&self) -> Option<u64> {
        let high = self.effective_high_watermark();
        let wasm_limit = if cfg!(target_arch = "wasm32") {
            self.max_wasm_linear_bytes
        } else {
            None
        };
        let target = match (self.target_watermark_bytes, self.max_heap_bytes, wasm_limit) {
            (Some(t), _, _) => Some(t),
            (None, Some(max_h), Some(max_lin)) => {
                Some((max_h.saturating_mul(70) / 100).min(max_lin.saturating_mul(65) / 100))
            }
            (None, Some(max_h), None) => Some(max_h.saturating_mul(70) / 100),
            (None, None, Some(max_lin)) => Some(max_lin.saturating_mul(65) / 100),
            (None, None, None) => high,
        };
        match (target, high) {
            (Some(t), Some(h)) => Some(t.min(h)),
            (other, _) => other,
        }
    }

    /// Effective capacity for the process-wide shared SSTable block cache.
    pub fn effective_block_cache_bytes(&self) -> usize {
        if let Some(bytes) = self.max_kv_block_cache_bytes {
            return bytes;
        }
        if let Some(max_h) = self.max_heap_bytes {
            return usize::try_from(max_h / 8)
                .unwrap_or(usize::MAX)
                .min(DEFAULT_GLOBAL_BLOCK_CACHE_BYTES);
        }
        if cfg!(target_arch = "wasm32") {
            if let Some(max_lin) = self.max_wasm_linear_bytes {
                return usize::try_from(max_lin / 8)
                    .unwrap_or(usize::MAX)
                    .min(DEFAULT_GLOBAL_BLOCK_CACHE_BYTES);
            }
        }
        DEFAULT_GLOBAL_BLOCK_CACHE_BYTES
    }
}

/// Process-wide / WASM-instance-wide memory statistics across all live [`crate::LoroDoc`]s.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GlobalMemoryStats {
    /// Number of currently alive [`crate::LoroDoc`] instances registered in the pool.
    pub live_doc_count: usize,
    /// Sum of estimated CRDT RAM across all live documents plus `global_block_cache_bytes`.
    pub docs_estimated_bytes: u64,
    /// Decompressed SSTable bytes currently held in the global `SharedBlockCache`.
    pub global_block_cache_bytes: usize,
    /// Live Rust heap bytes reported by the active global allocator (`0` if untracked).
    pub allocated_heap_bytes: u64,
    /// High-water mark of `allocated_heap_bytes` (`0` if untracked).
    pub peak_allocated_heap_bytes: u64,
    /// Committed WebAssembly linear memory size (`memory.size(0) * 65536`), or `0` on native.
    pub wasm_linear_memory_bytes: u64,
    /// Total bytes spilled to on-disk backing storage across all live documents.
    pub spilled_disk_bytes: u64,
}

/// Configuration for soft memory watermarks and hard memory quotas on a [`crate::LoroDoc`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MemoryBudget {
    /// Hard total estimated RAM ceiling for [`crate::LoroDoc`] in bytes.
    pub max_doc_bytes: Option<usize>,
    /// Soft high watermark (in bytes) that triggers Stage 1/2 automatic reclamation.
    /// Defaults to ~85% of `max_doc_bytes` when `max_doc_bytes` is set.
    pub high_watermark_bytes: Option<usize>,
    /// Target watermark (in bytes) that Stage 2 cold-state demotion tries to reach.
    /// Defaults to ~70% of `max_doc_bytes` when `max_doc_bytes` is set.
    pub target_watermark_bytes: Option<usize>,
    /// Hard per-blob byte limit enforced before decoding any imported snapshot or update.
    pub max_import_blob_bytes: Option<usize>,
    /// Hard byte quota for parked out-of-order pending changes in `OpLog`.
    pub max_pending_bytes: Option<usize>,
    /// Soft cap (in bytes) on the shared decompressed KV SSTable block cache.
    pub max_kv_block_cache_bytes: Option<usize>,
    /// Soft cap (in bytes) on decoded value-only wrappers kept in `DocState`.
    pub max_decoded_value_cache_bytes: Option<usize>,
    /// Maximum retained bytes in registered [`crate::UndoManager`] stacks.
    pub max_undo_bytes: Option<usize>,
    /// Optional native directory for spilling compacted SSTables onto disk
    /// (ignored on `wasm32` targets).
    pub spill_dir: Option<std::path::PathBuf>,
}

impl MemoryBudget {
    /// Creates an unbounded memory budget.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the hard document RAM ceiling (`max_doc_bytes`).
    pub fn with_max_doc_bytes(mut self, max_bytes: usize) -> Self {
        self.max_doc_bytes = Some(max_bytes);
        self
    }

    /// Sets explicit soft high and target watermarks (in bytes).
    pub fn with_watermarks(
        mut self,
        high_watermark_bytes: usize,
        target_watermark_bytes: usize,
    ) -> Self {
        self.high_watermark_bytes = Some(high_watermark_bytes);
        self.target_watermark_bytes = Some(target_watermark_bytes);
        self
    }

    /// Sets the hard per-blob size limit (`max_import_blob_bytes`) before decoding.
    pub fn with_max_import_blob_bytes(mut self, max_bytes: usize) -> Self {
        self.max_import_blob_bytes = Some(max_bytes);
        self
    }

    /// Sets the hard byte quota for parked out-of-order pending changes (`max_pending_bytes`).
    pub fn with_max_pending_bytes(mut self, max_bytes: usize) -> Self {
        self.max_pending_bytes = Some(max_bytes);
        self
    }

    /// Sets the byte cap on the shared KV SSTable block cache.
    pub fn with_max_kv_block_cache_bytes(mut self, max_bytes: usize) -> Self {
        self.max_kv_block_cache_bytes = Some(max_bytes);
        self
    }

    /// Sets the byte cap on `DocState`'s decoded value cache.
    pub fn with_max_decoded_value_cache_bytes(mut self, max_bytes: usize) -> Self {
        self.max_decoded_value_cache_bytes = Some(max_bytes);
        self
    }

    /// Sets the byte quota for attached [`crate::UndoManager`] instances.
    pub fn with_max_undo_bytes(mut self, max_bytes: usize) -> Self {
        self.max_undo_bytes = Some(max_bytes);
        self
    }

    /// Configures a native directory (`spill_dir`) for out-of-core SSTable spilling.
    ///
    /// Has no effect on `wasm32` targets.
    pub fn with_spill_dir(mut self, dir: impl Into<std::path::PathBuf>) -> Self {
        self.spill_dir = Some(dir.into());
        self
    }

    /// Effective Stage 1 trigger threshold in bytes (85% of `max_doc_bytes` by default).
    pub fn effective_high_watermark(&self) -> Option<usize> {
        let high = match (self.high_watermark_bytes, self.max_doc_bytes) {
            (Some(h), Some(max)) => Some(h.min(max)),
            (Some(h), None) => Some(h),
            (None, Some(max)) => Some(max.saturating_sub((max / 100) * 15)),
            (None, None) => None,
        };
        if let (Some(h), Some(max)) = (high, self.max_doc_bytes) {
            // Avoid rounding edge cases where (max / 100) * 15 == 0 for tiny limits
            if self.high_watermark_bytes.is_none() && max > 0 {
                return Some(h.min(max - (max.saturating_mul(15) / 100)));
            }
        }
        high
    }

    /// Effective Stage 2 target threshold in bytes (70% of `max_doc_bytes` by default).
    pub fn effective_target_watermark(&self) -> Option<usize> {
        let high = self.effective_high_watermark();
        let target = match (self.target_watermark_bytes, self.max_doc_bytes) {
            (Some(t), _) => Some(t),
            (None, Some(max)) => Some(max.saturating_mul(70) / 100),
            (None, None) => high,
        };
        match (target, high) {
            (Some(t), Some(h)) => Some(t.min(h)),
            (other, _) => other,
        }
    }
}

/// Estimated in-memory and spilled byte usage across all subsystems of a [`crate::LoroDoc`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DocMemoryStats {
    /// Total estimated RAM used by this document (sum of all RAM fields below,
    /// excluding `spilled_disk_bytes`).
    pub total_bytes: usize,
    /// Materialized container states in `DocState`.
    pub state_bytes: usize,
    /// Decoded value-only container wrappers cached in `DocState`.
    pub value_cache_bytes: usize,
    /// Compressed SSTables and in-memory memtables across state & oplog KV stores.
    pub kv_bytes: usize,
    /// Decompressed KV SSTable blocks held in the document's `SharedBlockCache`.
    pub block_cache_bytes: usize,
    /// Parsed/in-memory changes retained in `OpLog`'s `ChangeStore`.
    pub change_cache_bytes: usize,
    /// App DAG nodes and cached version vectors in `OpLog`.
    pub dag_bytes: usize,
    /// Shared container/string/value arena in `OpLog` & `DocState`.
    pub arena_bytes: usize,
    /// Parked out-of-order pending changes waiting for missing dependencies.
    pub pending_bytes: usize,
    /// Cached checkout / shallow-root history indices in `OpLog`.
    pub history_cache_bytes: usize,
    /// Cached persistent CRDT diff trackers in `DiffCalculator`.
    pub diff_calc_bytes: usize,
    /// Retained undo/redo stacks in registered `UndoManager` instances.
    pub undo_bytes: usize,
    /// Operations and event hints buffered in the currently open `Transaction`.
    pub uncommitted_txn_bytes: usize,
    /// Bytes spilled to on-disk backing storage (0 unless out-of-core spilling is active).
    pub spilled_disk_bytes: usize,
}

#[cfg(test)]
mod pressure_tests {
    use super::*;
    #[test]
    fn freed_heap_clears_pressure_even_when_wasm_pages_do_not_shrink() {
        let budget = GlobalMemoryBudget::new().with_max_wasm_linear_bytes(100_000);
        let linear = 90_000;
        assert!(budget.wasm_heap_pressure(linear, 75_000));
        assert!(!budget.wasm_heap_pressure(linear, 60_000));
        assert!(!budget.wasm_heap_pressure(linear, 69_000));
        assert!(budget.wasm_heap_pressure(linear, 71_000));
        assert!(!budget.wasm_heap_pressure(70_000, 71_000));
    }
}

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

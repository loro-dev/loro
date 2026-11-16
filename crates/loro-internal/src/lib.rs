//! loro-internal is a CRDT framework.
//!
//!
//!
//!
#![deny(clippy::undocumented_unsafe_blocks)]
#![allow(unused_assignments)]
#![allow(clippy::uninlined_format_args)]
#![warn(rustdoc::broken_intra_doc_links)]
#![warn(missing_debug_implementations)]

pub mod arena;
pub mod diff;
pub mod diff_calc;
pub mod handler;
pub mod sync;
pub use state::container_tree;

use crate::sync::{AtomicBool, AtomicUsize};
use std::sync::Arc;
mod change_meta;
pub(crate) mod lock;
use arena::SharedArena;
use configure::Configure;
use diff_calc::DiffCalculator;
use lock::LoroMutex;

pub use change_meta::ChangeMeta;
pub use event::{ContainerDiff, DiffEvent, DocDiff, ListDiff, ListDiffInsertItem, ListDiffItem};
pub use handler::{
    BasicHandler, HandlerTrait, ListHandler, MapHandler, MovableListHandler, TextHandler,
    TreeHandler, UnknownHandler,
};
pub use loro_common;
pub use oplog::OpLog;
use pre_commit::{
    FirstCommitFromPeerCallback, FirstCommitFromPeerPayload, PreCommitCallback,
    PreCommitCallbackPayload,
};
pub use rustc_hash::FxHashMap;
pub use state::DocState;
pub use state::{TreeNode, TreeNodeWithChildren, TreeParentId};
use subscription::{LocalUpdateCallback, Observer, PeerIdUpdateCallback};
use txn::Transaction;
pub use undo::UndoManager;
pub use utils::subscription::SubscriberSetWithQueue;
pub use utils::subscription::Subscription;
pub mod allocation;
pub mod awareness;
pub mod change;
pub mod configure;
pub mod container;
pub mod cursor;
pub mod dag;
pub mod encoding;
pub(crate) mod fork;
pub mod id;
#[cfg(feature = "jsonpath")]
pub mod jsonpath;
pub mod kv_store;
pub mod loro;
pub mod memory;
pub mod op;
pub mod oplog;
pub mod subscription;
pub mod txn;
pub mod version;

mod error;
#[cfg(feature = "test_utils")]
pub mod fuzz;
mod parent;
pub mod pre_commit;
mod span;
#[cfg(test)]
pub mod tests;
mod utils;
pub use utils::string_slice::StringSlice;

pub mod delta;
pub use loro_delta;
pub mod event;

pub mod estimated_size;
pub(crate) mod history_cache;
pub(crate) mod macros;
pub(crate) mod state;
pub mod undo;
pub(crate) mod value;

// TODO: rename as Key?
pub(crate) use loro_common::InternalString;

pub use container::ContainerType;
pub use encoding::json_schema::json;
pub use fractional_index::FractionalIndex;
pub use loro::{
    global_memory_budget, global_memory_stats, reclaim_global_memory, set_global_memory_budget,
    try_reclaim_global_to_budget, wasm_linear_memory_exceeded,
};
pub use loro_common::{loro_value, to_value};
pub use loro_common::{
    Counter, CounterSpan, IdLp, IdSpan, IdSpanVector, Lamport, LoroEncodeError, LoroError,
    LoroResult, LoroTreeError, MemoryLimitExceeded, MemoryLimitKind, PeerID, TreeID, ID,
};
pub use loro_common::{LoroBinaryValue, LoroListValue, LoroMapValue, LoroStringValue};
pub use memory::{DocMemoryStats, GlobalMemoryBudget, GlobalMemoryStats, MemoryBudget};
#[cfg(feature = "wasm")]
pub use value::wasm;
pub use value::{ApplyDiff, LoroValue, ToJson};
pub use version::VersionVector;

/// [`LoroDoc`] serves as the library's primary entry point.
/// It's constituted by an [OpLog] and an [DocState].
///
/// - [OpLog] encompasses all operations, signifying the document history.
/// - [DocState] signifies the current document state.
///
/// They will share a [super::arena::SharedArena]
///
/// # Detached Mode
///
/// This mode enables separate usage of [OpLog] and [DocState].
/// It facilitates temporal navigation. [DocState] can be reverted to
/// any version contained within the [OpLog].
///
/// `LoroDoc::detach()` separates [DocState] from [OpLog]. In this mode,
/// updates to [OpLog] won't affect [DocState], while updates to [DocState]
/// will continue to affect [OpLog].
#[derive(Debug, Clone)]
#[repr(transparent)]
pub struct LoroDoc {
    // Released explicitly in `Drop for LoroDoc`, inside the drop gate when this
    // handle is not the last one (see `DocDropGate`).
    inner: std::mem::ManuallyDrop<Arc<LoroDocInner>>,
}

impl LoroDoc {
    pub(crate) fn from_inner(inner: Arc<LoroDocInner>) -> Self {
        Self {
            inner: std::mem::ManuallyDrop::new(inner),
        }
    }
}

/// Serializes the "is this the last handle?" decision in `Drop for LoroDoc`
/// with the global registry's temporary references.
///
/// The registry upgrades its `Weak` and increments `registry_refs` while holding
/// this lock, and drops its `Arc` and decrements while holding it. A handle that
/// is not the last one also releases its `Arc` while holding it. So inside the
/// lock, `strong_count - registry_refs` counts exactly the other handles (plus
/// short-lived upgrades made while one of those handles is in use), and the
/// last handle always commits, however the registry interleaves. Kept outside
/// `LoroDocInner` so that the final drop may happen while the lock is held.
/// See `context/memory-budget.md` ("Last handle and registry references").
#[derive(Debug, Default)]
pub(crate) struct DocDropGate {
    pub(crate) registry_refs: crate::sync::Mutex<usize>,
}

impl std::ops::Deref for LoroDoc {
    type Target = LoroDocInner;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

pub struct LoroDocInner {
    oplog: Arc<LoroMutex<OpLog>>,
    state: Arc<LoroMutex<DocState>>,
    arena: SharedArena,
    config: Configure,
    visible_op_count: Arc<AtomicUsize>,
    observer: Arc<Observer>,
    diff_calculator: Arc<LoroMutex<DiffCalculator>>,
    /// When dropping the doc, the txn will be committed
    ///
    /// # Internal Notes
    ///
    /// Txn can be accessed by different threads. But for certain methods we need to lock the txn and ensure it's empty:
    ///
    /// - `import`
    /// - `export`
    /// - `checkout`
    /// - `checkout_to_latest`
    /// - ...
    ///
    /// We need to lock txn and keep it None because otherwise the DocState may change due to a parallel edit on a new Txn,
    /// which may break the invariants of `import`, `export` and `checkout`.
    txn: Arc<LoroMutex<Option<Transaction>>>,
    auto_commit: AtomicBool,
    detached: AtomicBool,
    local_update_subs: SubscriberSetWithQueue<(), LocalUpdateCallback, Vec<u8>>,
    peer_id_change_subs: SubscriberSetWithQueue<(), PeerIdUpdateCallback, ID>,
    first_commit_from_peer_subs:
        SubscriberSetWithQueue<(), FirstCommitFromPeerCallback, FirstCommitFromPeerPayload>,
    pre_commit_subs: SubscriberSetWithQueue<(), PreCommitCallback, PreCommitCallbackPayload>,
    pub(crate) undo_managers: crate::sync::Mutex<
        Vec<
            std::sync::Weak<
                parking_lot::ReentrantMutex<std::cell::RefCell<undo::UndoManagerInner>>,
            >,
        >,
    >,
    pub(crate) memory_budget: crate::sync::Mutex<memory::MemoryBudget>,
    pub(crate) budget_flags: std::sync::atomic::AtomicU8,
    pub(crate) kv_block_cache: kv_store::SharedBlockCache,
    pub(crate) pool_doc_id: u64,
    pub(crate) cached_doc_bytes: std::sync::atomic::AtomicUsize,
    /// `spilled_disk_bytes` of the statistics `cached_doc_bytes` came from.
    pub(crate) cached_spilled_bytes: std::sync::atomic::AtomicUsize,
    /// Whether `cached_doc_bytes` may be stale; see [`loro::DocStatsDirty`].
    pub(crate) stats_dirty: Arc<loro::DocStatsDirty>,
    /// Skips per-document soft sweeps that recently failed; see [`loro::SoftBackoff`].
    pub(crate) soft_backoff: loro::SoftBackoff,
    pub(crate) last_access_tick: std::sync::atomic::AtomicU64,
    pub(crate) drop_gate: Arc<DocDropGate>,
}

impl Drop for LoroDocInner {
    fn drop(&mut self) {
        loro::unregister_doc_from_global_pool(
            self.pool_doc_id,
            self.cached_doc_bytes
                .load(std::sync::atomic::Ordering::Relaxed),
            self.cached_spilled_bytes
                .load(std::sync::atomic::Ordering::Relaxed),
        );
    }
}

/// The version of the loro crate
pub const LORO_VERSION: &str = include_str!("../VERSION");

impl Drop for LoroDoc {
    fn drop(&mut self) {
        struct ReleaseOnExit(*mut std::mem::ManuallyDrop<Arc<LoroDocInner>>);
        impl Drop for ReleaseOnExit {
            fn drop(&mut self) {
                // SAFETY: points at `LoroDoc::inner`, which is not used after this
                // guard runs, also when the commit below unwinds.
                unsafe { std::mem::ManuallyDrop::drop(&mut *self.0) }
            }
        }

        let gate = Arc::clone(&self.inner.drop_gate);
        let refs = gate.registry_refs.lock_ignoring_poison();
        let others = Arc::strong_count(&self.inner)
            .saturating_sub(1)
            .saturating_sub(*refs);
        if others > 0 {
            // Release inside the gate: a concurrent `drop` of the other handle must
            // not count this one as still alive.
            // SAFETY: `inner` is not used after this.
            unsafe { std::mem::ManuallyDrop::drop(&mut self.inner) };
            drop(refs);
            return;
        }
        drop(refs);
        let _release = ReleaseOnExit(&mut self.inner);
        let _ = self.implicit_commit_then_stop();
    }
}

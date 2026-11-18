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
#[derive(Debug)]
#[repr(transparent)]
pub struct LoroDoc {
    // Released explicitly in `Drop for LoroDoc` after updating `user_handles`.
    inner: std::mem::ManuallyDrop<Arc<LoroDocInner>>,
}

impl Clone for LoroDoc {
    #[inline]
    fn clone(&self) -> Self {
        self.inner
            .user_handles
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl LoroDoc {
    #[inline]
    pub(crate) fn from_inner(inner: Arc<LoroDocInner>) -> Self {
        inner
            .user_handles
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Self {
            inner: std::mem::ManuallyDrop::new(inner),
        }
    }

    #[inline]
    pub(crate) fn from_new_inner(inner: Arc<LoroDocInner>) -> Self {
        debug_assert_eq!(
            inner
                .user_handles
                .load(std::sync::atomic::Ordering::Relaxed),
            1
        );
        Self {
            inner: std::mem::ManuallyDrop::new(inner),
        }
    }

    /// Borrows a `&LoroDoc` view over an existing `&Arc<LoroDocInner>` without
    /// creating or dropping a counted user handle.
    #[inline]
    pub(crate) fn borrow_from_inner(inner: &Arc<LoroDocInner>) -> &Self {
        // SAFETY: `LoroDoc` and `ManuallyDrop<Arc<LoroDocInner>>` are both `#[repr(transparent)]`.
        unsafe { &*(inner as *const Arc<LoroDocInner> as *const Self) }
    }
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
    pub(crate) active_undo_managers: std::sync::atomic::AtomicUsize,
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
    /// Number of live user-facing `LoroDoc` handles (including attached container
    /// handlers). Temporary internal `Arc<LoroDocInner>` upgrades (such as
    /// `RegistryRef` or `Transaction::commit`) do not touch this counter, so the
    /// last user handle commits lock-free without racing registry scans.
    pub(crate) user_handles: crate::sync::AtomicUsize,
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
    #[inline]
    fn drop(&mut self) {
        use std::sync::atomic::Ordering::{AcqRel, Relaxed};

        let mut cur = self.inner.user_handles.load(Relaxed);
        loop {
            if cur > 1 {
                match self
                    .inner
                    .user_handles
                    .compare_exchange(cur, cur - 1, AcqRel, Relaxed)
                {
                    Ok(_) => {
                        // SAFETY: `inner` is not used after this.
                        unsafe { std::mem::ManuallyDrop::drop(&mut self.inner) };
                        return;
                    }
                    Err(actual) => {
                        cur = actual;
                        continue;
                    }
                }
            }

            struct ReleaseOnUnwind(*mut std::mem::ManuallyDrop<Arc<LoroDocInner>>);
            impl Drop for ReleaseOnUnwind {
                fn drop(&mut self) {
                    // SAFETY: points at `LoroDoc::inner`, which is not used after
                    // unwinding exits `Drop::drop`.
                    unsafe {
                        let inner = &mut *self.0;
                        inner.user_handles.fetch_sub(1, AcqRel);
                        std::mem::ManuallyDrop::drop(inner);
                    }
                }
            }

            let guard = ReleaseOnUnwind(&mut self.inner);
            let _ = self.implicit_commit_then_stop();
            std::mem::forget(guard);

            match self
                .inner
                .user_handles
                .compare_exchange(1, 0, AcqRel, Relaxed)
            {
                Ok(_) => {
                    // SAFETY: `inner` is not used after this.
                    unsafe { std::mem::ManuallyDrop::drop(&mut self.inner) };
                    return;
                }
                Err(actual) => {
                    cur = actual;
                }
            }
        }
    }
}

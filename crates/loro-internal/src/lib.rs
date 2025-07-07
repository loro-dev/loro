//! loro-internal is a CRDT framework.
//!
//!
//!
//!
#![deny(clippy::undocumented_unsafe_blocks)]
#![warn(rustdoc::broken_intra_doc_links)]
#![warn(missing_debug_implementations)]

pub mod arena;
pub mod diff;
pub mod diff_calc;
pub mod handler;
pub mod sync;
use crate::sync::AtomicBool;
use std::sync::Arc;
mod change_meta;
pub(crate) mod lock;
use arena::SharedArena;
use configure::Configure;
use diff_calc::DiffCalculator;
use lock::LoroMutex;

pub use change_meta::ChangeMeta;
pub use event::{ContainerDiff, DiffEvent, DocDiff, ListDiff, ListDiffInsertItem, ListDiffItem};
pub use fxhash::FxHashMap;
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
pub use state::DocState;
pub use state::{TreeNode, TreeNodeWithChildren, TreeParentId};
use subscription::{LocalUpdateCallback, Observer, PeerIdUpdateCallback, UndoCallback};
use txn::Transaction;
pub use undo::{DiffBatch, UndoManager};
use utils::subscription::SubscriberSetWithQueue;
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
pub(crate) mod undo_transform;
pub(crate) mod undo_transform_enhanced;
pub(crate) mod value;

// TODO: rename as Key?
pub(crate) use loro_common::InternalString;

pub use container::ContainerType;
pub use encoding::json_schema::json;
pub use fractional_index::FractionalIndex;
pub use loro_common::{loro_value, to_value};
pub use loro_common::{
    Counter, CounterSpan, IdLp, IdSpan, IdSpanVector, Lamport, LoroEncodeError, LoroError,
    LoroResult, LoroTreeError, PeerID, TreeID, ID,
};
pub use loro_common::{LoroBinaryValue, LoroListValue, LoroMapValue, LoroStringValue};
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
    inner: Arc<LoroDocInner>,
}

impl LoroDoc {
    pub(crate) fn from_inner(inner: Arc<LoroDocInner>) -> Self {
        Self { inner }
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
    undo_subs: SubscriberSetWithQueue<(), UndoCallback, DiffBatch>,
}

/// The version of the loro crate
pub const LORO_VERSION: &str = include_str!("../VERSION");

impl Drop for LoroDoc {
    fn drop(&mut self) {
        if Arc::strong_count(&self.inner) == 1 {
            let _ = self.commit_then_stop();
        }
    }
}

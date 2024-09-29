//! loro-internal is a CRDT framework.
//!
//!
//!
//!
#![deny(clippy::undocumented_unsafe_blocks)]
#![warn(rustdoc::broken_intra_doc_links)]
#![warn(missing_debug_implementations)]

pub mod arena;
mod change_meta;
pub mod diff;
pub mod diff_calc;
pub mod handler;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use arena::SharedArena;
use configure::Configure;
use diff_calc::DiffCalculator;

pub use change_meta::ChangeMeta;
pub use event::{ContainerDiff, DiffEvent, DocDiff, ListDiff, ListDiffInsertItem, ListDiffItem};
pub use fxhash::FxHashMap;
pub use handler::{
    BasicHandler, HandlerTrait, ListHandler, MapHandler, MovableListHandler, TextHandler,
    TreeHandler, UnknownHandler,
};
pub use loro_common;
pub use oplog::OpLog;
pub use state::DocState;
pub use state::{TreeNode, TreeNodeWithChildren, TreeParentId};
use subscription::{LocalUpdateCallback, Observer, PeerIdUpdateCallback};
use txn::Transaction;
pub use undo::UndoManager;
use utils::subscription::SubscriberSet;
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
pub use loro_common::{loro_value, to_value};
pub use loro_common::{
    Counter, CounterSpan, IdLp, IdSpan, Lamport, LoroError, LoroResult, LoroTreeError, PeerID,
    TreeID, ID,
};
#[cfg(feature = "wasm")]
pub use value::wasm;
pub use value::{ApplyDiff, LoroValue, ToJson};
pub use version::VersionVector;

/// `LoroApp` serves as the library's primary entry point.
/// It's constituted by an [OpLog] and an [AppState].
///
/// - [OpLog] encompasses all operations, signifying the document history.
/// - [AppState] signifies the current document state.
///
/// They will share a [super::arena::SharedArena]
///
/// # Detached Mode
///
/// This mode enables separate usage of [OpLog] and [AppState].
/// It facilitates temporal navigation. [AppState] can be reverted to
/// any version contained within the [OpLog].
///
/// `LoroApp::detach()` separates [AppState] from [OpLog]. In this mode,
/// updates to [OpLog] won't affect [AppState], while updates to [AppState]
/// will continue to affect [OpLog].
pub struct LoroDoc {
    oplog: Arc<Mutex<OpLog>>,
    state: Arc<Mutex<DocState>>,
    arena: SharedArena,
    config: Configure,
    observer: Arc<Observer>,
    diff_calculator: Arc<Mutex<DiffCalculator>>,
    // when dropping the doc, the txn will be committed
    txn: Arc<Mutex<Option<Transaction>>>,
    auto_commit: AtomicBool,
    detached: AtomicBool,

    local_update_subs: SubscriberSet<(), LocalUpdateCallback>,
    peer_id_change_subs: SubscriberSet<(), PeerIdUpdateCallback>,
}

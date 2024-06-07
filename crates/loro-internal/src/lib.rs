//! loro-internal is a CRDT framework.
//!
//!
//!
//!
#![deny(clippy::undocumented_unsafe_blocks)]
#![warn(rustdoc::broken_intra_doc_links)]
#![warn(missing_debug_implementations)]

pub mod arena;
pub mod diff_calc;
pub mod handler;
pub use event::{ContainerDiff, DiffEvent, DocDiff, ListDiff, ListDiffInsertItem, ListDiffItem};
pub use fxhash::FxHashMap;
pub use handler::{
    BasicHandler, HandlerTrait, ListHandler, MapHandler, MovableListHandler, TextHandler,
    TreeHandler, UnknownHandler,
};
pub use loro::LoroDoc;
pub use loro_common;
pub use oplog::OpLog;
pub use state::DocState;
pub use undo::UndoManager;
pub mod awareness;
pub mod cursor;
pub mod loro;
pub mod obs;
pub mod oplog;
pub mod txn;

pub mod change;
pub mod configure;
pub mod container;
pub mod dag;
pub mod encoding;
pub mod id;
pub mod op;
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

pub use error::{LoroError, LoroResult};
pub(crate) mod group;
pub(crate) mod macros;
pub(crate) mod state;
pub mod undo;
pub(crate) mod value;
pub(crate) use id::{PeerID, ID};

// TODO: rename as Key?
pub(crate) use loro_common::InternalString;

pub use container::ContainerType;
pub use encoding::json_schema::op::*;
pub use loro_common::{loro_value, to_value};
#[cfg(feature = "wasm")]
pub use value::wasm;
pub use value::{ApplyDiff, LoroValue, ToJson};
pub use version::VersionVector;

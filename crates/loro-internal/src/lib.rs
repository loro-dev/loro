//! loro-internal is a CRDT framework.
//!
//!
//!
//!
#![deny(clippy::undocumented_unsafe_blocks)]
#![warn(rustdoc::broken_intra_doc_links)]

pub mod arena;
pub mod diff_calc;
pub mod handler;
pub use event::{ContainerDiff, DiffEvent, DocDiff};
pub use handler::{ListHandler, MapHandler, TextHandler, TreeHandler};
pub use loro::LoroDoc;
pub use oplog::OpLog;
pub use state::DocState;
pub mod loro;
pub mod obs;
pub mod oplog;
pub mod snapshot_encode;
mod state;
pub mod txn;

pub mod change;
pub mod configure;
pub mod container;
pub mod dag;
mod encoding;
pub mod id;
pub mod op;
pub mod version;

mod error;
#[cfg(feature = "test_utils")]
pub mod fuzz;
mod span;
#[cfg(test)]
pub mod tests;
mod utils;

pub mod delta;
pub mod event;

pub use error::LoroError;
pub(crate) mod macros;
pub(crate) mod value;
pub(crate) use change::Timestamp;
pub(crate) use id::{PeerID, ID};

// TODO: rename as Key?
pub(crate) type InternalString = DefaultAtom;

pub use container::ContainerType;
pub use fxhash::FxHashMap;
pub use value::{ApplyDiff, LoroValue, ToJson};
pub use version::VersionVector;

use string_cache::DefaultAtom;



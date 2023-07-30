//! loro-internal is a CRDT framework.
//!
//!
//!
//!
#![deny(clippy::undocumented_unsafe_blocks)]
#![warn(rustdoc::broken_intra_doc_links)]

pub mod change;
pub mod configure;
pub mod container;
pub mod dag;
pub mod id;
pub mod log_store;
pub mod op;
pub mod refactor;
pub mod version;
pub use refactor::*;

mod error;
#[cfg(feature = "test_utils")]
pub mod fuzz;
mod smstring;
mod span;
#[cfg(test)]
pub mod tests;

pub mod delta;
pub mod event;

pub use error::LoroError;
pub(crate) mod macros;
pub(crate) mod value;
pub(crate) use change::Timestamp;
pub(crate) use id::{PeerID, ID};
pub(crate) use op::{ContentType, InsertContentTrait};

// TODO: rename as Key?
pub(crate) type InternalString = DefaultAtom;

pub use container::ContainerType;
pub use fxhash::FxHashMap;
pub use log_store::EncodeMode;
pub use value::{ApplyDiff, LoroValue, ToJson};
pub use version::VersionVector;

use string_cache::DefaultAtom;

pub(crate) use container::text;

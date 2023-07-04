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
pub mod context;
pub mod dag;
pub mod id;
pub mod log_store;
pub mod op;
mod refactor;
pub mod version;

mod error;
#[cfg(feature = "test_utils")]
pub mod fuzz;
mod hierarchy;
mod loro;
mod smstring;
mod span;
#[cfg(test)]
pub mod tests;
mod transaction;
pub use transaction::{Origin, Transact, Transaction, TransactionWrap};

pub mod delta;
pub mod event;
pub mod prelim;
mod value;

pub use error::LoroError;
pub(crate) mod macros;
pub(crate) use change::{Lamport, Timestamp};
pub(crate) use id::{PeerID, ID};
pub(crate) use op::{ContentType, InsertContentTrait, Op};

// TODO: rename as Key?
pub(crate) type InternalString = DefaultAtom;
pub use container::ContainerTrait;

pub use container::{list::List, map::Map, text::Text, ContainerType};
pub use fxhash::FxHashMap;
pub use log_store::{EncodeMode, LogStore};
pub use loro::LoroCore;
pub use value::LoroValue;
pub use version::VersionVector;

use string_cache::DefaultAtom;

#[cfg(feature = "test_utils")]
pub use container::text;

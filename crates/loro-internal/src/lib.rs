//! loro-internal is a CRDT framework.
//!
//!
//!
//!
#![deny(clippy::undocumented_unsafe_blocks)]

pub mod change;
pub mod configure;
pub mod container;
pub mod context;
pub mod dag;
pub mod id;
pub mod log_store;
pub mod op;
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

pub mod delta;
pub mod event;
mod prelim;
pub use prelim::{Prelim, PrelimValue};
mod value;

pub use error::LoroError;
pub(crate) mod macros;
pub(crate) use change::{Lamport, Timestamp};
pub(crate) use id::{ClientID, ID};
pub(crate) use op::{ContentType, InsertContentTrait, Op};

// TODO: rename as Key?
pub(crate) type InternalString = DefaultAtom;
pub(crate) use container::Container;

pub use container::{list::List, map::Map, text::Text, ContainerType};
pub use log_store::LogStore;
pub use loro::LoroCore;
pub use value::LoroValue;
pub use version::VersionVector;

use string_cache::DefaultAtom;

#[cfg(feature = "test_utils")]
pub use container::text;

//! loro-core is a CRDT framework.
//!
//!
//!
#![allow(dead_code)]

pub mod change;
pub mod configure;
pub mod container;
pub mod dag;
pub mod id;
pub mod log_store;
pub mod op;
pub mod version;

mod error;
mod loro;
mod smstring;
mod snapshot;
mod span;
#[cfg(test)]
mod tests;
mod value;

pub(crate) mod macros;
pub(crate) use change::{Change, Lamport, Timestamp};
pub(crate) use id::{ClientID, ID};
pub(crate) use op::{ContentType, InsertContent, Op, OpContent, OpType};

pub(crate) type InternalString = DefaultAtom;

pub use container::ContainerType;
pub use log_store::LogStore;
pub use loro::*;
pub use value::LoroValue;
pub use version::VersionVector;

use string_cache::DefaultAtom;

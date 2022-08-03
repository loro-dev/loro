//! # Loro
//!
//!
#![allow(dead_code, unused_imports)]

pub mod change;
pub mod configure;
pub mod container;
pub mod dag;
pub mod id;
pub mod op;
pub mod version;

mod error;
mod log_store;
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
pub(crate) use smstring::SmString;
pub(crate) type InternalString = DefaultAtom;

pub use container::ContainerType;
pub use log_store::LogStore;
pub use loro::*;
pub use value::LoroValue;

use string_cache::DefaultAtom;

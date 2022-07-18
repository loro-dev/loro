//! # Loro
//!
//!
#![allow(dead_code, unused_imports, clippy::explicit_auto_deref)]

pub mod change;
pub mod configure;
pub mod container;
pub mod id;
pub mod op;
pub mod version;

mod id_span;
mod log_store;
mod loro;
mod snapshot;
mod tests;
mod value;

pub(crate) mod macros;
pub(crate) use change::{Change, Lamport, Timestamp};
pub(crate) use id::{ClientID, ID};
pub(crate) use snapshot::Snapshot;
pub(crate) type SmString = SmartString<LazyCompact>;
pub(crate) use op::{ContentType, InsertContent, Op, OpContent, OpType};
pub(crate) type InternalString = DefaultAtom;

pub use container::ContainerType;
pub use log_store::LogStore;
pub use loro::*;
pub use value::LoroValue;

use smartstring::{LazyCompact, SmartString};
use string_cache::DefaultAtom;

//! # Loro
//!
//!
#![allow(dead_code, unused_imports)]

pub mod change;
pub mod configure;
pub mod id;
mod id_span;
mod log_store;
pub mod op;
mod snapshot;
mod value;
mod version;

pub mod container;

pub(crate) use change::{Change, Lamport, Timestamp};
pub(crate) use id::{ClientID, ID};
pub use log_store::LogStore;
pub(crate) use op::{ContentType, InsertContent, Op, OpContent, OpType};
use smartstring::{LazyCompact, SmartString};
pub(crate) use snapshot::Snapshot;
pub(crate) type SmString = SmartString<LazyCompact>;
use string_cache::DefaultAtom;
pub(crate) type InternalString = DefaultAtom;
pub use value::LoroValue;

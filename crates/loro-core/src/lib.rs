#![allow(dead_code, unused_imports)]

mod change;
mod id;
mod id_span;
mod log_store;
mod op;
mod snapshot;
mod value;
mod version;

pub mod container;

pub use change::{Change, Lamport, Timestamp};
pub use id::{ClientID, ID};
pub use log_store::LogStore;
pub use op::{content, ContentType, InsertContent, Op, OpContent, OpType};
use smartstring::{LazyCompact, SmartString};
pub use snapshot::Snapshot;
pub(crate) type SmString = SmartString<LazyCompact>;
use string_cache::DefaultAtom;
pub(crate) type InternalString = DefaultAtom;

#![allow(dead_code, unused_imports)]

mod change;
mod id;
mod id_span;
mod log_store;
mod op;
mod snapshot;

pub mod container;
pub mod text;

pub use change::{Change, Lamport, Timestamp};
pub use id::{ClientID, ID};
pub use log_store::LogStore;
pub use op::{content, ContentType, InsertContent, Op, OpContent, OpType};
use smartstring::{LazyCompact, SmartString};
pub(crate) type SmString = SmartString<LazyCompact>;
use string_cache::DefaultAtom;
pub(crate) type AtomString = DefaultAtom;

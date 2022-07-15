#![allow(dead_code, unused_imports)]

mod change;
mod id;
mod id_span;
mod log_store;
mod op;

pub mod container;
pub mod text;

pub use change::{Change, Lamport, Timestamp};
pub use id::{ClientID, ID};
pub use log_store::LogStore;
pub use op::{content, ContentType, InsertContent, Op, OpContent, OpType};
use smartstring::{LazyCompact, SmartString};
pub type SmString = SmartString<LazyCompact>;

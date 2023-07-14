use serde::{Deserialize, Serialize};
mod error;
mod id;
mod span;

pub use error::LoroError;
pub use span::*;
pub type PeerID = u64;
pub type Counter = i32;
pub type Lamport = u32;

#[derive(PartialEq, Eq, Hash, Clone, Copy, Serialize, Deserialize)]
pub struct ID {
    pub peer: PeerID,
    pub counter: Counter,
}

pub type IdSpanVector = fxhash::FxHashMap<PeerID, CounterSpan>;

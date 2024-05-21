mod op;
mod serde_impl;

pub use op::*;

use loro_common::{Lamport, LoroValue, ID};
use serde::{Deserialize, Serialize};
use smallvec::SmallVec;
#[derive(Debug, Serialize, Deserialize)]
pub struct LoroJsonSchema {
    pub loro_version: String,
    pub start_vv: String,
    pub end_vv: String,
    pub changes: Vec<Change>,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct Change {
    pub id: ID,
    pub timestamp: i64,
    pub deps: SmallVec<[ID; 2]>,
    pub lamport: Lamport,
    pub msg: Option<String>,

    pub ops: Vec<Op>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct JsonLoroValue(pub LoroValue);

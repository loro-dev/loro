use loro_common::{Lamport, ID};
use smallvec::SmallVec;

pub struct LoroJsonSchema {
    loro_version: String,
    start_vv: String,
    end_vv: String,
    schema_version: u8,
}

pub struct Change {
    id: ID,
    timestamp: u64,
    deps: SmallVec<[ID; 2]>,
    lamport: Lamport,
    msg: String,
    ops: Vec<Op>,
}

pub struct Op{
    id: ID,
    
}
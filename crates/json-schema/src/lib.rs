use loro_common::{ContainerID, Lamport, ID};
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
    lamport: u32,
    msg: String,
    ops: Vec<Op>,
}

pub struct Op {
    counter: i32,
    container: ContainerID,
    content: OpContent,
}

pub enum OpContent {
    List(ListOp),
    Map(MapOp),
    Text(TextOp),
    Tree(TreeOp),
}

struct 
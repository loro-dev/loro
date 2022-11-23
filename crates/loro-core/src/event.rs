use fxhash::{FxHashMap, FxHashSet};

use crate::{
    container::ContainerID, delta::Delta, id::ContainerIdx, version::Frontiers, InternalString,
    LoroValue,
};

pub(crate) struct RawEvent {
    container_idx: ContainerIdx,
    old_version: Frontiers,
    new_version: Frontiers,
    diff: Diff,
}

pub struct Event {
    pub old_version: Frontiers,
    pub new_version: Frontiers,
    pub current_target: ContainerID,
    pub target: ContainerID,
    /// the relative path from current_target to target
    pub relative_path: Path,
    pub diff: Vec<Diff>,
}

pub type Path = Vec<Index>;

pub enum Index {
    Key(InternalString),
    Index(usize),
}

pub enum Diff {
    List(Delta<Vec<LoroValue>>),
    Text(Delta<String>),
    Map(MapDiff),
}

pub struct ValuePair {
    pub old: LoroValue,
    pub new: LoroValue,
}

pub struct MapDiff {
    pub added: FxHashMap<InternalString, LoroValue>,
    pub updated: FxHashMap<InternalString, ValuePair>,
    pub deleted: FxHashSet<InternalString>,
}

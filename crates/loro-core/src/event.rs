use fxhash::{FxHashMap, FxHashSet};

use crate::{container::ContainerID, delta::Delta, version::Frontiers, InternalString, LoroValue};

#[derive(Debug)]
pub struct RawEvent {
    pub container_id: ContainerID,
    pub old_version: Frontiers,
    pub new_version: Frontiers,
    pub local: bool,
    pub diff: Vec<Diff>,
}

#[derive(Debug)]
pub struct Event {
    pub old_version: Frontiers,
    pub new_version: Frontiers,
    pub current_target: ContainerID,
    pub target: ContainerID,
    /// the relative path from current_target to target
    pub relative_path: Path,
    pub diff: Vec<Diff>,
    pub local: bool,
}

pub type Path = Vec<Index>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Index {
    Key(InternalString),
    Seq(usize),
}

#[derive(Clone, Debug)]
pub enum Diff {
    List(Delta<Vec<LoroValue>>),
    Text(Delta<String>),
    Map(MapDiff),
}

#[derive(Clone, Debug)]
pub struct ValuePair {
    pub old: LoroValue,
    pub new: LoroValue,
}

#[derive(Clone, Debug)]
pub struct MapDiff {
    pub added: FxHashMap<InternalString, LoroValue>,
    pub updated: FxHashMap<InternalString, ValuePair>,
    pub deleted: FxHashSet<InternalString>,
}

pub type Observer = Box<dyn Fn(&Event)>;

pub type SubscriptionID = u64;

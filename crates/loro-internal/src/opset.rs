use enum_dispatch::enum_dispatch;
use fxhash::FxHashMap;
use loro_common::{InternalString, TreeID, ID};

use crate::{
    change::Change,
    container::idx::ContainerIdx,
    op::{InnerContent, Op},
};

#[derive(Debug, Default, Clone)]
pub(crate) struct OpSet {
    ops: FxHashMap<ContainerIdx, OpsManager>,
}

impl OpSet {
    pub(crate) fn insert_by_change(&mut self, change: &Change) {
        let peer = change.peer();
        for op in change.ops.iter() {
            let container_idx = op.container;
            let manager = self
                .ops
                .entry(container_idx)
                .or_insert_with(|| match op.content {
                    InnerContent::Map(_) => OpsManager::Map(MapOpSet::default()),
                    _ => unreachable!(),
                });
            manager.insert(op, peer)
        }
    }
}

#[enum_dispatch(OpSetTrait)]
#[derive(Debug, Clone)]

enum OpsManager {
    // List(ListOpSet),
    Map(MapOpSet),
    // Tree(TreeOpSet),
}

enum OpSetKey {
    // List,
    Map(InternalString),
    // Tree(TreeID),
}

#[enum_dispatch]
trait OpSetTrait {
    fn insert(&mut self, op: &Op, peer: u64);
    fn get(&self, key: &OpSetKey) -> Option<&Vec<ID>>;
}

#[derive(Debug, Default, Clone)]
struct MapOpSet {
    ops: FxHashMap<InternalString, Vec<ID>>,
}

impl OpSetTrait for MapOpSet {
    fn insert(&mut self, op: &Op, peer: u64) {
        let key = match &op.content {
            InnerContent::Map(map) => map.key.clone(),
            _ => unreachable!(),
        };
        let entry = self.ops.entry(key).or_default();
        entry.push(ID::new(peer, op.counter));
    }

    fn get(&self, key: &OpSetKey) -> Option<&Vec<ID>> {
        match key {
            OpSetKey::Map(key) => self.ops.get(key),
        }
    }
}

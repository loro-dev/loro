use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, Mutex},
};

use enum_as_inner::EnumAsInner;
use enum_dispatch::enum_dispatch;
use fxhash::FxHashMap;
use loro_common::{Counter, HasId, InternalString, LoroValue, PeerID, ID};

use crate::{
    change::{Change, Lamport},
    container::{idx::ContainerIdx, tree::tree_op::TreeOp},
    diff_calc::tree::TreeCacheForDiff,
    op::{InnerContent, RichOp},
    VersionVector,
};

#[derive(Debug, Default, Clone)]
pub(crate) struct OpGroups {
    groups: FxHashMap<ContainerIdx, OpGroup>,
}

impl OpGroups {
    pub(crate) fn insert_by_change(&mut self, change: &Change) {
        for op in change.ops.iter() {
            let container_idx = op.container;
            let rich_op = RichOp::new_by_change(change, op);
            let manager = self
                .groups
                .entry(container_idx)
                .or_insert_with(|| match op.content {
                    InnerContent::Map(_) => OpGroup::Map(MapOpGroup::default()),
                    InnerContent::List(_) => OpGroup::List(ListOpGroup),
                    InnerContent::Tree(_) => OpGroup::Tree(TreeOpGroup::default()),
                });
            manager.insert(&rich_op)
        }
    }

    pub(crate) fn get(&self, container_idx: &ContainerIdx) -> Option<&OpGroup> {
        self.groups.get(container_idx)
    }

    pub(crate) fn get_tree(&self, container_idx: &ContainerIdx) -> Option<&TreeOpGroup> {
        self.groups
            .get(container_idx)
            .and_then(|group| match group {
                OpGroup::Tree(tree) => Some(tree),
                _ => None,
            })
    }

    pub(crate) fn get_map(&self, container_idx: &ContainerIdx) -> Option<&MapOpGroup> {
        self.groups
            .get(container_idx)
            .and_then(|group| match group {
                OpGroup::Map(map) => Some(map),
                _ => None,
            })
    }
}

#[enum_dispatch(OpGroupTrait)]
#[derive(Debug, Clone, EnumAsInner)]

pub(crate) enum OpGroup {
    List(ListOpGroup),
    Map(MapOpGroup),
    Tree(TreeOpGroup),
}

#[enum_dispatch]
trait OpGroupTrait {
    fn insert(&mut self, op: &RichOp);
}

#[derive(Debug, Clone)]
pub(crate) struct GroupedMapOpInfo {
    pub(crate) value: Option<LoroValue>,
    pub(crate) counter: Counter,
    pub(crate) lamport: Lamport,
    pub(crate) peer: PeerID,
}

impl PartialEq for GroupedMapOpInfo {
    fn eq(&self, other: &Self) -> bool {
        self.lamport == other.lamport && self.peer == other.peer
    }
}

impl Eq for GroupedMapOpInfo {}

impl PartialOrd for GroupedMapOpInfo {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for GroupedMapOpInfo {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.lamport
            .cmp(&other.lamport)
            .then_with(|| self.peer.cmp(&other.peer))
    }
}

#[derive(Debug, Default, Clone)]
pub(crate) struct MapOpGroup {
    ops: FxHashMap<InternalString, BTreeSet<GroupedMapOpInfo>>,
}

impl MapOpGroup {
    pub(crate) fn last_op(
        &self,
        key: &InternalString,
        vv: &VersionVector,
    ) -> Option<&GroupedMapOpInfo> {
        self.ops.get(key).and_then(|set| {
            set.iter()
                .rev()
                .find(|op| vv.get(&op.peer).copied().unwrap_or(0) > op.counter)
        })
    }
}

impl OpGroupTrait for MapOpGroup {
    fn insert(&mut self, op: &RichOp) {
        let key = match &op.op.content {
            InnerContent::Map(map) => map.key.clone(),
            _ => unreachable!(),
        };
        let entry = self.ops.entry(key).or_default();
        entry.insert(GroupedMapOpInfo {
            value: op.op.content.as_map().unwrap().value.clone(),
            counter: op.op.counter,
            lamport: op.lamport,
            peer: op.peer,
        });
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct GroupedTreeOpInfo {
    pub(crate) peer: PeerID,
    pub(crate) counter: Counter,
    pub(crate) value: TreeOp,
}

impl HasId for GroupedTreeOpInfo {
    fn id_start(&self) -> loro_common::ID {
        ID::new(self.peer, self.counter)
    }
}

impl PartialEq for GroupedTreeOpInfo {
    fn eq(&self, other: &Self) -> bool {
        self.peer == other.peer && self.counter == other.counter
    }
}

impl Eq for GroupedTreeOpInfo {}

impl PartialOrd for GroupedTreeOpInfo {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for GroupedTreeOpInfo {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.peer.cmp(&other.peer)
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct TreeOpGroup {
    pub(crate) ops: BTreeMap<Lamport, BTreeSet<GroupedTreeOpInfo>>,
    pub(crate) tree_for_diff: Arc<Mutex<TreeCacheForDiff>>,
}

impl OpGroupTrait for TreeOpGroup {
    fn insert(&mut self, op: &RichOp) {
        let tree_op = op.op.content.as_tree().unwrap();
        let target = tree_op.target;
        let parent = tree_op.parent;
        let entry = self.ops.entry(op.lamport).or_default();
        entry.insert(GroupedTreeOpInfo {
            value: TreeOp { target, parent },
            counter: op.op.counter,
            peer: op.peer,
        });
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ListOpGroup;

impl OpGroupTrait for ListOpGroup {
    fn insert(&mut self, _op: &RichOp) {}
}

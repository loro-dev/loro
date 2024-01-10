use std::collections::BTreeSet;

use enum_as_inner::EnumAsInner;
use enum_dispatch::enum_dispatch;
use fxhash::FxHashMap;
use loro_common::{InternalString, PeerID};

use crate::{
    change::{Change, Lamport},
    container::idx::ContainerIdx,
    op::{InnerContent, Op, RichOp},
    VersionVector,
};

#[derive(Debug, Default, Clone)]
pub(crate) struct OpGroup {
    groups: FxHashMap<ContainerIdx, ContainerOps>,
}

impl OpGroup {
    pub(crate) fn insert_by_change(&mut self, change: &Change) {
        for op in change.ops.iter() {
            let container_idx = op.container;
            let rich_op = RichOp::new_by_change(change, op);
            let manager = self
                .groups
                .entry(container_idx)
                .or_insert_with(|| match op.content {
                    InnerContent::Map(_) => ContainerOps::Map(MapOpGroup::default()),
                    InnerContent::List(_) => ContainerOps::List(ListOpSet),
                    InnerContent::Tree(_) => ContainerOps::Tree(TreeOpSet),
                });
            manager.insert(&rich_op)
        }
    }

    pub(crate) fn get(&self, container_idx: &ContainerIdx) -> Option<&ContainerOps> {
        self.groups.get(container_idx)
    }
}

#[enum_dispatch(OpSetTrait)]
#[derive(Debug, Clone, EnumAsInner)]

pub(crate) enum ContainerOps {
    List(ListOpSet),
    Map(MapOpGroup),
    Tree(TreeOpSet),
}

#[derive(Debug, Clone)]
struct ListOpSet;

impl OpSetTrait for ListOpSet {
    fn insert(&mut self, _op: &RichOp) {}
}

#[derive(Debug, Clone)]
struct TreeOpSet;

impl OpSetTrait for TreeOpSet {
    fn insert(&mut self, _op: &RichOp) {}
}

enum OpKey {
    // List,
    Map(InternalString),
    // Tree(TreeID),
}

#[enum_dispatch]
trait OpSetTrait {
    fn insert(&mut self, op: &RichOp);
}

#[derive(Debug, Clone)]
pub(crate) struct OpWithInfo {
    pub(crate) op: Op,
    pub(crate) lamport: Lamport,
    pub(crate) peer: PeerID,
}

impl<'a> From<&RichOp<'a>> for OpWithInfo {
    fn from(value: &RichOp<'a>) -> Self {
        OpWithInfo {
            op: value.op.clone(),
            lamport: value.lamport,
            peer: value.peer,
        }
    }
}

impl PartialOrd for OpWithInfo {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for OpWithInfo {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.lamport
            .cmp(&other.lamport)
            .then_with(|| self.peer.cmp(&other.peer))
    }
}

impl PartialEq for OpWithInfo {
    fn eq(&self, other: &Self) -> bool {
        self.lamport == other.lamport
            && self.peer == other.peer
            && self.op.container == other.op.container
    }
}

impl Eq for OpWithInfo {}

#[derive(Debug, Default, Clone)]
pub(crate) struct MapOpGroup {
    ops: FxHashMap<InternalString, BTreeSet<OpWithInfo>>,
}

impl MapOpGroup {
    pub(crate) fn get(&self, key: &OpKey) -> Option<impl Iterator<Item = &OpWithInfo>> {
        match key {
            OpKey::Map(key) => self.ops.get(key).map(|set| set.iter()),
        }
    }

    pub(crate) fn last_op(&self, key: &InternalString, vv: &VersionVector) -> Option<&OpWithInfo> {
        self.ops.get(key).and_then(|set| {
            set.iter()
                .rev()
                .find(|op| vv.get(&op.peer).copied().unwrap_or(0) > op.op.counter)
        })
    }
}

impl OpSetTrait for MapOpGroup {
    fn insert(&mut self, op: &RichOp) {
        let key = match &op.op.content {
            InnerContent::Map(map) => map.key.clone(),
            _ => unreachable!(),
        };
        let entry = self.ops.entry(key).or_default();
        entry.insert(OpWithInfo::from(op));
    }
}

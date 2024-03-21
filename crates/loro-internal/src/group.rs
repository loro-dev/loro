use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, Mutex},
};

use enum_as_inner::EnumAsInner;
use enum_dispatch::enum_dispatch;
use fxhash::FxHashMap;
use loro_common::{
    ContainerID, ContainerType, Counter, HasId, HasLamport, IdLp, InternalString, LoroValue,
    PeerID, ID,
};

use crate::{
    arena::SharedArena,
    change::{Change, Lamport},
    container::{idx::ContainerIdx, tree::tree_op::TreeOp},
    diff_calc::tree::TreeCacheForDiff,
    op::{InnerContent, RichOp},
    VersionVector,
};

#[derive(Debug, Clone)]
pub(crate) struct OpGroups {
    arena: SharedArena,
    groups: FxHashMap<ContainerIdx, OpGroup>,
}

impl OpGroups {
    pub(crate) fn new(arena: SharedArena) -> Self {
        Self {
            arena,
            groups: Default::default(),
        }
    }

    pub(crate) fn insert_by_change(&mut self, change: &Change) {
        // tracing::
        for op in change.ops.iter() {
            if matches!(
                op.container.get_type(),
                ContainerType::Text | ContainerType::List
            ) {
                continue;
            }

            let container_idx = op.container;
            let rich_op = RichOp::new_by_change(change, op);
            let manager =
                self.groups
                    .entry(container_idx)
                    .or_insert_with(|| match op.container.get_type() {
                        ContainerType::Map => OpGroup::Map(MapOpGroup::default()),
                        ContainerType::MovableList => {
                            OpGroup::MovableList(MovableListOpGroup::new(self.arena.clone()))
                        }
                        ContainerType::Tree => OpGroup::Tree(TreeOpGroup::default()),
                        _ => unreachable!(),
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

    #[allow(unused)]
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
    Map(MapOpGroup),
    Tree(TreeOpGroup),
    MovableList(MovableListOpGroup),
}

#[enum_dispatch]
trait OpGroupTrait {
    fn insert(&mut self, op: &RichOp);
}

#[derive(Debug, Clone)]
pub(crate) struct GroupedMapOpInfo<T = Option<LoroValue>> {
    pub(crate) value: T,
    pub(crate) counter: Counter,
    pub(crate) lamport: Lamport,
    pub(crate) peer: PeerID,
}

impl<T> PartialEq for GroupedMapOpInfo<T> {
    fn eq(&self, other: &Self) -> bool {
        self.lamport == other.lamport && self.peer == other.peer
    }
}

impl<T> Eq for GroupedMapOpInfo<T> {}

impl<T> PartialOrd for GroupedMapOpInfo<T> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl<T> Ord for GroupedMapOpInfo<T> {
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
        let key = match &op.raw_op().content {
            InnerContent::Map(map) => map.key.clone(),
            _ => unreachable!(),
        };
        let entry = self.ops.entry(key).or_default();
        entry.insert(GroupedMapOpInfo {
            value: op.raw_op().content.as_map().unwrap().value.clone(),
            counter: op.raw_op().counter,
            lamport: op.lamport(),
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
        let tree_op = op.raw_op().content.as_tree().unwrap();
        let target = tree_op.target;
        let parent = tree_op.parent;
        let entry = self.ops.entry(op.lamport()).or_default();
        entry.insert(GroupedTreeOpInfo {
            value: TreeOp { target, parent },
            counter: op.raw_op().counter,
            peer: op.peer,
        });
    }
}

#[derive(Debug, Clone)]
pub(crate) struct MovableListOpGroup {
    arena: SharedArena,
    /// mappings from elem_id to a set of target poses & values
    mappings: FxHashMap<IdLp, MovableListTarget>,
}

#[derive(Debug, Default, Clone)]
pub(crate) struct MovableListTarget {
    poses: BTreeSet<GroupedMapOpInfo<IdLp>>,
    values: BTreeSet<GroupedMapOpInfo<LoroValue>>,
}

impl OpGroupTrait for MovableListOpGroup {
    fn insert(&mut self, op: &RichOp) {
        let start_id = op.id_full().idlp();
        match &op.op().content {
            InnerContent::List(list) => match list {
                crate::container::list::list_op::InnerListOp::Set { elem_id, value } => {
                    let full_id = op.id_full();
                    let mapping = self.mappings.entry(*elem_id).or_default();
                    mapping.values.insert(GroupedMapOpInfo {
                        value: value.clone(),
                        counter: full_id.counter,
                        lamport: full_id.lamport,
                        peer: full_id.peer,
                    });
                }
                crate::container::list::list_op::InnerListOp::Insert { slice, pos: _ } => {
                    for (i, v) in self.arena.iter_value_slice(slice.to_range()).enumerate() {
                        let id = start_id.inc(i as i32);
                        let full_id = op.id_full().inc(i as i32);
                        let mapping = self.mappings.entry(id).or_default();
                        mapping.poses.insert(GroupedMapOpInfo {
                            value: full_id.idlp(),
                            counter: full_id.counter,
                            lamport: full_id.lamport,
                            peer: full_id.peer,
                        });
                        mapping.values.insert(GroupedMapOpInfo {
                            value: v,
                            counter: full_id.counter,
                            lamport: full_id.lamport,
                            peer: full_id.peer,
                        });
                    }
                }
                crate::container::list::list_op::InnerListOp::Move { from_id, .. } => {
                    let full_id = op.id_full();
                    let mapping = self.mappings.entry(*from_id).or_default();
                    mapping.poses.insert(GroupedMapOpInfo {
                        value: full_id.idlp(),
                        counter: full_id.counter,
                        lamport: full_id.lamport,
                        peer: full_id.peer,
                    });
                }
                // Don't mark deletions for now, but the cost is the state now may contain invalid elements
                // that are deleted but not removed from the state.
                // Maybe we can remove the elements with no valid pos mapping directly from the state. When
                // it's needed, we load it lazily from this group.
                crate::container::list::list_op::InnerListOp::Delete(_) => {}
                crate::container::list::list_op::InnerListOp::StyleStart { .. }
                | crate::container::list::list_op::InnerListOp::StyleEnd
                | crate::container::list::list_op::InnerListOp::InsertText { .. } => unreachable!(),
            },
            InnerContent::Map(_) => unreachable!(),
            InnerContent::Tree(_) => unreachable!(),
        };
    }
}

impl MovableListOpGroup {
    fn new(arena: SharedArena) -> Self {
        Self {
            arena,
            mappings: Default::default(),
        }
    }

    pub(crate) fn last_pos(
        &self,
        key: &IdLp,
        vv: &VersionVector,
    ) -> Option<&GroupedMapOpInfo<IdLp>> {
        self.mappings.get(key).and_then(|set| {
            set.poses
                .iter()
                .rev()
                .find(|op| vv.get(&op.peer).copied().unwrap_or(0) > op.counter)
        })
    }

    pub(crate) fn last_value(
        &self,
        key: &IdLp,
        vv: &VersionVector,
    ) -> Option<&GroupedMapOpInfo<LoroValue>> {
        self.mappings.get(key).and_then(|set| {
            set.values
                .iter()
                .rev()
                .find(|op| vv.get(&op.peer).copied().unwrap_or(0) > op.counter)
        })
    }
}

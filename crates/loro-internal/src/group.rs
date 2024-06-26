use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, Mutex},
};

use enum_as_inner::EnumAsInner;
use enum_dispatch::enum_dispatch;
use fxhash::FxHashMap;
use loro_common::{
    ContainerType, Counter, HasId, HasLamport, IdLp, InternalString, LoroValue, PeerID, ID,
};

use crate::{
    arena::SharedArena,
    change::{Change, Lamport},
    container::{idx::ContainerIdx, tree::tree_op::TreeOp},
    diff_calc::tree::TreeCacheForDiff,
    op::{InnerContent, RichOp},
    VersionVector,
};

#[derive(Debug)]
pub(crate) struct OpGroups {
    arena: SharedArena,
    groups: FxHashMap<ContainerIdx, OpGroup>,
}

impl OpGroups {
    pub(crate) fn fork(&self, arena: SharedArena) -> Self {
        let mut groups = FxHashMap::with_capacity_and_hasher(self.groups.len(), Default::default());
        for (container_idx, group) in self.groups.iter() {
            groups.insert(*container_idx, group.fork(&arena));
        }

        Self { arena, groups }
    }

    pub(crate) fn new(arena: SharedArena) -> Self {
        Self {
            arena,
            groups: Default::default(),
        }
    }

    pub(crate) fn insert_by_change(&mut self, change: &Change) {
        for op in change.ops.iter() {
            if matches!(
                op.container.get_type(),
                ContainerType::Text | ContainerType::List | ContainerType::Unknown(_)
            ) {
                continue;
            }

            #[cfg(feature = "counter")]
            if matches!(op.container.get_type(), ContainerType::Counter) {
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

    pub(crate) fn get_movable_list(
        &self,
        container_idx: &ContainerIdx,
    ) -> Option<&MovableListOpGroup> {
        self.groups
            .get(container_idx)
            .and_then(|group| match group {
                OpGroup::MovableList(movable_list) => Some(movable_list),
                _ => None,
            })
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

impl OpGroup {
    fn fork(&self, a: &SharedArena) -> Self {
        match self {
            OpGroup::Map(m) => OpGroup::Map(m.clone()),
            OpGroup::Tree(t) => OpGroup::Tree(TreeOpGroup {
                ops: t.ops.clone(),
                tree_for_diff: Arc::new(Mutex::new(Default::default())),
            }),
            OpGroup::MovableList(m) => OpGroup::MovableList(MovableListOpGroup {
                arena: a.clone(),
                elem_mappings: m.elem_mappings.clone(),
                pos_to_elem: m.pos_to_elem.clone(),
            }),
        }
    }
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

impl<T> GroupedMapOpInfo<T> {
    pub(crate) fn id(&self) -> ID {
        ID::new(self.peer, self.counter)
    }

    pub(crate) fn idlp(&self) -> IdLp {
        IdLp::new(self.peer, self.lamport)
    }
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
    ops: FxHashMap<InternalString, SmallSet<GroupedMapOpInfo>>,
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

#[derive(Debug, Clone)]
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
        let entry = self.ops.entry(op.lamport()).or_default();
        entry.insert(GroupedTreeOpInfo {
            value: tree_op.clone(),
            counter: op.raw_op().counter,
            peer: op.peer,
        });
    }
}

#[derive(Debug, Clone)]
pub(crate) struct MovableListOpGroup {
    arena: SharedArena,
    /// mappings from elem_id to a set of target poses & values
    elem_mappings: FxHashMap<IdLp, MovableListTarget>,
    /// mappings from pos to elem_id
    pos_to_elem: FxHashMap<IdLp, IdLp>,
}

#[derive(Debug, Clone)]
enum MovableListTarget {
    One { value: LoroValue, counter: Counter },
    Multiple(Box<MultipleInner>),
}

#[derive(Debug, Clone)]
struct MultipleInner {
    poses: BTreeSet<GroupedMapOpInfo<()>>,
    values: BTreeSet<GroupedMapOpInfo<LoroValue>>,
}

impl MovableListTarget {
    fn upgrade(&mut self, key_idlp: IdLp) -> &mut MultipleInner {
        match self {
            MovableListTarget::One { value, counter } => {
                let mut inner = MultipleInner {
                    poses: BTreeSet::default(),
                    values: BTreeSet::default(),
                };
                inner.poses.insert(GroupedMapOpInfo {
                    value: (),
                    counter: *counter,
                    lamport: key_idlp.lamport,
                    peer: key_idlp.peer,
                });
                inner.values.insert(GroupedMapOpInfo {
                    value: value.clone(),
                    counter: *counter,
                    lamport: key_idlp.lamport,
                    peer: key_idlp.peer,
                });
                *self = MovableListTarget::Multiple(Box::new(inner));
                match self {
                    MovableListTarget::Multiple(a) => a,
                    _ => unreachable!(),
                }
            }
            MovableListTarget::Multiple(a) => a,
        }
    }
}

impl OpGroupTrait for MovableListOpGroup {
    fn insert(&mut self, op: &RichOp) {
        let start_id = op.id_full().idlp();
        match &op.op().content {
            InnerContent::List(list) => match list {
                crate::container::list::list_op::InnerListOp::Set { elem_id, value } => {
                    let full_id = op.id_full();
                    let mapping = self
                        .elem_mappings
                        .entry(*elem_id)
                        .or_insert_with(|| {
                            MovableListTarget::Multiple(Box::new(MultipleInner {
                                poses: BTreeSet::default(),
                                values: BTreeSet::default(),
                            }))
                        })
                        .upgrade(*elem_id);
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
                        if let Some(target) = self.elem_mappings.get_mut(&id) {
                            let inner = target.upgrade(id);
                            inner.poses.insert(GroupedMapOpInfo {
                                value: (),
                                counter: full_id.counter,
                                lamport: full_id.lamport,
                                peer: full_id.peer,
                            });
                            inner.values.insert(GroupedMapOpInfo {
                                value: v.clone(),
                                counter: full_id.counter,
                                lamport: full_id.lamport,
                                peer: full_id.peer,
                            });
                        } else {
                            self.elem_mappings.insert(
                                id,
                                MovableListTarget::One {
                                    value: v,
                                    counter: full_id.counter,
                                },
                            );
                        }
                    }
                }
                crate::container::list::list_op::InnerListOp::Move { from_id, .. } => {
                    let full_id = op.id_full();
                    let mapping = self
                        .elem_mappings
                        .entry(*from_id)
                        .or_insert_with(|| {
                            MovableListTarget::Multiple(Box::new(MultipleInner {
                                poses: BTreeSet::default(),
                                values: BTreeSet::default(),
                            }))
                        })
                        .upgrade(*from_id);
                    mapping.poses.insert(GroupedMapOpInfo {
                        value: (),
                        counter: full_id.counter,
                        lamport: full_id.lamport,
                        peer: full_id.peer,
                    });
                    self.pos_to_elem.insert(full_id.idlp(), *from_id);
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
            InnerContent::Future(_) => unreachable!(),
        };
    }
}

impl MovableListOpGroup {
    fn new(arena: SharedArena) -> Self {
        Self {
            arena,
            pos_to_elem: Default::default(),
            elem_mappings: Default::default(),
        }
    }

    pub(crate) fn last_pos(&self, key: &IdLp, vv: &VersionVector) -> Option<GroupedMapOpInfo<()>> {
        let ans = self.elem_mappings.get(key).and_then(|set| match set {
            MovableListTarget::One { value: _, counter } => {
                if vv.get(&key.peer).copied().unwrap_or(0) > *counter {
                    Some(GroupedMapOpInfo {
                        value: (),
                        counter: *counter,
                        lamport: key.lamport,
                        peer: key.peer,
                    })
                } else {
                    None
                }
            }
            MovableListTarget::Multiple(m) => m
                .poses
                .iter()
                .rev()
                .find(|op| vv.get(&op.peer).copied().unwrap_or(0) > op.counter)
                .cloned(),
        });

        ans
    }

    pub(crate) fn last_value(
        &self,
        key: &IdLp,
        vv: &VersionVector,
    ) -> Option<GroupedMapOpInfo<LoroValue>> {
        self.elem_mappings.get(key).and_then(|set| match set {
            MovableListTarget::One { value, counter } => {
                if vv.get(&key.peer).copied().unwrap_or(0) > *counter {
                    Some(GroupedMapOpInfo {
                        value: value.clone(),
                        counter: *counter,
                        lamport: key.lamport,
                        peer: key.peer,
                    })
                } else {
                    None
                }
            }
            MovableListTarget::Multiple(m) => m
                .values
                .iter()
                .rev()
                .find(|op| vv.get(&op.peer).copied().unwrap_or(0) > op.counter)
                .cloned(),
        })
    }

    pub(crate) fn get_elem_from_pos(&self, pos: IdLp) -> IdLp {
        self.pos_to_elem.get(&pos).cloned().unwrap_or(pos)
    }
}

#[derive(Default, Clone, Debug)]
enum SmallSet<T> {
    #[default]
    Empty,
    One(T),
    Many(BTreeSet<T>),
}

struct SmallSetIter<'a, T> {
    set: &'a SmallSet<T>,
    one_itered: bool,
    iter: Option<std::collections::btree_set::Iter<'a, T>>,
}

impl<T: Ord> SmallSet<T> {
    fn insert(&mut self, new_value: T) {
        match self {
            SmallSet::Empty => *self = SmallSet::One(new_value),
            SmallSet::One(v) => {
                if v != &new_value {
                    let mut set = BTreeSet::new();
                    let SmallSet::One(v) = std::mem::take(self) else {
                        unreachable!()
                    };
                    set.insert(v);
                    set.insert(new_value);
                    *self = SmallSet::Many(set);
                }
            }
            SmallSet::Many(set) => {
                set.insert(new_value);
            }
        }
    }

    fn iter(&self) -> SmallSetIter<T> {
        SmallSetIter {
            set: self,
            one_itered: false,
            iter: match self {
                SmallSet::Empty => None,
                SmallSet::One(_) => None,
                SmallSet::Many(set) => Some(set.iter()),
            },
        }
    }
}

impl<'a, T> DoubleEndedIterator for SmallSetIter<'a, T> {
    fn next_back(&mut self) -> Option<Self::Item> {
        match self.iter {
            Some(ref mut iter) => iter.next_back(),
            None => {
                if self.one_itered {
                    None
                } else {
                    self.one_itered = true;
                    match self.set {
                        SmallSet::One(v) => Some(v),
                        _ => None,
                    }
                }
            }
        }
    }
}

impl<'a, T> Iterator for SmallSetIter<'a, T> {
    type Item = &'a T;

    fn next(&mut self) -> Option<Self::Item> {
        match self.set {
            SmallSet::Empty => None,
            SmallSet::One(v) => {
                if self.one_itered {
                    None
                } else {
                    self.one_itered = true;
                    Some(v)
                }
            }
            SmallSet::Many(_) => self.iter.as_mut().unwrap().next(),
        }
    }
}

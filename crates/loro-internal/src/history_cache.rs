use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, Mutex, Weak},
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
    oplog::ChangeStore,
    OpLog, VersionVector,
};

/// A cache for the history of a container.
///
/// There are cases where a container needs a view of the history of the container.
/// For example, when switching to an older version, a map container may need to know the value of a key at a certain version.
/// This cache provides a faster way to do that than scanning the oplog.
#[derive(Debug)]
pub(crate) struct ContainerHistoryCache {
    arena: SharedArena,
    change_store: ChangeStore,
    for_checkout: Option<FxHashMap<ContainerIdx, HistoryCacheForCheckout>>,
    for_importing: Option<FxHashMap<ContainerIdx, HistoryCacheForImporting>>,
}

impl ContainerHistoryCache {
    pub(crate) fn fork(&self, arena: SharedArena, change_store: ChangeStore) -> Self {
        let mut ans = Self {
            arena,
            change_store,
            for_checkout: None,
            for_importing: None,
        };

        if let Some(old_for_checkout) = &self.for_checkout {
            let mut for_checkout =
                FxHashMap::with_capacity_and_hasher(old_for_checkout.len(), Default::default());
            for (container_idx, group) in old_for_checkout.iter() {
                for_checkout.insert(*container_idx, group.fork(&ans.arena));
            }

            ans.for_checkout = Some(for_checkout);
        }

        if let Some(old_for_importing) = &self.for_importing {
            let mut for_importing =
                FxHashMap::with_capacity_and_hasher(old_for_importing.len(), Default::default());
            for (container_idx, group) in old_for_importing.iter() {
                for_importing.insert(*container_idx, group.fork(&ans.arena));
            }

            ans.for_importing = Some(for_importing);
        }

        ans
    }

    pub(crate) fn new(arena: SharedArena, change_store: ChangeStore) -> Self {
        Self {
            arena,
            change_store,
            for_checkout: Default::default(),
            for_importing: Default::default(),
        }
    }

    pub(crate) fn insert_by_new_change(
        &mut self,
        change: &Change,
        for_checkout: bool,
        for_importing: bool,
    ) {
        if self.for_checkout.is_none() && self.for_importing.is_none() {
            return;
        }

        for op in change.ops.iter() {
            match op.container.get_type() {
                ContainerType::Map | ContainerType::MovableList
                    if self.for_checkout.is_some() && for_checkout =>
                {
                    let container_idx = op.container;
                    let rich_op = RichOp::new_by_change(change, op);
                    let manager = self
                        .for_checkout
                        .as_mut()
                        .unwrap()
                        .entry(container_idx)
                        .or_insert_with(|| match op.container.get_type() {
                            ContainerType::Map => {
                                HistoryCacheForCheckout::Map(MapOpGroup::default())
                            }
                            ContainerType::MovableList => HistoryCacheForCheckout::MovableList(
                                MovableListOpGroup::new(self.arena.clone()),
                            ),
                            _ => unreachable!(),
                        });
                    manager.insert(&rich_op)
                }
                ContainerType::Tree if self.for_importing.is_some() && for_importing => {
                    let container_idx = op.container;
                    let rich_op = RichOp::new_by_change(change, op);
                    let manager = self
                        .for_importing
                        .as_mut()
                        .unwrap()
                        .entry(container_idx)
                        .or_insert_with(|| match op.container.get_type() {
                            ContainerType::Tree => {
                                HistoryCacheForImporting::Tree(TreeOpGroup::default())
                            }
                            _ => unreachable!(),
                        });
                    manager.insert(&rich_op)
                }
                _ => continue,
            }
        }
    }

    pub(crate) fn ensure_all_caches_exist(&mut self) {
        let mut record_for_checkout = false;
        let mut record_for_importing = false;
        if self.for_checkout.is_none() {
            self.for_checkout = Some(FxHashMap::default());
            record_for_checkout = true;
        }

        if self.for_importing.is_none() {
            self.for_importing = Some(FxHashMap::default());
            record_for_importing = true;
        }

        if !record_for_checkout && !record_for_importing {
            return;
        }

        self.init_cache_by_visit_all_change_slow(record_for_checkout, record_for_importing);
    }

    pub(crate) fn ensure_importing_caches_exist(&mut self) {
        if self.for_importing.is_some() {
            return;
        }

        self.for_importing = Some(FxHashMap::default());
        self.init_cache_by_visit_all_change_slow(false, true);
    }

    fn init_cache_by_visit_all_change_slow(&mut self, for_checkout: bool, for_importing: bool) {
        self.change_store.visit_all_changes(&mut |c| {
            if self.for_checkout.is_none() && self.for_importing.is_none() {
                return;
            }

            for op in c.ops.iter() {
                match op.container.get_type() {
                    ContainerType::Map | ContainerType::MovableList
                        if self.for_checkout.is_some() && for_checkout =>
                    {
                        let container_idx = op.container;
                        let rich_op = RichOp::new_by_change(c, op);
                        let manager = self
                            .for_checkout
                            .as_mut()
                            .unwrap()
                            .entry(container_idx)
                            .or_insert_with(|| match op.container.get_type() {
                                ContainerType::Map => {
                                    HistoryCacheForCheckout::Map(MapOpGroup::default())
                                }
                                ContainerType::MovableList => HistoryCacheForCheckout::MovableList(
                                    MovableListOpGroup::new(self.arena.clone()),
                                ),
                                _ => unreachable!(),
                            });
                        manager.insert(&rich_op)
                    }
                    ContainerType::Tree if self.for_importing.is_some() && for_importing => {
                        let container_idx = op.container;
                        let rich_op = RichOp::new_by_change(c, op);
                        let manager = self
                            .for_importing
                            .as_mut()
                            .unwrap()
                            .entry(container_idx)
                            .or_insert_with(|| match op.container.get_type() {
                                ContainerType::Tree => {
                                    HistoryCacheForImporting::Tree(TreeOpGroup::default())
                                }
                                _ => unreachable!(),
                            });
                        manager.insert(&rich_op)
                    }
                    _ => continue,
                }
            }
        });
    }

    pub(crate) fn get_checkout_cache(
        &mut self,
        container_idx: &ContainerIdx,
    ) -> Option<&HistoryCacheForCheckout> {
        self.ensure_all_caches_exist();
        self.for_checkout.as_ref().unwrap().get(container_idx)
    }

    pub(crate) fn get_importing_cache_unsafe(
        &self,
        container_idx: &ContainerIdx,
    ) -> Option<&HistoryCacheForImporting> {
        self.for_importing.as_ref().unwrap().get(container_idx)
    }

    pub(crate) fn get_movable_list(
        &mut self,
        container_idx: &ContainerIdx,
    ) -> Option<&MovableListOpGroup> {
        self.ensure_all_caches_exist();
        self.for_checkout
            .as_ref()
            .unwrap()
            .get(container_idx)
            .and_then(|group| match group {
                HistoryCacheForCheckout::MovableList(movable_list) => Some(movable_list),
                _ => None,
            })
    }

    pub(crate) fn get_tree(&mut self, container_idx: &ContainerIdx) -> Option<&TreeOpGroup> {
        self.ensure_importing_caches_exist();
        self.for_importing
            .as_ref()
            .unwrap()
            .get(container_idx)
            .map(|group| match group {
                HistoryCacheForImporting::Tree(tree) => tree,
            })
    }

    pub(crate) fn get_tree_unsafe(&self, container_idx: &ContainerIdx) -> Option<&TreeOpGroup> {
        self.for_importing
            .as_ref()
            .unwrap()
            .get(container_idx)
            .map(|group| match group {
                HistoryCacheForImporting::Tree(tree) => tree,
            })
    }

    #[allow(unused)]
    pub(crate) fn get_map(&mut self, container_idx: &ContainerIdx) -> Option<&MapOpGroup> {
        self.ensure_all_caches_exist();
        self.for_checkout
            .as_ref()
            .unwrap()
            .get(container_idx)
            .and_then(|group| match group {
                HistoryCacheForCheckout::Map(map) => Some(map),
                _ => None,
            })
    }
}

#[enum_dispatch(OpGroupTrait)]
#[derive(Debug, Clone, EnumAsInner)]
pub(crate) enum HistoryCacheForCheckout {
    Map(MapOpGroup),
    MovableList(MovableListOpGroup),
}

#[enum_dispatch(OpGroupTrait)]
#[derive(Debug, Clone, EnumAsInner)]
pub(crate) enum HistoryCacheForImporting {
    Tree(TreeOpGroup),
}

impl HistoryCacheForCheckout {
    fn fork(&self, a: &SharedArena) -> Self {
        match self {
            HistoryCacheForCheckout::Map(m) => HistoryCacheForCheckout::Map(m.clone()),
            HistoryCacheForCheckout::MovableList(m) => {
                HistoryCacheForCheckout::MovableList(MovableListOpGroup {
                    arena: a.clone(),
                    elem_mappings: m.elem_mappings.clone(),
                })
            }
        }
    }
}

impl HistoryCacheForImporting {
    fn fork(&self, a: &SharedArena) -> Self {
        match self {
            HistoryCacheForImporting::Tree(t) => HistoryCacheForImporting::Tree(TreeOpGroup {
                ops: t.ops.clone(),
                tree_for_diff: Arc::new(Mutex::new(Default::default())),
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

    pub(crate) fn keys(&self) -> impl Iterator<Item = &InternalString> {
        self.ops.keys()
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
                crate::container::list::list_op::InnerListOp::Move {
                    elem_id: from_id, ..
                } => {
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

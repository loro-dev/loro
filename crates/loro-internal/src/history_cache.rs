use std::{
    collections::{BTreeMap, BTreeSet},
    ops::Bound,
    sync::{Arc, Mutex},
};

use enum_as_inner::EnumAsInner;
use enum_dispatch::enum_dispatch;
use fxhash::FxHashMap;
use loro_common::{
    ContainerType, Counter, HasId, HasLamport, IdFull, IdLp, InternalString, LoroValue, PeerID, ID,
};
use rle::HasLength;

use crate::{
    arena::SharedArena,
    change::{Change, Lamport},
    container::{idx::ContainerIdx, list::list_op::InnerListOp, tree::tree_op::TreeOp},
    delta::MovableListInnerDelta,
    diff_calc::tree::TreeCacheForDiff,
    encoding::value_register::ValueRegister,
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
    for_checkout: Option<ForCheckout>,
    for_importing: Option<FxHashMap<ContainerIdx, HistoryCacheForImporting>>,
}

#[derive(Debug, Default)]
pub(crate) struct ForCheckout {
    pub(crate) map: MapHistoryCache,
    pub(crate) movable_list: MovableListHistoryCache,
}

impl HistoryCacheTrait for ForCheckout {
    fn insert(&mut self, op: &RichOp) {
        match op.raw_op().container.get_type() {
            ContainerType::Map => self.map.insert(op),
            ContainerType::MovableList => self.movable_list.insert(op),
            _ => {}
        }
    }
}

impl ContainerHistoryCache {
    pub(crate) fn fork(&self, arena: SharedArena, change_store: ChangeStore) -> Self {
        Self {
            arena,
            change_store,
            for_checkout: None,
            for_importing: None,
        }
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
                    let rich_op = RichOp::new_by_change(change, op);
                    self.for_checkout.as_mut().unwrap().insert(&rich_op)
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

    pub(crate) fn get_checkout_index(&mut self) -> &ForCheckout {
        self.ensure_all_caches_exist();
        self.for_checkout.as_ref().unwrap()
    }

    pub(crate) fn ensure_all_caches_exist(&mut self) {
        let mut record_for_checkout = false;
        let mut record_for_importing = false;
        if self.for_checkout.is_none() {
            self.for_checkout = Some(ForCheckout::default());
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
                        let rich_op = RichOp::new_by_change(c, op);
                        self.for_checkout.as_mut().unwrap().insert(&rich_op)
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

    pub(crate) fn get_importing_cache_unsafe(
        &self,
        container_idx: &ContainerIdx,
    ) -> Option<&HistoryCacheForImporting> {
        self.for_importing.as_ref().unwrap().get(container_idx)
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

    pub(crate) fn has_cache(&self) -> bool {
        self.for_checkout.is_some()
    }

    pub(crate) fn free(&mut self) {
        self.for_checkout = None;
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
    fn insert(&mut self, op: &RichOp) {
        match self {
            HistoryCacheForImporting::Tree(t) => t.insert(op),
        }
    }
}

#[enum_dispatch]
trait HistoryCacheTrait {
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

#[derive(Debug, Ord, PartialOrd, Eq, PartialEq, Clone)]
struct MapHistoryCacheEntry {
    container: ContainerIdx,
    key: u32,
    lamport: Lamport,
    peer: PeerID,
    counter: Counter,
}

#[derive(Debug, Default)]
pub(crate) struct MapHistoryCache {
    keys: ValueRegister<InternalString>,
    map: BTreeSet<MapHistoryCacheEntry>,
}

impl HistoryCacheTrait for MapHistoryCache {
    fn insert(&mut self, op: &RichOp) {
        let container = op.raw_op().container;
        let key = match &op.raw_op().content {
            InnerContent::Map(map) => map.key.clone(),
            _ => unreachable!(),
        };

        let key_idx = self.keys.register(&key);
        self.map.insert(MapHistoryCacheEntry {
            container,
            key: key_idx as u32,
            lamport: op.lamport(),
            peer: op.peer,
            counter: op.counter(),
        });
    }
}

impl MapHistoryCache {
    pub fn get_container_latest_op_at_vv(
        &self,
        container: ContainerIdx,
        vv: &VersionVector,
        // PERF: utilize this lamport
        _max_lamport: Lamport,
        oplog: &OpLog,
    ) -> FxHashMap<InternalString, GroupedMapOpInfo> {
        let mut ans = FxHashMap::default();
        let mut last_key = u32::MAX;

        'outer: loop {
            let range = (
                Bound::Included(MapHistoryCacheEntry {
                    container,
                    key: 0,
                    lamport: 0,
                    peer: 0,
                    counter: 0,
                }),
                Bound::Excluded(MapHistoryCacheEntry {
                    container,
                    key: last_key,
                    lamport: 0,
                    peer: 0,
                    counter: 0,
                }),
            );

            for entry in self.map.range(range).rev() {
                if vv.get(&entry.peer).copied().unwrap_or(0) > entry.counter {
                    let id = ID::new(entry.peer, entry.counter);
                    let op = oplog.get_op_that_includes(id).unwrap();
                    debug_assert_eq!(op.atom_len(), 1);
                    match &op.content {
                        InnerContent::Map(map) => {
                            ans.insert(
                                self.keys.get_value(entry.key as usize).unwrap().clone(),
                                GroupedMapOpInfo {
                                    value: map.value.clone(),
                                    counter: id.counter,
                                    lamport: entry.lamport,
                                    peer: entry.peer,
                                },
                            );
                        }
                        _ => unreachable!(),
                    }
                    last_key = entry.key;
                    continue 'outer;
                }
            }

            break;
        }

        ans
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

impl HistoryCacheTrait for MapOpGroup {
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

impl HistoryCacheTrait for TreeOpGroup {
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

#[derive(Debug, Default)]
pub(crate) struct MovableListHistoryCache {
    pub(crate) move_set: BTreeSet<MovableListInnerDeltaEntry>,
    pub(crate) set_set: BTreeSet<MovableListInnerDeltaEntry>,
}

#[derive(Debug, Ord, PartialOrd, Eq, PartialEq, Clone)]
struct MovableListInnerDeltaEntry {
    element_lamport: Lamport,
    element_peer: PeerID,
    lamport: Lamport,
    peer: PeerID,
    counter: Counter,
}

impl HistoryCacheTrait for MovableListHistoryCache {
    fn insert(&mut self, op: &RichOp) {
        let cur_id = op.id_full();
        match &op.op().content {
            InnerContent::List(l) => match l {
                crate::container::list::list_op::InnerListOp::Move { from, elem_id, to } => {
                    self.move_set.insert(MovableListInnerDeltaEntry {
                        element_lamport: elem_id.lamport,
                        element_peer: elem_id.peer,
                        lamport: cur_id.lamport,
                        peer: cur_id.peer,
                        counter: cur_id.counter,
                    });
                }
                crate::container::list::list_op::InnerListOp::Set { elem_id, value } => {
                    self.set_set.insert(MovableListInnerDeltaEntry {
                        element_lamport: elem_id.lamport,
                        element_peer: elem_id.peer,
                        lamport: cur_id.lamport,
                        peer: cur_id.peer,
                        counter: cur_id.counter,
                    });
                }
                _ => {}
            },
            _ => unreachable!(),
        }
    }
}

impl MovableListHistoryCache {
    pub(crate) fn last_value(
        &self,
        key: IdLp,
        vv: &VersionVector,
        max_lamport: Lamport,
        oplog: &OpLog,
    ) -> Option<GroupedMapOpInfo<LoroValue>> {
        self.set_set
            .range((
                Bound::Included(MovableListInnerDeltaEntry {
                    element_lamport: key.lamport,
                    element_peer: key.peer,
                    lamport: 0,
                    peer: 0,
                    counter: 0,
                }),
                Bound::Excluded(MovableListInnerDeltaEntry {
                    element_lamport: key.lamport,
                    element_peer: key.peer,
                    lamport: max_lamport,
                    peer: PeerID::MAX,
                    counter: Counter::MAX,
                }),
            ))
            .rev()
            .find(|e| vv.get(&e.peer).copied().unwrap_or(0) > e.counter)
            .map_or_else(
                || {
                    let id = oplog.idlp_to_id(key).unwrap();
                    if vv.get(&id.peer).copied().unwrap_or(0) <= id.counter {
                        return None;
                    }

                    let op = oplog.get_op_that_includes(id).unwrap();
                    let offset = id.counter - op.counter;
                    match &op.content {
                        InnerContent::List(InnerListOp::Insert { slice, .. }) => {
                            let value = oplog
                                .arena
                                .get_value(slice.0.start as usize + offset as usize)
                                .unwrap();
                            Some(GroupedMapOpInfo {
                                value,
                                counter: id.counter,
                                lamport: key.lamport,
                                peer: id.peer,
                            })
                        }
                        _ => {
                            unreachable!()
                        }
                    }
                },
                |e| {
                    let id = ID::new(e.peer, e.counter);
                    let op = oplog.get_op_that_includes(id).unwrap();
                    debug_assert_eq!(op.atom_len(), 1);
                    let lamport = op.lamport();
                    match &op.content {
                        InnerContent::List(InnerListOp::Set { value, .. }) => {
                            Some(GroupedMapOpInfo {
                                value: value.clone(),
                                counter: id.counter,
                                lamport,
                                peer: id.peer,
                            })
                        }
                        _ => {
                            unreachable!()
                        }
                    }
                },
            )
    }

    pub(crate) fn last_pos(
        &self,
        key: IdLp,
        vv: &VersionVector,
        max_lamport: Lamport,
        oplog: &OpLog,
    ) -> Option<IdFull> {
        self.move_set
            .range((
                Bound::Included(MovableListInnerDeltaEntry {
                    element_lamport: key.lamport,
                    element_peer: key.peer,
                    lamport: 0,
                    peer: 0,
                    counter: 0,
                }),
                Bound::Excluded(MovableListInnerDeltaEntry {
                    element_lamport: key.lamport,
                    element_peer: key.peer,
                    lamport: max_lamport,
                    peer: PeerID::MAX,
                    counter: Counter::MAX,
                }),
            ))
            .rev()
            .find(|e| vv.get(&e.peer).copied().unwrap_or(0) > e.counter)
            .map_or_else(
                || {
                    let id = oplog.idlp_to_id(key).unwrap();
                    if vv.get(&id.peer).copied().unwrap_or(0) > id.counter {
                        Some(IdFull::new(id.peer, id.counter, key.lamport))
                    } else {
                        None
                    }
                },
                |e| {
                    let id = ID::new(e.peer, e.counter);
                    let lamport = oplog.get_lamport_at(id).unwrap();
                    Some(IdFull::new(e.peer, e.counter, lamport))
                },
            )
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

impl HistoryCacheTrait for MovableListOpGroup {
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

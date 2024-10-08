use std::{
    collections::{BTreeMap, BTreeSet},
    marker::PhantomData,
    ops::Bound,
    sync::{Arc, Mutex},
};

use either::Either;
use enum_as_inner::EnumAsInner;
use enum_dispatch::enum_dispatch;
use fxhash::FxHashMap;
use generic_btree::rle::Sliceable;
use loro_common::{
    ContainerType, Counter, HasLamport, IdFull, IdLp, InternalString, LoroValue, PeerID, ID,
};
use rle::HasLength;
use tracing::trace;

use crate::{
    change::{Change, Lamport},
    container::{
        idx::ContainerIdx, list::list_op::InnerListOp,
        richtext::richtext_state::RichtextStateChunk, tree::tree_op::TreeOp,
    },
    delta::MapValue,
    diff_calc::tree::{MoveLamportAndID, TreeCacheForDiff},
    encoding::value_register::ValueRegister,
    op::{InnerContent, RichOp, SliceWithId},
    oplog::ChangeStore,
    state::{ContainerCreationContext, GcStore},
    OpLog, VersionVector,
};

/// A cache for the history of a container.
///
/// There are cases where a container needs a view of the history of the container.
/// For example, when switching to an older version, a map container may need to know the value of a key at a certain version.
/// This cache provides a faster way to do that than scanning the oplog.
#[derive(Debug)]
pub(crate) struct ContainerHistoryCache {
    change_store: ChangeStore,
    shallow_root_state: Option<Arc<GcStore>>,
    for_checkout: Option<ForCheckout>,
    for_importing: Option<FxHashMap<ContainerIdx, HistoryCacheForImporting>>,
}

#[derive(Debug, Default)]
pub(crate) struct ForCheckout {
    pub(crate) map: MapHistoryCache,
    pub(crate) movable_list: MovableListHistoryCache,
}

#[derive(Clone, Copy)]
pub(crate) struct HasImportingCacheMark {
    _private: PhantomData<()>,
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
    pub(crate) fn fork(&self, change_store: ChangeStore, gc: Option<Arc<GcStore>>) -> Self {
        Self {
            change_store,
            for_checkout: None,
            for_importing: None,
            shallow_root_state: gc,
        }
    }

    pub(crate) fn new(change_store: ChangeStore, gc: Option<Arc<GcStore>>) -> Self {
        Self {
            change_store,
            for_checkout: Default::default(),
            for_importing: Default::default(),
            shallow_root_state: gc,
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

    pub(crate) fn ensure_importing_caches_exist(&mut self) -> HasImportingCacheMark {
        if self.for_importing.is_some() {
            return HasImportingCacheMark {
                _private: PhantomData,
            };
        }

        self.for_importing = Some(FxHashMap::default());
        self.init_cache_by_visit_all_change_slow(false, true);
        HasImportingCacheMark {
            _private: PhantomData,
        }
    }

    fn init_cache_by_visit_all_change_slow(&mut self, for_checkout: bool, for_importing: bool) {
        if self.for_checkout.is_none() && self.for_importing.is_none() {
            return;
        }

        if !for_checkout && !for_importing {
            return;
        }

        self.change_store.visit_all_changes(&mut |c| {
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

        let default_ctx = ContainerCreationContext {
            configure: &Default::default(),
            peer: 0,
        };

        trace!("init_cache_by_visit_all_change_slow");
        if let Some(state) = self.shallow_root_state.as_ref() {
            trace!("init_cache_by_visit_all_change_slow with gc");
            ensure_cov::notify_cov(
                "loro_internal::history_cache::init_cache_by_visit_all_change_slow::visit_gc",
            );
            let mut store = state.store.try_lock().unwrap();
            for (idx, c) in store.iter_all_containers_mut() {
                match idx.get_type() {
                    ContainerType::Text | ContainerType::List | ContainerType::Unknown(_) => {
                        continue
                    }
                    #[cfg(feature = "counter")]
                    ContainerType::Counter => continue,
                    ContainerType::Map => {}
                    ContainerType::MovableList => {}
                    ContainerType::Tree => {}
                }

                let state = c.get_state_mut(*idx, default_ctx);

                match state {
                    crate::state::State::MapState(m) => {
                        if for_checkout {
                            let c = self.for_checkout.as_mut().unwrap();
                            for (k, v) in m.iter() {
                                c.map.record_shallow_root_state_entry(*idx, k, v);
                            }
                        }
                    }
                    crate::state::State::MovableListState(l) => {
                        for (idlp, elem) in l.elements() {
                            if for_checkout {
                                let c = self.for_checkout.as_mut().unwrap();
                                let item = l.get_list_item(elem.pos).unwrap();
                                c.movable_list.record_shallow_root_state(
                                    item.id,
                                    idlp.peer,
                                    idlp.lamport.into(),
                                    elem.value.clone(),
                                );
                            }
                        }
                    }
                    crate::state::State::TreeState(t) => {
                        if for_importing {
                            let c = self.for_importing.as_mut().unwrap();
                            let tree = c.entry(*idx).or_insert_with(|| {
                                HistoryCacheForImporting::Tree(Default::default())
                            });
                            tree.as_tree_mut().unwrap().record_shallow_root_state(
                                t.tree_nodes()
                                    .into_iter()
                                    .map(|node| MoveLamportAndID {
                                        id: node.last_move_op,
                                        op: Arc::new(TreeOp::Create {
                                            target: node.id,
                                            parent: node.parent.tree_id(),
                                            position: node.fractional_index.clone(),
                                        }),
                                        effected: true,
                                    })
                                    .collect(),
                            );
                        }
                    }
                    _ => unreachable!(),
                }
            }
        }
    }

    pub(crate) fn get_importing_cache(
        &self,
        container_idx: &ContainerIdx,
        _: HasImportingCacheMark,
    ) -> Option<&HistoryCacheForImporting> {
        self.for_importing.as_ref().unwrap().get(container_idx)
    }

    pub(crate) fn get_tree(
        &self,
        container_idx: &ContainerIdx,
        _: HasImportingCacheMark,
    ) -> Option<&TreeOpGroup> {
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

    pub(crate) fn set_shallow_root_store(&mut self, shallow_root_store: Option<Arc<GcStore>>) {
        self.shallow_root_state = shallow_root_store;
    }

    pub(crate) fn find_text_chunks_in(
        &self,
        idx: ContainerIdx,
        target_span: loro_common::IdSpan,
    ) -> Vec<RichtextStateChunk> {
        ensure_cov::notify_cov("loro_internal::history_cache::find_text_chunks_in");
        let Some(state) = self.shallow_root_state.as_ref() else {
            return Vec::new();
        };

        let mut binding = state.store.try_lock().unwrap();
        let Some(text) = binding.get_mut(idx) else {
            return Vec::new();
        };

        let text_state = text
            .get_state(
                idx,
                ContainerCreationContext {
                    configure: &Default::default(),
                    peer: 0,
                },
            )
            .as_richtext_state()
            .unwrap();

        // TODO: FIXME: This could be super slow in some cases
        let mut ans = Vec::new();
        text_state.iter_raw(&mut |chunk| {
            let c_span = chunk.get_id_span();
            if let Some(range) = target_span.get_slice_range_on(&c_span) {
                let c = chunk.slice(range.0..range.1);
                ans.push(c);
            }
        });

        ans.sort_unstable_by_key(|x| x.counter());
        ans
    }

    pub(crate) fn find_list_chunks_in(
        &self,
        idx: ContainerIdx,
        target_span: loro_common::IdSpan,
    ) -> Vec<SliceWithId> {
        ensure_cov::notify_cov("loro_internal::history_cache::find_list_chunks_in");
        let Some(state) = self.shallow_root_state.as_ref() else {
            return Vec::new();
        };

        let mut binding = state.store.try_lock().unwrap();
        let Some(list) = binding.get_mut(idx) else {
            return Vec::new();
        };

        let list_state = list.get_state(
            idx,
            ContainerCreationContext {
                configure: &Default::default(),
                peer: 0,
            },
        );

        let mut ans = Vec::with_capacity(target_span.atom_len());
        match list_state {
            crate::state::State::ListState(list) => {
                for v in list.iter_with_id() {
                    if target_span.contains(v.id.id()) {
                        ans.push(SliceWithId {
                            values: Either::Right(v.v.clone()),
                            id: v.id,
                            elem_id: None,
                        })
                    }
                }
            }
            crate::state::State::MovableListState(list) => {
                for (move_id, elem_id, v) in list.iter_with_last_move_id_and_elem_id() {
                    if target_span.contains(move_id.id()) {
                        ans.push(SliceWithId {
                            values: Either::Right(v.clone()),
                            id: move_id,
                            elem_id: Some(elem_id),
                        })
                    }
                }
            }
            _ => unreachable!(),
        }

        ans.sort_unstable_by_key(|x| x.id.counter);
        ans
    }
}

#[enum_dispatch(OpGroupTrait)]
#[derive(Debug, EnumAsInner)]
pub(crate) enum HistoryCacheForImporting {
    Tree(TreeOpGroup),
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

#[derive(Debug, Clone)]
struct MapHistoryCacheEntry {
    container: ContainerIdx,
    key: u32,
    lamport: Lamport,
    peer: PeerID,
    counter_or_value: Either<Counter, Box<Option<LoroValue>>>,
}

impl PartialEq for MapHistoryCacheEntry {
    fn eq(&self, other: &Self) -> bool {
        self.container == other.container
            && self.key == other.key
            && self.lamport == other.lamport
            && self.peer == other.peer
    }
}

impl Eq for MapHistoryCacheEntry {}

impl PartialOrd for MapHistoryCacheEntry {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for MapHistoryCacheEntry {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.container
            .cmp(&other.container)
            .then_with(|| self.key.cmp(&other.key))
            .then_with(|| self.lamport.cmp(&other.lamport))
            .then_with(|| self.peer.cmp(&other.peer))
    }
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
            counter_or_value: Either::Left(op.counter()),
        });
    }
}

impl MapHistoryCache {
    fn record_shallow_root_state_entry(
        &mut self,
        idx: ContainerIdx,
        k: &InternalString,
        v: &MapValue,
    ) {
        let key_idx = self.keys.register(k);
        self.map.insert(MapHistoryCacheEntry {
            container: idx,
            key: key_idx as u32,
            lamport: v.lamport(),
            peer: v.peer,
            counter_or_value: Either::Right(Box::new(v.value.clone())),
        });
    }

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
                    counter_or_value: Either::Left(0),
                }),
                Bound::Excluded(MapHistoryCacheEntry {
                    container,
                    key: last_key,
                    lamport: 0,
                    peer: 0,
                    counter_or_value: Either::Left(0),
                }),
            );

            for entry in self.map.range(range).rev() {
                match &entry.counter_or_value {
                    Either::Left(cnt) => {
                        if vv.get(&entry.peer).copied().unwrap_or(0) > *cnt {
                            let id = ID::new(entry.peer, *cnt);
                            let op = oplog.get_op_that_includes(id).unwrap();
                            debug_assert_eq!(op.atom_len(), 1);
                            match &op.content {
                                InnerContent::Map(map) => {
                                    ans.insert(
                                        self.keys.get_value(entry.key as usize).unwrap().clone(),
                                        GroupedMapOpInfo {
                                            value: map.value.clone(),
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
                    Either::Right(v) => {
                        let k = self.keys.get_value(entry.key as usize).unwrap().clone();
                        ans.insert(
                            k,
                            GroupedMapOpInfo {
                                value: (**v).clone(),
                                lamport: entry.lamport,
                                peer: entry.peer,
                            },
                        );
                        last_key = entry.key;
                        continue 'outer;
                    }
                }
            }

            break;
        }

        ans
    }
}

#[derive(Debug, Clone)]
pub(crate) struct GroupedTreeOpInfo {
    pub(crate) counter: Counter,
    pub(crate) value: Arc<TreeOp>,
}

#[derive(Debug, Default)]
pub(crate) struct TreeOpGroup {
    ops: BTreeMap<IdLp, GroupedTreeOpInfo>,
    tree_for_diff: Mutex<TreeCacheForDiff>,
}

impl HistoryCacheTrait for TreeOpGroup {
    fn insert(&mut self, op: &RichOp) {
        let tree_op = op.raw_op().content.as_tree().unwrap();
        self.ops.insert(
            op.idlp(),
            GroupedTreeOpInfo {
                value: tree_op.clone(),
                counter: op.raw_op().counter,
            },
        );
    }
}

impl TreeOpGroup {
    pub fn ops(&self) -> &BTreeMap<IdLp, GroupedTreeOpInfo> {
        &self.ops
    }

    pub fn tree(&self) -> &Mutex<TreeCacheForDiff> {
        &self.tree_for_diff
    }

    pub(crate) fn record_shallow_root_state(&mut self, nodes: Vec<MoveLamportAndID>) {
        let mut tree = self.tree_for_diff.try_lock().unwrap();
        for node in nodes.iter() {
            self.ops.insert(
                node.id.idlp(),
                GroupedTreeOpInfo {
                    counter: node.id.counter,
                    value: node.op.clone(),
                },
            );
        }
        tree.init_tree_with_shallow_root_version(nodes);
    }
}

#[derive(Debug, Default)]
pub(crate) struct MovableListHistoryCache {
    move_set: BTreeSet<MovableListInnerDeltaEntry>,
    set_set: BTreeSet<MovableListSetDeltaEntry>,
}

#[derive(Debug, Eq, PartialEq, Clone)]
struct MovableListInnerDeltaEntry {
    element_lamport: Lamport,
    element_peer: PeerID,
    lamport: Lamport,
    peer: PeerID,
    counter: Counter,
}

#[derive(Debug, Eq, PartialEq, Clone)]
struct MovableListSetDeltaEntry {
    element_lamport: Lamport,
    element_peer: PeerID,
    lamport: Lamport,
    peer: PeerID,
    counter_or_value: Either<Counter, Box<LoroValue>>,
}

impl Ord for MovableListInnerDeltaEntry {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.element_lamport
            .cmp(&other.element_lamport)
            .then_with(|| self.element_peer.cmp(&other.element_peer))
            .then_with(|| self.lamport.cmp(&other.lamport))
            .then_with(|| self.peer.cmp(&other.peer))
    }
}

impl PartialOrd for MovableListInnerDeltaEntry {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialOrd for MovableListSetDeltaEntry {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for MovableListSetDeltaEntry {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.element_lamport
            .cmp(&other.element_lamport)
            .then_with(|| self.element_peer.cmp(&other.element_peer))
            .then_with(|| self.lamport.cmp(&other.lamport))
            .then_with(|| self.peer.cmp(&other.peer))
    }
}

impl HistoryCacheTrait for MovableListHistoryCache {
    fn insert(&mut self, op: &RichOp) {
        let cur_id = op.id_full();
        match &op.op().content {
            InnerContent::List(l) => match l {
                crate::container::list::list_op::InnerListOp::Move { elem_id, .. } => {
                    self.move_set.insert(MovableListInnerDeltaEntry {
                        element_lamport: elem_id.lamport,
                        element_peer: elem_id.peer,
                        lamport: cur_id.lamport,
                        peer: cur_id.peer,
                        counter: cur_id.counter,
                    });
                }
                crate::container::list::list_op::InnerListOp::Set { elem_id, .. } => {
                    self.set_set.insert(MovableListSetDeltaEntry {
                        element_lamport: elem_id.lamport,
                        element_peer: elem_id.peer,
                        lamport: cur_id.lamport,
                        peer: cur_id.peer,
                        counter_or_value: Either::Left(cur_id.counter),
                    });
                }
                _ => {}
            },
            _ => unreachable!(),
        }
    }
}

impl MovableListHistoryCache {
    pub(crate) fn record_shallow_root_state(
        &mut self,
        id: IdFull,
        elem_peer: PeerID,
        elem_lamport: Lamport,
        value: LoroValue,
    ) {
        self.set_set.insert(MovableListSetDeltaEntry {
            element_lamport: elem_lamport,
            element_peer: elem_peer,
            lamport: id.lamport,
            peer: id.peer,
            counter_or_value: Either::Right(Box::new(value.clone())),
        });
        self.move_set.insert(MovableListInnerDeltaEntry {
            element_lamport: elem_lamport,
            element_peer: elem_peer,
            lamport: id.lamport,
            peer: id.peer,
            counter: id.counter,
        });
    }

    pub(crate) fn last_value(
        &self,
        key: IdLp,
        vv: &VersionVector,
        max_lamport: Lamport,
        oplog: &OpLog,
    ) -> Option<GroupedMapOpInfo<LoroValue>> {
        self.set_set
            .range((
                Bound::Included(MovableListSetDeltaEntry {
                    element_lamport: key.lamport,
                    element_peer: key.peer,
                    lamport: 0,
                    peer: 0,
                    counter_or_value: Either::Left(0),
                }),
                Bound::Excluded(MovableListSetDeltaEntry {
                    element_lamport: key.lamport,
                    element_peer: key.peer,
                    lamport: max_lamport,
                    peer: PeerID::MAX,
                    counter_or_value: Either::Left(Counter::MAX),
                }),
            ))
            .rev()
            .find(|e| {
                let counter = match &e.counter_or_value {
                    Either::Left(c) => *c,
                    Either::Right(_) => -1,
                };
                vv.get(&e.peer).copied().unwrap_or(0) > counter
            })
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
                    let (lamport, value) = match &e.counter_or_value {
                        Either::Left(c) => {
                            let id = ID::new(e.peer, *c);
                            let op = oplog.get_op_that_includes(id).unwrap();
                            debug_assert_eq!(op.atom_len(), 1);
                            let lamport = op.lamport();
                            match &op.content {
                                InnerContent::List(InnerListOp::Set { value, .. }) => {
                                    (lamport, value.clone())
                                }
                                _ => {
                                    unreachable!()
                                }
                            }
                        }
                        Either::Right(v) => (e.lamport, v.as_ref().clone()),
                    };
                    Some(GroupedMapOpInfo {
                        value: value.clone(),
                        lamport,
                        peer: e.peer,
                    })
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
                    let lamport = oplog.get_lamport_at(id).unwrap_or(e.lamport);
                    Some(IdFull::new(e.peer, e.counter, lamport))
                },
            )
    }
}

use std::{
    borrow::Cow,
    io::Write,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex, RwLock, Weak,
    },
};

use container_store::ContainerStore;
use dead_containers_cache::DeadContainersCache;
use enum_as_inner::EnumAsInner;
use enum_dispatch::enum_dispatch;
use fxhash::{FxHashMap, FxHashSet};
use itertools::Itertools;
use loro_common::{ContainerID, LoroError, LoroResult, TreeID};
use loro_delta::DeltaItem;
use tracing::{info_span, instrument, warn};

use crate::{
    configure::{Configure, DefaultRandom, SecureRandomGenerator},
    container::{idx::ContainerIdx, richtext::config::StyleConfigMap, ContainerIdRaw},
    cursor::Cursor,
    delta::TreeExternalDiff,
    diff_calc::{DiffCalculator, DiffMode},
    encoding::{StateSnapshotDecodeContext, StateSnapshotEncoder},
    event::{Diff, EventTriggerKind, Index, InternalContainerDiff, InternalDiff},
    fx_map,
    handler::ValueOrHandler,
    id::PeerID,
    op::{Op, RawOp},
    txn::Transaction,
    version::Frontiers,
    ContainerDiff, ContainerType, DocDiff, InternalString, LoroDocInner, LoroValue, OpLog,
};

pub(crate) mod analyzer;
pub(crate) mod container_store;
#[cfg(feature = "counter")]
mod counter_state;
mod dead_containers_cache;
mod list_state;
mod map_state;
mod movable_list_state;
mod richtext_state;
mod tree_state;
mod unknown_state;

pub(crate) use self::movable_list_state::{IndexType, MovableListState};
pub(crate) use container_store::GcStore;
pub(crate) use list_state::ListState;
pub(crate) use map_state::MapState;
pub(crate) use richtext_state::RichtextState;
pub(crate) use tree_state::FiIfNotConfigured;
pub(crate) use tree_state::{get_meta_value, FractionalIndexGenResult, NodePosition, TreeState};
pub use tree_state::{TreeNode, TreeNodeWithChildren, TreeParentId};

use self::{container_store::ContainerWrapper, unknown_state::UnknownState};

#[cfg(feature = "counter")]
use self::counter_state::CounterState;

use super::{arena::SharedArena, event::InternalDocDiff};

pub struct DocState {
    pub(super) peer: Arc<AtomicU64>,

    pub(super) frontiers: Frontiers,
    // pub(super) states: FxHashMap<ContainerIdx, State>,
    pub(super) store: ContainerStore,
    pub(super) arena: SharedArena,
    pub(crate) config: Configure,
    // resolve event stuff
    doc: Weak<LoroDocInner>,
    // txn related stuff
    in_txn: bool,
    changed_idx_in_txn: FxHashSet<ContainerIdx>,

    // diff related stuff
    event_recorder: EventRecorder,

    dead_containers_cache: DeadContainersCache,
}

impl std::fmt::Debug for DocState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DocState")
            .field("peer", &self.peer)
            .finish()
    }
}

#[derive(Clone, Copy)]
pub(crate) struct ContainerCreationContext<'a> {
    pub configure: &'a Configure,
    pub peer: PeerID,
}

pub(crate) struct DiffApplyContext<'a> {
    pub mode: DiffMode,
    pub doc: &'a Weak<LoroDocInner>,
}

pub(crate) trait FastStateSnapshot {
    fn encode_snapshot_fast<W: Write>(&mut self, w: W);
    fn decode_value(bytes: &[u8]) -> LoroResult<(LoroValue, &[u8])>;
    fn decode_snapshot_fast(
        idx: ContainerIdx,
        v: (LoroValue, &[u8]),
        ctx: ContainerCreationContext,
    ) -> LoroResult<Self>
    where
        Self: Sized;
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ApplyLocalOpReturn {
    pub deleted_containers: Vec<ContainerID>,
}

#[enum_dispatch]
pub(crate) trait ContainerState {
    fn container_idx(&self) -> ContainerIdx;
    fn estimate_size(&self) -> usize;

    fn is_state_empty(&self) -> bool;

    #[must_use]
    fn apply_diff_and_convert(&mut self, diff: InternalDiff, ctx: DiffApplyContext) -> Diff;

    fn apply_diff(&mut self, diff: InternalDiff, ctx: DiffApplyContext);

    fn apply_local_op(&mut self, raw_op: &RawOp, op: &Op) -> LoroResult<ApplyLocalOpReturn>;
    /// Convert a state to a diff, such that an empty state will be transformed into the same as this state when it's applied.
    fn to_diff(&mut self, doc: &Weak<LoroDocInner>) -> Diff;

    fn get_value(&mut self) -> LoroValue;

    /// Get the index of the child container
    #[allow(unused)]
    fn get_child_index(&self, id: &ContainerID) -> Option<Index>;

    #[allow(unused)]
    fn contains_child(&self, id: &ContainerID) -> bool;

    #[allow(unused)]
    fn get_child_containers(&self) -> Vec<ContainerID>;

    /// Encode the ops and the blob that can be used to restore the state to the current state.
    ///
    /// State will use the provided encoder to encode the ops and export a blob.
    /// The ops should be encoded into the snapshot as well as the blob.
    /// The users then can use the ops and the blob to restore the state to the current state.
    fn encode_snapshot(&self, encoder: StateSnapshotEncoder) -> Vec<u8>;

    /// Restore the state to the state represented by the ops and the blob that exported by `get_snapshot_ops`
    fn import_from_snapshot_ops(&mut self, ctx: StateSnapshotDecodeContext) -> LoroResult<()>;
    fn fork(&self, config: &Configure) -> Self;
}

impl<T: FastStateSnapshot> FastStateSnapshot for Box<T> {
    fn encode_snapshot_fast<W: Write>(&mut self, w: W) {
        self.as_mut().encode_snapshot_fast(w)
    }

    fn decode_value(bytes: &[u8]) -> LoroResult<(LoroValue, &[u8])> {
        T::decode_value(bytes)
    }

    fn decode_snapshot_fast(
        idx: ContainerIdx,
        v: (LoroValue, &[u8]),
        ctx: ContainerCreationContext,
    ) -> LoroResult<Self>
    where
        Self: Sized,
    {
        T::decode_snapshot_fast(idx, v, ctx).map(|x| Box::new(x))
    }
}

impl<T: ContainerState> ContainerState for Box<T> {
    fn container_idx(&self) -> ContainerIdx {
        self.as_ref().container_idx()
    }

    fn estimate_size(&self) -> usize {
        self.as_ref().estimate_size()
    }

    fn is_state_empty(&self) -> bool {
        self.as_ref().is_state_empty()
    }

    fn apply_diff_and_convert(&mut self, diff: InternalDiff, ctx: DiffApplyContext) -> Diff {
        self.as_mut().apply_diff_and_convert(diff, ctx)
    }

    fn apply_diff(&mut self, diff: InternalDiff, ctx: DiffApplyContext) {
        self.as_mut().apply_diff(diff, ctx)
    }

    fn apply_local_op(&mut self, raw_op: &RawOp, op: &Op) -> LoroResult<ApplyLocalOpReturn> {
        self.as_mut().apply_local_op(raw_op, op)
    }

    #[doc = r" Convert a state to a diff, such that an empty state will be transformed into the same as this state when it's applied."]
    fn to_diff(&mut self, doc: &Weak<LoroDocInner>) -> Diff {
        self.as_mut().to_diff(doc)
    }

    fn get_value(&mut self) -> LoroValue {
        self.as_mut().get_value()
    }

    #[doc = r" Get the index of the child container"]
    #[allow(unused)]
    fn get_child_index(&self, id: &ContainerID) -> Option<Index> {
        self.as_ref().get_child_index(id)
    }

    fn contains_child(&self, id: &ContainerID) -> bool {
        self.as_ref().contains_child(id)
    }

    #[allow(unused)]
    fn get_child_containers(&self) -> Vec<ContainerID> {
        self.as_ref().get_child_containers()
    }

    #[doc = r" Encode the ops and the blob that can be used to restore the state to the current state."]
    #[doc = r""]
    #[doc = r" State will use the provided encoder to encode the ops and export a blob."]
    #[doc = r" The ops should be encoded into the snapshot as well as the blob."]
    #[doc = r" The users then can use the ops and the blob to restore the state to the current state."]
    fn encode_snapshot(&self, encoder: StateSnapshotEncoder) -> Vec<u8> {
        self.as_ref().encode_snapshot(encoder)
    }

    #[doc = r" Restore the state to the state represented by the ops and the blob that exported by `get_snapshot_ops`"]
    fn import_from_snapshot_ops(&mut self, ctx: StateSnapshotDecodeContext) -> LoroResult<()> {
        self.as_mut().import_from_snapshot_ops(ctx)
    }

    fn fork(&self, config: &Configure) -> Self {
        Box::new(self.as_ref().fork(config))
    }
}

#[allow(clippy::enum_variant_names)]
#[enum_dispatch(ContainerState)]
#[derive(EnumAsInner, Debug)]
pub enum State {
    ListState(Box<ListState>),
    MovableListState(Box<MovableListState>),
    MapState(Box<MapState>),
    RichtextState(Box<RichtextState>),
    TreeState(Box<TreeState>),
    #[cfg(feature = "counter")]
    CounterState(Box<counter_state::CounterState>),
    UnknownState(UnknownState),
}

impl From<ListState> for State {
    fn from(s: ListState) -> Self {
        Self::ListState(Box::new(s))
    }
}

impl From<RichtextState> for State {
    fn from(s: RichtextState) -> Self {
        Self::RichtextState(Box::new(s))
    }
}

impl From<MovableListState> for State {
    fn from(s: MovableListState) -> Self {
        Self::MovableListState(Box::new(s))
    }
}

impl From<MapState> for State {
    fn from(s: MapState) -> Self {
        Self::MapState(Box::new(s))
    }
}

impl From<TreeState> for State {
    fn from(s: TreeState) -> Self {
        Self::TreeState(Box::new(s))
    }
}

#[cfg(feature = "counter")]
impl From<CounterState> for State {
    fn from(s: CounterState) -> Self {
        Self::CounterState(Box::new(s))
    }
}

impl State {
    pub fn new_list(idx: ContainerIdx) -> Self {
        Self::ListState(Box::new(ListState::new(idx)))
    }

    pub fn new_map(idx: ContainerIdx) -> Self {
        Self::MapState(Box::new(MapState::new(idx)))
    }

    pub fn new_richtext(idx: ContainerIdx, config: Arc<RwLock<StyleConfigMap>>) -> Self {
        Self::RichtextState(Box::new(RichtextState::new(idx, config)))
    }

    pub fn new_tree(idx: ContainerIdx, peer: PeerID) -> Self {
        Self::TreeState(Box::new(TreeState::new(idx, peer)))
    }

    pub fn new_unknown(idx: ContainerIdx) -> Self {
        Self::UnknownState(UnknownState::new(idx))
    }

    pub fn encode_snapshot_fast<W: Write>(&mut self, mut w: W) {
        match self {
            State::ListState(s) => s.encode_snapshot_fast(&mut w),
            State::MovableListState(s) => s.encode_snapshot_fast(&mut w),
            State::MapState(s) => s.encode_snapshot_fast(&mut w),
            State::RichtextState(s) => s.encode_snapshot_fast(&mut w),
            State::TreeState(s) => s.encode_snapshot_fast(&mut w),
            #[cfg(feature = "counter")]
            State::CounterState(s) => s.encode_snapshot_fast(&mut w),
            State::UnknownState(s) => s.encode_snapshot_fast(&mut w),
        }
    }

    pub fn fork(&self, config: &Configure) -> Self {
        match self {
            State::ListState(list_state) => State::ListState(list_state.fork(config)),
            State::MovableListState(movable_list_state) => {
                State::MovableListState(movable_list_state.fork(config))
            }
            State::MapState(map_state) => State::MapState(map_state.fork(config)),
            State::RichtextState(richtext_state) => {
                State::RichtextState(richtext_state.fork(config))
            }
            State::TreeState(tree_state) => State::TreeState(tree_state.fork(config)),
            #[cfg(feature = "counter")]
            State::CounterState(counter_state) => State::CounterState(counter_state.fork(config)),
            State::UnknownState(unknown_state) => State::UnknownState(unknown_state.fork(config)),
        }
    }
}

impl DocState {
    #[inline]
    pub fn new_arc(doc: Weak<LoroDocInner>, config: Configure) -> Arc<Mutex<Self>> {
        let peer = DefaultRandom.next_u64();
        let arena = doc.upgrade().unwrap().arena.clone();
        // TODO: maybe we should switch to certain version in oplog?

        let peer = Arc::new(AtomicU64::new(peer));
        Arc::new(Mutex::new(Self {
            store: ContainerStore::new(arena.clone(), config.clone(), peer.clone()),
            peer,
            arena,
            frontiers: Frontiers::default(),
            doc,
            config,
            in_txn: false,
            changed_idx_in_txn: FxHashSet::default(),
            event_recorder: Default::default(),
            dead_containers_cache: Default::default(),
        }))
    }

    pub fn fork_with_new_peer_id(
        &mut self,
        doc: Weak<LoroDocInner>,
        config: Configure,
    ) -> Arc<Mutex<Self>> {
        let arena = doc.upgrade().unwrap().arena.clone();
        let peer = Arc::new(AtomicU64::new(DefaultRandom.next_u64()));
        let store = self.store.fork(arena.clone(), peer.clone(), config.clone());
        Arc::new(Mutex::new(Self {
            peer,
            frontiers: self.frontiers.clone(),
            store,
            arena,
            config,
            doc,
            in_txn: false,
            changed_idx_in_txn: FxHashSet::default(),
            event_recorder: Default::default(),
            dead_containers_cache: Default::default(),
        }))
    }

    pub fn start_recording(&mut self) {
        if self.is_recording() {
            return;
        }

        self.event_recorder.recording_diff = true;
        self.event_recorder.diff_start_version = Some(self.frontiers.clone());
    }

    #[inline(always)]
    pub fn stop_and_clear_recording(&mut self) {
        self.event_recorder = Default::default();
    }

    #[inline(always)]
    pub fn is_recording(&self) -> bool {
        self.event_recorder.recording_diff
    }

    pub fn refresh_peer_id(&mut self) {
        self.peer.store(
            DefaultRandom.next_u64(),
            std::sync::atomic::Ordering::Relaxed,
        );
    }

    /// Take all the diffs that are recorded and convert them to events.
    pub fn take_events(&mut self) -> Vec<DocDiff> {
        if !self.is_recording() {
            return vec![];
        }

        self.convert_current_batch_diff_into_event();
        std::mem::take(&mut self.event_recorder.events)
    }

    /// Record the next diff.
    /// Caller should call [pre_txn] before calling this.
    ///
    /// # Panic
    ///
    /// Panic when the diff cannot be merged with the previous diff.
    /// Caller should call [pre_txn] before calling this to avoid panic.
    fn record_diff(&mut self, diff: InternalDocDiff) {
        if !self.event_recorder.recording_diff || diff.diff.is_empty() {
            return;
        }

        let Some(last_diff) = self.event_recorder.diffs.last_mut() else {
            self.event_recorder.diffs.push(diff.into_owned());
            return;
        };

        if last_diff.can_merge(&diff) {
            self.event_recorder.diffs.push(diff.into_owned());
            return;
        }

        panic!("should call pre_txn before record_diff")
    }

    /// This should be called when DocState is going to apply a transaction / a diff.
    fn pre_txn(&mut self, next_origin: InternalString, next_trigger: EventTriggerKind) {
        if !self.is_recording() {
            return;
        }

        let Some(last_diff) = self.event_recorder.diffs.last() else {
            return;
        };

        if last_diff.origin == next_origin && last_diff.by == next_trigger {
            return;
        }

        // current diff batch cannot merge with the incoming diff,
        // need to convert all the current diffs into event
        self.convert_current_batch_diff_into_event()
    }

    fn convert_current_batch_diff_into_event(&mut self) {
        let recorder = &mut self.event_recorder;
        if recorder.diffs.is_empty() {
            return;
        }

        let diffs = std::mem::take(&mut recorder.diffs);
        let start = recorder.diff_start_version.take().unwrap();
        recorder.diff_start_version = Some((*diffs.last().unwrap().new_version).to_owned());
        let event = self.diffs_to_event(diffs, start);
        self.event_recorder.events.push(event);
    }

    /// Change the peer id of this doc state.
    /// It changes the peer id for the future txn on this AppState
    #[inline]
    pub fn set_peer_id(&mut self, peer: PeerID) {
        self.peer.store(peer, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn peer_id(&self) -> PeerID {
        self.peer.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// It's expected that diff only contains [`InternalDiff`]
    ///
    #[instrument(skip_all)]
    pub(crate) fn apply_diff(&mut self, mut diff: InternalDocDiff<'static>, diff_mode: DiffMode) {
        if self.in_txn {
            panic!("apply_diff should not be called in a transaction");
        }

        match diff_mode {
            DiffMode::Checkout => {
                self.dead_containers_cache.clear();
            }
            _ => {
                self.dead_containers_cache.clear_alive();
            }
        }

        let is_recording = self.is_recording();
        self.pre_txn(diff.origin.clone(), diff.by);
        let Cow::Owned(mut diffs) = std::mem::take(&mut diff.diff) else {
            unreachable!()
        };

        // # Revival
        //
        // A Container, if it is deleted from its parent Container, will still exist
        // in the internal state of Loro;  whereas on the user side, a tree structure
        // is maintained following Events, and at this point, the corresponding state
        // is considered deleted.
        //
        // Sometimes, this "pseudo-dead" Container may be revived (for example, through
        // backtracking or parallel editing),  and the user side should receive an Event
        // that restores the consistency between the revived Container and the  internal
        // state of Loro. This Event is required to restore the pseudo-dead  Container
        // State to its current state on Loro, and we refer to this process as "revival".
        //
        // Revival occurs during the application of the internal diff, and this operation
        // is necessary when it needs to be converted into an external Event.
        //
        // We can utilize the output of the Diff to determine which child nodes should be revived.
        //
        // For nodes that are to be revived, we can disregard the Events output by their
        // round of apply_diff_and_convert,  and instead, directly convert their state into
        // an Event once their application is complete.
        //
        // Suppose A is revived and B is A's child, and B also needs to be revived; therefore,
        // we should process each level alternately.

        // We need to ensure diff is processed in order
        diffs.sort_by_cached_key(|diff| self.arena.get_depth(diff.idx));
        let mut to_revive_in_next_layer: FxHashSet<ContainerIdx> = FxHashSet::default();
        let mut to_revive_in_this_layer: FxHashSet<ContainerIdx> = FxHashSet::default();
        let mut last_depth = 0;
        let len = diffs.len();
        for mut diff in std::mem::replace(&mut diffs, Vec::with_capacity(len)) {
            let Some(depth) = self.arena.get_depth(diff.idx) else {
                warn!("{:?} is not in arena. It could be a dangling container that was deleted before the shallow start version.", self.arena.idx_to_id(diff.idx));
                continue;
            };
            let this_depth = depth.get();
            while this_depth > last_depth {
                // Clear `to_revive` when we are going to process a new level
                // so that we can process the revival of the next level
                let to_create = std::mem::take(&mut to_revive_in_this_layer);
                to_revive_in_this_layer = std::mem::take(&mut to_revive_in_next_layer);
                for new in to_create {
                    let state = self.store.get_or_create_mut(new);
                    if state.is_state_empty() {
                        continue;
                    }

                    let external_diff = state.to_diff(&self.doc);
                    trigger_on_new_container(
                        &external_diff,
                        |cid| {
                            to_revive_in_this_layer.insert(cid);
                        },
                        &self.arena,
                    );

                    diffs.push(InternalContainerDiff {
                        idx: new,
                        bring_back: true,
                        is_container_deleted: false,
                        diff: external_diff.into(),
                        diff_mode: DiffMode::Checkout,
                    });
                }

                last_depth += 1;
            }

            let idx = diff.idx;
            let internal_diff = std::mem::take(&mut diff.diff);
            match &internal_diff {
                crate::event::DiffVariant::None => {
                    if is_recording {
                        let state = self.store.get_or_create_mut(diff.idx);
                        let extern_diff = state.to_diff(&self.doc);
                        trigger_on_new_container(
                            &extern_diff,
                            |cid| {
                                to_revive_in_next_layer.insert(cid);
                            },
                            &self.arena,
                        );
                        diff.diff = extern_diff.into();
                    }
                }
                crate::event::DiffVariant::Internal(_) => {
                    let cid = self.arena.idx_to_id(idx).unwrap();
                    info_span!("apply diff on", container_id = ?cid).in_scope(|| {
                        if self.in_txn {
                            self.changed_idx_in_txn.insert(idx);
                        }
                        let state = self.store.get_or_create_mut(idx);
                        if is_recording {
                            // process bring_back before apply
                            let external_diff =
                                if diff.bring_back || to_revive_in_this_layer.contains(&idx) {
                                    state.apply_diff(
                                        internal_diff.into_internal().unwrap(),
                                        DiffApplyContext {
                                            mode: diff.diff_mode,
                                            doc: &self.doc,
                                        },
                                    );
                                    state.to_diff(&self.doc)
                                } else {
                                    state.apply_diff_and_convert(
                                        internal_diff.into_internal().unwrap(),
                                        DiffApplyContext {
                                            mode: diff.diff_mode,
                                            doc: &self.doc,
                                        },
                                    )
                                };
                            trigger_on_new_container(
                                &external_diff,
                                |cid| {
                                    to_revive_in_next_layer.insert(cid);
                                },
                                &self.arena,
                            );
                            diff.diff = external_diff.into();
                        } else {
                            state.apply_diff(
                                internal_diff.into_internal().unwrap(),
                                DiffApplyContext {
                                    mode: diff.diff_mode,
                                    doc: &self.doc,
                                },
                            );
                        }
                    });
                }
                crate::event::DiffVariant::External(_) => unreachable!(),
            }

            to_revive_in_this_layer.remove(&idx);
            if !diff.diff.is_empty() {
                diffs.push(diff);
            }
        }

        // Revive the last several layers
        while !to_revive_in_this_layer.is_empty() || !to_revive_in_next_layer.is_empty() {
            let to_create = std::mem::take(&mut to_revive_in_this_layer);
            for new in to_create {
                let state = self.store.get_or_create_mut(new);
                if state.is_state_empty() {
                    continue;
                }

                let external_diff = state.to_diff(&self.doc);
                trigger_on_new_container(
                    &external_diff,
                    |cid| {
                        to_revive_in_next_layer.insert(cid);
                    },
                    &self.arena,
                );

                if !external_diff.is_empty() {
                    diffs.push(InternalContainerDiff {
                        idx: new,
                        bring_back: true,
                        is_container_deleted: false,
                        diff: external_diff.into(),
                        diff_mode: DiffMode::Checkout,
                    });
                }
            }

            to_revive_in_this_layer = std::mem::take(&mut to_revive_in_next_layer);
        }

        diff.diff = diffs.into();
        self.frontiers = diff.new_version.clone().into_owned();
        if self.is_recording() {
            self.record_diff(diff)
        }
    }

    pub fn apply_local_op(&mut self, raw_op: &RawOp, op: &Op) -> LoroResult<()> {
        // set parent first, `MapContainer` will only be created for TreeID that does not contain
        self.set_container_parent_by_raw_op(raw_op);
        let state = self.store.get_or_create_mut(op.container);
        if self.in_txn {
            self.changed_idx_in_txn.insert(op.container);
        }
        let ret = state.apply_local_op(raw_op, op)?;
        if !ret.deleted_containers.is_empty() {
            self.dead_containers_cache.clear_alive();
        }

        Ok(())
    }

    pub(crate) fn start_txn(&mut self, origin: InternalString, trigger: EventTriggerKind) {
        self.pre_txn(origin, trigger);
        self.in_txn = true;
    }

    pub(crate) fn abort_txn(&mut self) {
        self.in_txn = false;
    }

    pub fn iter_and_decode_all(&mut self) -> impl Iterator<Item = &mut State> {
        self.store.iter_and_decode_all()
    }

    pub(crate) fn iter_all_containers_mut(
        &mut self,
    ) -> impl Iterator<Item = (&ContainerIdx, &mut ContainerWrapper)> {
        self.store.iter_all_containers()
    }

    pub fn does_container_exist(&self, id: &ContainerID) -> bool {
        // TODO: we may need a better way to handle this in the future when we need to enable fully lazy loading on state
        self.arena.id_to_idx(id).is_some()
    }

    pub(crate) fn init_container(
        &mut self,
        cid: ContainerID,
        decode_ctx: StateSnapshotDecodeContext,
    ) -> LoroResult<()> {
        let idx = self.arena.register_container(&cid);
        let state = self.store.get_or_create_mut(idx);
        state.import_from_snapshot_ops(decode_ctx)
    }

    pub(crate) fn init_unknown_container(&mut self, cid: ContainerID) {
        let idx = self.arena.register_container(&cid);
        self.store.get_or_create_imm(idx);
    }

    pub(crate) fn commit_txn(&mut self, new_frontiers: Frontiers, diff: Option<InternalDocDiff>) {
        self.in_txn = false;
        self.frontiers = new_frontiers;
        if self.is_recording() {
            self.record_diff(diff.unwrap());
        }
    }

    #[inline]
    pub(super) fn get_container_mut(&mut self, idx: ContainerIdx) -> Option<&mut State> {
        self.store.get_container_mut(idx)
    }

    /// Ensure the container is created and will be encoded in the next `encode` call
    #[inline]
    pub(crate) fn ensure_container(&mut self, id: &ContainerID) {
        self.store.ensure_container(id);
    }

    /// Ensure all alive containers are created in DocState and will be encoded in the next `encode` call
    pub(crate) fn ensure_all_alive_containers(&mut self) -> FxHashSet<ContainerID> {
        // TODO: PERF This can be optimized because we shouldn't need to call get_value for
        // all the containers every time we export
        let ans = self.get_all_alive_containers();
        for id in ans.iter() {
            self.ensure_container(id);
        }

        ans
    }

    pub(crate) fn get_value_by_idx(&mut self, container_idx: ContainerIdx) -> LoroValue {
        self.store
            .get_value(container_idx)
            .unwrap_or_else(|| container_idx.get_type().default_value())
    }

    /// Set the state of the container with the given container idx.
    /// This is only used for decode.
    ///
    /// # Panic
    ///
    /// If the state is not empty.
    pub(super) fn init_with_states_and_version(
        &mut self,
        frontiers: Frontiers,
        oplog: &OpLog,
        unknown_containers: Vec<ContainerIdx>,
        need_to_register_parent: bool,
    ) {
        self.pre_txn(Default::default(), EventTriggerKind::Import);
        if need_to_register_parent {
            for state in self.store.iter_and_decode_all() {
                let idx = state.container_idx();
                let s = state;
                for child_id in s.get_child_containers() {
                    let child_idx = self.arena.register_container(&child_id);
                    self.arena.set_parent(child_idx, Some(idx));
                }
            }
        }

        if !unknown_containers.is_empty() {
            let mut diff_calc = DiffCalculator::new(false);
            let stack_vv;
            let vv = if oplog.frontiers() == &frontiers {
                oplog.vv()
            } else {
                stack_vv = oplog.dag().frontiers_to_vv(&frontiers);
                stack_vv.as_ref().unwrap()
            };

            let (unknown_diffs, _diff_mode) = diff_calc.calc_diff_internal(
                oplog,
                &Default::default(),
                &Default::default(),
                vv,
                &frontiers,
                Some(&|idx| !idx.is_unknown() && unknown_containers.contains(&idx)),
            );
            self.apply_diff(
                InternalDocDiff {
                    origin: Default::default(),
                    by: EventTriggerKind::Import,
                    diff: unknown_diffs.into(),
                    new_version: Cow::Owned(frontiers.clone()),
                },
                DiffMode::Checkout,
            )
        }

        if self.is_recording() {
            let diff: Vec<_> = self
                .store
                .iter_all_containers()
                .map(|(&idx, state)| InternalContainerDiff {
                    idx,
                    bring_back: false,
                    is_container_deleted: false,
                    diff: state
                        .get_state_mut(
                            idx,
                            ContainerCreationContext {
                                configure: &self.config,
                                peer: self.peer.load(Ordering::Relaxed),
                            },
                        )
                        .to_diff(&self.doc)
                        .into(),
                    diff_mode: DiffMode::Checkout,
                })
                .collect();

            self.record_diff(InternalDocDiff {
                origin: Default::default(),
                by: EventTriggerKind::Import,
                diff: diff.into(),
                new_version: Cow::Borrowed(&frontiers),
            });
        }

        self.frontiers = frontiers;
    }

    /// id can be a str, ContainerID, or ContainerIdRaw.
    /// if it's str it will use Root container, which will not be None
    pub fn get_text<I: Into<ContainerIdRaw>>(
        &mut self,
        id: I,
    ) -> Option<&mut richtext_state::RichtextState> {
        let idx = self.id_to_idx(id, ContainerType::Text);
        self.store
            .get_or_create_mut(idx)
            .as_richtext_state_mut()
            .map(|x| &mut **x)
    }

    /// id can be a str, ContainerID, or ContainerIdRaw.
    /// if it's str it will use Root container, which will not be None
    #[allow(unused)]
    pub(crate) fn get_tree<I: Into<ContainerIdRaw>>(&mut self, id: I) -> Option<&mut TreeState> {
        let idx = self.id_to_idx(id, ContainerType::Tree);
        self.store
            .get_or_create_mut(idx)
            .as_tree_state_mut()
            .map(|x| &mut **x)
    }

    fn id_to_idx<I: Into<ContainerIdRaw>>(&mut self, id: I, kind: ContainerType) -> ContainerIdx {
        let id: ContainerIdRaw = id.into();
        let cid;
        let idx = match id {
            ContainerIdRaw::Root { name } => {
                cid = crate::container::ContainerID::Root {
                    name,
                    container_type: kind,
                };
                Some(self.arena.register_container(&cid))
            }
            ContainerIdRaw::Normal { id: _ } => {
                cid = id.with_type(kind);
                self.arena.id_to_idx(&cid)
            }
        };

        idx.unwrap()
    }

    #[inline(always)]
    #[allow(unused)]
    pub(crate) fn with_state<F, R>(&mut self, idx: ContainerIdx, f: F) -> R
    where
        F: FnOnce(&State) -> R,
    {
        let depth = self.arena.get_depth(idx).unwrap().get() as usize;
        let state = self.store.get_or_create_imm(idx);
        f(state)
    }

    #[inline(always)]
    pub(crate) fn with_state_mut<F, R>(&mut self, idx: ContainerIdx, f: F) -> R
    where
        F: FnOnce(&mut State) -> R,
    {
        let state = self.store.get_or_create_mut(idx);
        f(state)
    }

    pub(super) fn is_in_txn(&self) -> bool {
        self.in_txn
    }

    pub fn can_import_snapshot(&self) -> bool {
        !self.in_txn && self.arena.can_import_snapshot() && self.store.can_import_snapshot()
    }

    pub fn get_value(&self) -> LoroValue {
        let roots = self.arena.root_containers();
        let ans: loro_common::LoroMapValue = roots
            .into_iter()
            .map(|idx| {
                let id = self.arena.idx_to_id(idx).unwrap();
                let ContainerID::Root {
                    name,
                    container_type: _,
                } = &id
                else {
                    unreachable!()
                };
                (name.to_string(), LoroValue::Container(id))
            })
            .collect();
        LoroValue::Map(ans)
    }

    pub fn get_deep_value(&mut self) -> LoroValue {
        let roots = self.arena.root_containers();
        let mut ans = FxHashMap::with_capacity_and_hasher(roots.len(), Default::default());
        for root_idx in roots {
            let id = self.arena.idx_to_id(root_idx).unwrap();
            match id {
                loro_common::ContainerID::Root { name, .. } => {
                    ans.insert(name.to_string(), self.get_container_deep_value(root_idx));
                }
                loro_common::ContainerID::Normal { .. } => {
                    unreachable!()
                }
            }
        }

        LoroValue::Map(ans.into())
    }

    pub fn get_deep_value_with_id(&mut self) -> LoroValue {
        let roots = self.arena.root_containers();
        let mut ans = FxHashMap::with_capacity_and_hasher(roots.len(), Default::default());
        for root_idx in roots {
            let id = self.arena.idx_to_id(root_idx).unwrap();
            match id.clone() {
                loro_common::ContainerID::Root { name, .. } => {
                    ans.insert(
                        name.to_string(),
                        self.get_container_deep_value_with_id(root_idx, Some(id)),
                    );
                }
                loro_common::ContainerID::Normal { .. } => {
                    unreachable!()
                }
            }
        }

        LoroValue::Map(ans.into())
    }

    pub fn get_all_container_value_flat(&mut self) -> LoroValue {
        let mut map = FxHashMap::default();
        self.store.iter_and_decode_all().for_each(|c| {
            let value = c.get_value();
            let cid = self.arena.idx_to_id(c.container_idx()).unwrap().to_string();
            map.insert(cid, value);
        });

        LoroValue::Map(map.into())
    }

    pub(crate) fn get_container_deep_value_with_id(
        &mut self,
        container: ContainerIdx,
        id: Option<ContainerID>,
    ) -> LoroValue {
        let id = id.unwrap_or_else(|| self.arena.idx_to_id(container).unwrap());
        let Some(state) = self.store.get_container_mut(container) else {
            return container.get_type().default_value();
        };
        let value = state.get_value();
        let cid_str = LoroValue::String(format!("idx:{}, id:{}", container.to_index(), id).into());
        match value {
            LoroValue::Container(_) => unreachable!(),
            LoroValue::List(mut list) => {
                if container.get_type() == ContainerType::Tree {
                    get_meta_value(list.make_mut(), self);
                } else {
                    if list.iter().all(|x| !x.is_container()) {
                        return LoroValue::Map(
                            (fx_map!(
                                "cid".into() => cid_str,
                                "value".into() =>  LoroValue::List(list)
                            ))
                            .into(),
                        );
                    }

                    let list_mut = list.make_mut();
                    for item in list_mut.iter_mut() {
                        if item.is_container() {
                            let container = item.as_container().unwrap();
                            let container_idx = self.arena.register_container(container);
                            let value = self.get_container_deep_value_with_id(
                                container_idx,
                                Some(container.clone()),
                            );
                            *item = value;
                        }
                    }
                }
                LoroValue::Map(
                    (fx_map!(
                        "cid".into() => cid_str,
                        "value".into() => LoroValue::List(list)
                    ))
                    .into(),
                )
            }
            LoroValue::Map(mut map) => {
                let map_mut = map.make_mut();
                for (_key, value) in map_mut.iter_mut() {
                    if value.is_container() {
                        let container = value.as_container().unwrap();
                        let container_idx = self.arena.register_container(container);
                        let new_value = self.get_container_deep_value_with_id(
                            container_idx,
                            Some(container.clone()),
                        );
                        *value = new_value;
                    }
                }

                LoroValue::Map(
                    (fx_map!(
                        "cid".into() => cid_str,
                        "value".into() => LoroValue::Map(map)
                    ))
                    .into(),
                )
            }
            _ => LoroValue::Map(
                (fx_map!(
                    "cid".into() => cid_str,
                    "value".into() => value
                ))
                .into(),
            ),
        }
    }

    pub fn get_container_deep_value(&mut self, container: ContainerIdx) -> LoroValue {
        let Some(value) = self.store.get_value(container) else {
            return container.get_type().default_value();
        };
        match value {
            LoroValue::Container(_) => unreachable!(),
            LoroValue::List(mut list) => {
                if container.get_type() == ContainerType::Tree {
                    // Each tree node has an associated map container to represent
                    // the metadata of this node. When the user get the deep value,
                    // we need to add a field named `meta` to the tree node,
                    // whose value is deep value of map container.
                    get_meta_value(list.make_mut(), self);
                } else {
                    if list.iter().all(|x| !x.is_container()) {
                        return LoroValue::List(list);
                    }

                    let list_mut = list.make_mut();
                    for item in list_mut.iter_mut() {
                        if item.is_container() {
                            let container = item.as_container().unwrap();
                            let container_idx = self.arena.register_container(container);
                            let value = self.get_container_deep_value(container_idx);
                            *item = value;
                        }
                    }
                }
                LoroValue::List(list)
            }
            LoroValue::Map(mut map) => {
                if map.iter().all(|x| !x.1.is_container()) {
                    return LoroValue::Map(map);
                }

                let map_mut = map.make_mut();
                for (_key, value) in map_mut.iter_mut() {
                    if value.is_container() {
                        let container = value.as_container().unwrap();
                        let container_idx = self.arena.register_container(container);
                        let new_value = self.get_container_deep_value(container_idx);
                        *value = new_value;
                    }
                }
                LoroValue::Map(map)
            }
            _ => value,
        }
    }

    pub(crate) fn get_all_alive_containers(&mut self) -> FxHashSet<ContainerID> {
        let mut ans = FxHashSet::default();
        let mut to_visit = self
            .arena
            .root_containers()
            .iter()
            .map(|x| self.arena.get_container_id(*x).unwrap())
            .collect_vec();

        while let Some(id) = to_visit.pop() {
            self.get_alive_children_of(&id, &mut to_visit);
            ans.insert(id);
        }

        ans
    }

    pub(crate) fn get_alive_children_of(&mut self, id: &ContainerID, ans: &mut Vec<ContainerID>) {
        let idx = self.arena.register_container(id);
        let Some(value) = self.store.get_value(idx) else {
            return;
        };

        match value {
            LoroValue::Container(_) => unreachable!(),
            LoroValue::List(list) => {
                if idx.get_type() == ContainerType::Tree {
                    // Each tree node has an associated map container to represent
                    // the metadata of this node. When the user get the deep value,
                    // we need to add a field named `meta` to the tree node,
                    // whose value is deep value of map container.
                    let mut list = list.unwrap();
                    while let Some(node) = list.pop() {
                        let map = node.as_map().unwrap();
                        let meta = map.get("meta").unwrap();
                        let id = meta.as_container().unwrap();
                        ans.push(id.clone());
                        let children = map.get("children").unwrap();
                        let children = children.as_list().unwrap();
                        for child in children.iter() {
                            list.push(child.clone());
                        }
                    }
                } else {
                    for item in list.iter() {
                        if let LoroValue::Container(id) = item {
                            ans.push(id.clone());
                        }
                    }
                }
            }
            LoroValue::Map(map) => {
                for (_key, value) in map.iter() {
                    if let LoroValue::Container(id) = value {
                        ans.push(id.clone());
                    }
                }
            }
            _ => {}
        }
    }

    // Because we need to calculate path based on [DocState], so we cannot extract
    // the event recorder to a separate module.
    fn diffs_to_event(&mut self, diffs: Vec<InternalDocDiff<'_>>, from: Frontiers) -> DocDiff {
        if diffs.is_empty() {
            panic!("diffs is empty");
        }

        let triggered_by = diffs[0].by;
        debug_assert!(diffs.iter().all(|x| x.by == triggered_by));
        let mut containers = FxHashMap::default();
        let to = (*diffs.last().unwrap().new_version).to_owned();
        let origin = diffs[0].origin.clone();
        for diff in diffs {
            #[allow(clippy::unnecessary_to_owned)]
            for container_diff in diff.diff.into_owned() {
                if container_diff.is_container_deleted {
                    // omit event form deleted container
                    continue;
                }
                let Some((last_container_diff, _)) = containers.get_mut(&container_diff.idx) else {
                    if let Some(path) = self.get_path(container_diff.idx) {
                        containers.insert(container_diff.idx, (container_diff.diff, path));
                    } else {
                        // if we cannot find the path to the container, the container must be overwritten afterwards.
                        // So we can ignore the diff from it.
                        tracing::warn!(
                            "⚠️ WARNING: ignore event because cannot find its path {:#?} container id:{}",
                            &container_diff,
                            self.arena.idx_to_id(container_diff.idx).unwrap()
                        );
                    }

                    continue;
                };
                // TODO: PERF avoid this clone
                *last_container_diff = last_container_diff
                    .clone()
                    .compose(container_diff.diff)
                    .unwrap();
            }
        }
        let mut diff: Vec<_> = containers
            .into_iter()
            .map(|(container, (diff, path))| {
                let idx = container;
                let id = self.arena.get_container_id(idx).unwrap();
                let is_unknown = id.is_unknown();

                ContainerDiff {
                    id,
                    idx,
                    diff: diff.into_external().unwrap(),
                    is_unknown,
                    path,
                }
            })
            .collect();

        // Sort by path length, so caller can apply the diff from the root to the leaf.
        // Otherwise, the caller may use a wrong path to apply the diff.

        diff.sort_by_key(|x| {
            (
                x.path.len(),
                match &x.id {
                    ContainerID::Root { .. } => 0,
                    ContainerID::Normal { counter, .. } => *counter + 1,
                },
            )
        });
        DocDiff {
            from,
            to,
            origin,
            by: triggered_by,
            diff,
        }
    }

    pub(crate) fn get_reachable(&mut self, id: &ContainerID) -> bool {
        if matches!(id, ContainerID::Root { .. }) {
            return true;
        }

        let Some(mut idx) = self.arena.id_to_idx(id) else {
            return false;
        };
        loop {
            let id = self.arena.idx_to_id(idx).unwrap();
            if let Some(parent_idx) = self.arena.get_parent(idx) {
                let Some(parent_state) = self.store.get_container_mut(parent_idx) else {
                    return false;
                };
                if !parent_state.contains_child(&id) {
                    return false;
                }
                idx = parent_idx;
            } else {
                if id.is_root() {
                    return true;
                }

                return false;
            }
        }
    }

    // the container may be override, so it may return None
    pub(super) fn get_path(&mut self, idx: ContainerIdx) -> Option<Vec<(ContainerID, Index)>> {
        let mut ans = Vec::new();
        let mut idx = idx;
        loop {
            let id = self.arena.idx_to_id(idx).unwrap();
            if let Some(parent_idx) = self.arena.get_parent(idx) {
                let parent_state = self.store.get_container_mut(parent_idx)?;
                let Some(prop) = parent_state.get_child_index(&id) else {
                    tracing::info!("Missing in parent children");
                    return None;
                };
                ans.push((id, prop));
                idx = parent_idx;
            } else {
                // this container may be deleted
                let Ok(prop) = id.clone().into_root() else {
                    let id = format!("{}", &id);
                    tracing::info!(?id, "Missing parent - container is deleted");
                    return None;
                };
                ans.push((id, Index::Key(prop.0)));
                break;
            }
        }

        ans.reverse();

        Some(ans)
    }

    pub(crate) fn check_before_decode_snapshot(&self) -> LoroResult<()> {
        if self.is_in_txn() {
            return Err(LoroError::DecodeError(
                "State is in txn".to_string().into_boxed_str(),
            ));
        }

        if !self.can_import_snapshot() {
            return Err(LoroError::DecodeError(
                "State is not empty, cannot import snapshot directly"
                    .to_string()
                    .into_boxed_str(),
            ));
        }

        Ok(())
    }

    /// Check whether two [DocState]s are the same. Panic if not.
    ///
    /// Compared to check equality on `get_deep_value`, this function also checks the equality on richtext
    /// styles and states that are not reachable from the root.
    ///
    /// This is only used for test.
    pub(crate) fn check_is_the_same(&mut self, other: &mut Self) {
        fn get_entries_for_state(
            arena: &SharedArena,
            state: &mut State,
        ) -> Option<(ContainerID, (ContainerIdx, LoroValue))> {
            if state.is_state_empty() {
                return None;
            }

            let id = arena.idx_to_id(state.container_idx()).unwrap();
            let value = match state {
                State::RichtextState(s) => s.get_richtext_value(),
                _ => state.get_value(),
            };
            if match &value {
                LoroValue::List(l) => l.is_empty(),
                LoroValue::Map(m) => m.is_empty(),
                _ => false,
            } {
                return None;
            }
            #[cfg(feature = "counter")]
            if id.container_type() == ContainerType::Counter {
                if let LoroValue::Double(c) = value {
                    if c.abs() < f64::EPSILON {
                        return None;
                    }
                }
            }

            Some((id, (state.container_idx(), value)))
        }

        let self_id_to_states: FxHashMap<ContainerID, (ContainerIdx, LoroValue)> = self
            .store
            .iter_and_decode_all()
            .filter_map(|state: &mut State| {
                let arena = &self.arena;
                get_entries_for_state(arena, state)
            })
            .collect();
        let mut other_id_to_states: FxHashMap<ContainerID, (ContainerIdx, LoroValue)> = other
            .store
            .iter_and_decode_all()
            .filter_map(|state: &mut State| {
                let arena = &other.arena;
                get_entries_for_state(arena, state)
            })
            .collect();
        for (id, (idx, this_value)) in self_id_to_states {
            let (_, other_value) = match other_id_to_states.remove(&id) {
                Some(x) => x,
                None => {
                    panic!(
                        "id: {:?}, path: {:?} is missing, value={:?}",
                        id,
                        self.get_path(idx),
                        &this_value
                    );
                }
            };

            pretty_assertions::assert_eq!(
                this_value,
                other_value,
                "[self!=other] id: {:?}, path: {:?}",
                id,
                self.get_path(idx)
            );
        }

        if !other_id_to_states.is_empty() {
            panic!("other has more states {:#?}", &other_id_to_states);
        }
    }

    pub fn log_estimated_size(&self) {
        let state_entries_size =
            self.store.len() * (std::mem::size_of::<State>() + std::mem::size_of::<ContainerIdx>());
        let mut state_size_sum = 0;
        state_size_sum += self.store.estimate_size();

        eprintln!(
            "ContainerNum: {}\nEstimated state size: \nEntries: {} \nSum: {}",
            self.store.len(),
            state_entries_size,
            state_size_sum
        );
    }

    pub fn create_state(&self, idx: ContainerIdx) -> State {
        let config = &self.config;
        let peer = self.peer.load(std::sync::atomic::Ordering::Relaxed);
        create_state_(idx, config, peer)
    }

    pub fn create_unknown_state(&self, idx: ContainerIdx) -> State {
        State::UnknownState(UnknownState::new(idx))
    }

    pub fn get_relative_position(&mut self, pos: &Cursor, use_event_index: bool) -> Option<usize> {
        let idx = self.arena.register_container(&pos.container);
        let state = self.store.get_container_mut(idx)?;
        if let Some(id) = pos.id {
            match state {
                State::ListState(s) => s.get_index_of_id(id),
                State::RichtextState(s) => s.get_text_index_of_id(id, use_event_index),
                State::MovableListState(s) => s.get_index_of_id(id),
                State::MapState(_) | State::TreeState(_) | State::UnknownState(_) => unreachable!(),
                #[cfg(feature = "counter")]
                State::CounterState(_) => unreachable!(),
            }
        } else {
            if matches!(pos.side, crate::cursor::Side::Left) {
                return Some(0);
            }

            match state {
                State::ListState(s) => Some(s.len()),
                State::RichtextState(s) => Some(if use_event_index {
                    s.len_event()
                } else {
                    s.len_unicode()
                }),
                State::MovableListState(s) => Some(s.len()),
                State::MapState(_) | State::TreeState(_) | State::UnknownState(_) => unreachable!(),
                #[cfg(feature = "counter")]
                State::CounterState(_) => unreachable!(),
            }
        }
    }

    pub fn get_value_by_path(&mut self, path: &[Index]) -> Option<LoroValue> {
        if path.is_empty() {
            return None;
        }

        enum CurContainer {
            Container(ContainerIdx),
            TreeNode {
                tree: ContainerIdx,
                node: Option<TreeID>,
            },
        }

        let mut state_idx = {
            let root_index = path[0].as_key()?;
            CurContainer::Container(self.arena.get_root_container_idx_by_key(root_index)?)
        };

        if path.len() == 1 {
            if let CurContainer::Container(c) = state_idx {
                let cid = self.arena.idx_to_id(c)?;
                return Some(LoroValue::Container(cid));
            }
        }

        let mut i = 1;
        while i < path.len() - 1 {
            let index = &path[i];
            match state_idx {
                CurContainer::Container(idx) => {
                    let parent_state = self.store.get_container_mut(idx)?;
                    match parent_state {
                        State::ListState(l) => {
                            let Some(LoroValue::Container(c)) = l.get(*index.as_seq()?) else {
                                return None;
                            };
                            state_idx = CurContainer::Container(self.arena.register_container(c));
                        }
                        State::MovableListState(l) => {
                            let Some(LoroValue::Container(c)) =
                                l.get(*index.as_seq()?, IndexType::ForUser)
                            else {
                                return None;
                            };
                            state_idx = CurContainer::Container(self.arena.register_container(c));
                        }
                        State::MapState(m) => {
                            let Some(LoroValue::Container(c)) = m.get(index.as_key()?) else {
                                return None;
                            };
                            state_idx = CurContainer::Container(self.arena.register_container(c));
                        }
                        State::RichtextState(_) => return None,
                        State::TreeState(_) => {
                            state_idx = CurContainer::TreeNode {
                                tree: idx,
                                node: None,
                            };
                            continue;
                        }
                        #[cfg(feature = "counter")]
                        State::CounterState(_) => return None,
                        State::UnknownState(_) => unreachable!(),
                    }
                }
                CurContainer::TreeNode { tree, node } => match index {
                    Index::Key(internal_string) => {
                        let node = node?;
                        let idx = self
                            .arena
                            .register_container(&node.associated_meta_container());
                        let map = self.store.get_container(idx)?;
                        let Some(LoroValue::Container(c)) =
                            map.as_map_state().unwrap().get(internal_string)
                        else {
                            return None;
                        };

                        state_idx = CurContainer::Container(self.arena.register_container(c));
                    }
                    Index::Seq(i) => {
                        let tree_state =
                            self.store.get_container_mut(tree)?.as_tree_state().unwrap();
                        let parent: TreeParentId = if let Some(node) = node {
                            node.into()
                        } else {
                            TreeParentId::Root
                        };
                        let child = tree_state.get_children(&parent)?.nth(*i)?;
                        state_idx = CurContainer::TreeNode {
                            tree,
                            node: Some(child),
                        };
                    }
                    Index::Node(tree_id) => {
                        let tree_state =
                            self.store.get_container_mut(tree)?.as_tree_state().unwrap();
                        if tree_state.parent(tree_id).is_some() {
                            state_idx = CurContainer::TreeNode {
                                tree,
                                node: Some(*tree_id),
                            }
                        } else {
                            return None;
                        }
                    }
                },
            }
            i += 1;
        }

        let parent_idx = match state_idx {
            CurContainer::Container(container_idx) => container_idx,
            CurContainer::TreeNode { tree, node } => {
                if let Some(node) = node {
                    self.arena
                        .register_container(&node.associated_meta_container())
                } else {
                    tree
                }
            }
        };

        let parent_state = self.store.get_container_mut(parent_idx)?;
        let index = path.last().unwrap();
        let value: LoroValue = match parent_state {
            State::ListState(l) => l.get(*index.as_seq()?).cloned()?,
            State::MovableListState(l) => l.get(*index.as_seq()?, IndexType::ForUser).cloned()?,
            State::MapState(m) => m.get(index.as_key()?).cloned()?,
            State::RichtextState(s) => {
                let s = s.to_string_mut();
                s.chars()
                    .nth(*index.as_seq()?)
                    .map(|c| c.to_string().into())?
            }
            State::TreeState(_) => {
                let id = index.as_node()?;
                let cid = id.associated_meta_container();
                cid.into()
            }
            #[cfg(feature = "counter")]
            State::CounterState(_) => unreachable!(),
            State::UnknownState(_) => unreachable!(),
        };

        Some(value)
    }

    pub(crate) fn shallow_root_store(&self) -> Option<&Arc<GcStore>> {
        self.store.shallow_root_store()
    }
}

fn create_state_(idx: ContainerIdx, config: &Configure, peer: u64) -> State {
    match idx.get_type() {
        ContainerType::Map => State::MapState(Box::new(MapState::new(idx))),
        ContainerType::List => State::ListState(Box::new(ListState::new(idx))),
        ContainerType::Text => State::RichtextState(Box::new(RichtextState::new(
            idx,
            config.text_style_config.clone(),
        ))),
        ContainerType::Tree => State::TreeState(Box::new(TreeState::new(idx, peer))),
        ContainerType::MovableList => State::MovableListState(Box::new(MovableListState::new(idx))),
        #[cfg(feature = "counter")]
        ContainerType::Counter => {
            State::CounterState(Box::new(counter_state::CounterState::new(idx)))
        }
        ContainerType::Unknown(_) => State::UnknownState(UnknownState::new(idx)),
    }
}

fn trigger_on_new_container(
    state_diff: &Diff,
    mut listener: impl FnMut(ContainerIdx),
    arena: &SharedArena,
) {
    match state_diff {
        Diff::List(list) => {
            for delta in list.iter() {
                if let DeltaItem::Replace {
                    value,
                    attr,
                    delete: _,
                } = delta
                {
                    if attr.from_move {
                        continue;
                    }

                    for v in value.iter() {
                        if let ValueOrHandler::Handler(h) = v {
                            let idx = h.container_idx();
                            listener(idx);
                        }
                    }
                }
            }
        }
        Diff::Map(map) => {
            for (_, v) in map.updated.iter() {
                if let Some(ValueOrHandler::Handler(h)) = &v.value {
                    let idx = h.container_idx();
                    listener(idx);
                }
            }
        }
        Diff::Tree(tree) => {
            for item in tree.iter() {
                if matches!(item.action, TreeExternalDiff::Create { .. }) {
                    let id = item.target.associated_meta_container();
                    listener(arena.id_to_idx(&id).unwrap());
                }
            }
        }
        _ => {}
    };
}

#[derive(Default, Clone)]
struct EventRecorder {
    recording_diff: bool,
    // A batch of diffs will be converted to a event when
    // they cannot be merged with the next diff.
    diffs: Vec<InternalDocDiff<'static>>,
    events: Vec<DocDiff>,
    diff_start_version: Option<Frontiers>,
}

impl EventRecorder {
    #[allow(unused)]
    pub fn new() -> Self {
        Self::default()
    }
}

#[test]
fn test_size() {
    println!("Size of State = {}", std::mem::size_of::<State>());
    println!("Size of MapState = {}", std::mem::size_of::<MapState>());
    println!("Size of ListState = {}", std::mem::size_of::<ListState>());
    println!(
        "Size of TextState = {}",
        std::mem::size_of::<RichtextState>()
    );
    println!("Size of TreeState = {}", std::mem::size_of::<TreeState>());
}

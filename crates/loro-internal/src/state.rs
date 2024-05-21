use std::{
    borrow::Cow,
    sync::{atomic::AtomicU8, Arc, Mutex, RwLock, Weak},
};

use enum_as_inner::EnumAsInner;
use enum_dispatch::enum_dispatch;
use fxhash::{FxHashMap, FxHashSet};
use loro_common::{ContainerID, LoroError, LoroResult};
use loro_delta::DeltaItem;
use tracing::{info, instrument};

use crate::{
    configure::{Configure, DefaultRandom, SecureRandomGenerator},
    container::{idx::ContainerIdx, richtext::config::StyleConfigMap, ContainerIdRaw},
    cursor::Cursor,
    diff_calc::DiffCalculator,
    encoding::{StateSnapshotDecodeContext, StateSnapshotEncoder},
    event::{Diff, EventTriggerKind, Index, InternalContainerDiff, InternalDiff},
    fx_map,
    handler::ValueOrHandler,
    id::PeerID,
    op::{Op, RawOp},
    txn::Transaction,
    version::Frontiers,
    ContainerDiff, ContainerType, DocDiff, InternalString, LoroValue, OpLog,
};

#[cfg(feature = "counter")]
mod counter_state;
mod list_state;
mod map_state;
mod movable_list_state;
mod richtext_state;
mod tree_state;
mod unknown_state;

pub(crate) use self::movable_list_state::{IndexType, MovableListState};
pub(crate) use list_state::ListState;
pub(crate) use map_state::MapState;
pub(crate) use richtext_state::RichtextState;
pub(crate) use tree_state::{get_meta_value, FractionalIndexGenResult, TreeParentId, TreeState};

use self::unknown_state::UnknownState;

use super::{arena::SharedArena, event::InternalDocDiff};

macro_rules! get_or_create {
    ($doc_state: ident, $idx: expr) => {{
        if !$doc_state.states.contains_key(&$idx) {
            let state = $doc_state.create_state($idx);
            $doc_state.states.insert($idx, state);
        }

        $doc_state.states.get_mut(&$idx).unwrap()
    }};
}

#[derive(Clone)]
pub struct DocState {
    pub(super) peer: PeerID,

    pub(super) frontiers: Frontiers,
    pub(super) states: FxHashMap<ContainerIdx, State>,
    pub(super) arena: SharedArena,
    pub(crate) config: Configure,
    // resolve event stuff
    weak_state: Weak<Mutex<DocState>>,
    global_txn: Weak<Mutex<Option<Transaction>>>,
    // txn related stuff
    in_txn: bool,
    changed_idx_in_txn: FxHashSet<ContainerIdx>,

    // diff related stuff
    event_recorder: EventRecorder,
}

impl std::fmt::Debug for DocState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DocState")
            .field("peer", &self.peer)
            .finish()
    }
}

#[enum_dispatch]
pub(crate) trait ContainerState: Clone {
    fn container_idx(&self) -> ContainerIdx;
    fn estimate_size(&self) -> usize;

    fn is_state_empty(&self) -> bool;

    #[must_use]
    fn apply_diff_and_convert(
        &mut self,
        diff: InternalDiff,
        arena: &SharedArena,
        txn: &Weak<Mutex<Option<Transaction>>>,
        state: &Weak<Mutex<DocState>>,
    ) -> Diff;

    fn apply_diff(
        &mut self,
        diff: InternalDiff,
        arena: &SharedArena,
        txn: &Weak<Mutex<Option<Transaction>>>,
        state: &Weak<Mutex<DocState>>,
    );

    fn apply_local_op(&mut self, raw_op: &RawOp, op: &Op) -> LoroResult<()>;
    /// Convert a state to a diff, such that an empty state will be transformed into the same as this state when it's applied.
    fn to_diff(
        &mut self,
        arena: &SharedArena,
        txn: &Weak<Mutex<Option<Transaction>>>,
        state: &Weak<Mutex<DocState>>,
    ) -> Diff;

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
    fn import_from_snapshot_ops(&mut self, ctx: StateSnapshotDecodeContext);
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

    fn apply_diff_and_convert(
        &mut self,
        diff: InternalDiff,
        arena: &SharedArena,
        txn: &Weak<Mutex<Option<Transaction>>>,
        state: &Weak<Mutex<DocState>>,
    ) -> Diff {
        self.as_mut()
            .apply_diff_and_convert(diff, arena, txn, state)
    }

    fn apply_diff(
        &mut self,
        diff: InternalDiff,
        arena: &SharedArena,
        txn: &Weak<Mutex<Option<Transaction>>>,
        state: &Weak<Mutex<DocState>>,
    ) {
        self.as_mut().apply_diff(diff, arena, txn, state)
    }

    fn apply_local_op(&mut self, raw_op: &RawOp, op: &Op) -> LoroResult<()> {
        self.as_mut().apply_local_op(raw_op, op)
    }

    #[doc = r" Convert a state to a diff, such that an empty state will be transformed into the same as this state when it's applied."]
    fn to_diff(
        &mut self,
        arena: &SharedArena,
        txn: &Weak<Mutex<Option<Transaction>>>,
        state: &Weak<Mutex<DocState>>,
    ) -> Diff {
        self.as_mut().to_diff(arena, txn, state)
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
    fn import_from_snapshot_ops(&mut self, ctx: StateSnapshotDecodeContext) {
        self.as_mut().import_from_snapshot_ops(ctx)
    }
}

#[allow(clippy::enum_variant_names)]
#[enum_dispatch(ContainerState)]
#[derive(EnumAsInner, Clone, Debug)]
pub enum State {
    ListState(Box<ListState>),
    MovableListState(Box<MovableListState>),
    MapState(Box<MapState>),
    RichtextState(Box<RichtextState>),
    TreeState(Box<TreeState>),
    #[cfg(feature = "counter")]
    CounterState(Box<counter_state::CounterState>),
    UnknownState(Box<UnknownState>),
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

    pub fn new_tree(idx: ContainerIdx, peer: PeerID, jitter: Arc<AtomicU8>) -> Self {
        Self::TreeState(Box::new(TreeState::new(idx, peer, jitter)))
    }

    pub fn new_unknown(idx: ContainerIdx) -> Self {
        Self::UnknownState(Box::new(UnknownState::new(idx)))
    }
}

impl DocState {
    #[inline]
    pub fn new_arc(
        arena: SharedArena,
        global_txn: Weak<Mutex<Option<Transaction>>>,
        config: Configure,
    ) -> Arc<Mutex<Self>> {
        let peer = DefaultRandom.next_u64();
        // TODO: maybe we should switch to certain version in oplog?
        Arc::new_cyclic(|weak| {
            Mutex::new(Self {
                peer,
                arena,
                frontiers: Frontiers::default(),
                states: FxHashMap::default(),
                weak_state: weak.clone(),
                config,
                global_txn,
                in_txn: false,
                changed_idx_in_txn: FxHashSet::default(),
                event_recorder: Default::default(),
            })
        })
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
        self.peer = DefaultRandom.next_u64();
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
        self.peer = peer;
    }

    pub fn peer_id(&self) -> PeerID {
        self.peer
    }

    /// It's expected that diff only contains [`InternalDiff`]
    ///
    #[instrument(skip_all)]
    pub(crate) fn apply_diff(&mut self, mut diff: InternalDocDiff<'static>) {
        if self.in_txn {
            panic!("apply_diff should not be called in a transaction");
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
        diffs.sort_by_cached_key(|diff| self.arena.get_depth(diff.idx).unwrap());

        let mut to_revive_in_next_layer: FxHashSet<ContainerIdx> = FxHashSet::default();
        let mut to_revive_in_this_layer: FxHashSet<ContainerIdx> = FxHashSet::default();
        let mut last_depth = 0;
        let len = diffs.len();
        for mut diff in std::mem::replace(&mut diffs, Vec::with_capacity(len)) {
            let this_depth = self.arena.get_depth(diff.idx).unwrap().get();
            while this_depth > last_depth {
                // Clear `to_revive` when we are going to process a new level
                // so that we can process the revival of the next level
                let to_create = std::mem::take(&mut to_revive_in_this_layer);
                to_revive_in_this_layer = std::mem::take(&mut to_revive_in_next_layer);
                for new in to_create {
                    let state = {
                        if !self.states.contains_key(&new) {
                            continue;
                        }
                        self.states.get_mut(&new).unwrap()
                    };

                    if state.is_state_empty() {
                        continue;
                    }

                    let external_diff =
                        state.to_diff(&self.arena, &self.global_txn, &self.weak_state);
                    trigger_on_new_container(&external_diff, |cid| {
                        to_revive_in_this_layer.insert(cid);
                    });

                    diffs.push(InternalContainerDiff {
                        idx: new,
                        bring_back: true,
                        is_container_deleted: false,
                        diff: external_diff.into(),
                    });
                }

                last_depth += 1;
            }

            let idx = diff.idx;
            let internal_diff = std::mem::take(&mut diff.diff);
            match &internal_diff {
                crate::event::DiffVariant::None => {
                    if is_recording {
                        let state = get_or_create!(self, diff.idx);
                        let extern_diff =
                            state.to_diff(&self.arena, &self.global_txn, &self.weak_state);
                        trigger_on_new_container(&extern_diff, |cid| {
                            to_revive_in_next_layer.insert(cid);
                        });
                        diff.diff = extern_diff.into();
                    }
                }
                crate::event::DiffVariant::Internal(_) => {
                    if self.in_txn {
                        self.changed_idx_in_txn.insert(idx);
                    }
                    let state = get_or_create!(self, idx);
                    if is_recording {
                        // process bring_back before apply
                        let external_diff =
                            if diff.bring_back || to_revive_in_this_layer.contains(&idx) {
                                state.apply_diff(
                                    internal_diff.into_internal().unwrap(),
                                    &self.arena,
                                    &self.global_txn,
                                    &self.weak_state,
                                );
                                state.to_diff(&self.arena, &self.global_txn, &self.weak_state)
                            } else {
                                state.apply_diff_and_convert(
                                    internal_diff.into_internal().unwrap(),
                                    &self.arena,
                                    &self.global_txn,
                                    &self.weak_state,
                                )
                            };
                        trigger_on_new_container(&external_diff, |cid| {
                            to_revive_in_next_layer.insert(cid);
                        });
                        diff.diff = external_diff.into();
                    } else {
                        state.apply_diff(
                            internal_diff.into_internal().unwrap(),
                            &self.arena,
                            &self.global_txn,
                            &self.weak_state,
                        );
                    }
                }
                crate::event::DiffVariant::External(_) => unreachable!(),
            }

            to_revive_in_this_layer.remove(&idx);
            diffs.push(diff);
        }

        // Revive the last several layers
        while !to_revive_in_this_layer.is_empty() || !to_revive_in_next_layer.is_empty() {
            let to_create = std::mem::take(&mut to_revive_in_this_layer);
            for new in to_create {
                let state = {
                    if !self.states.contains_key(&new) {
                        continue;
                    }
                    self.states.get_mut(&new).unwrap()
                };

                if state.is_state_empty() {
                    continue;
                }

                let external_diff = state.to_diff(&self.arena, &self.global_txn, &self.weak_state);
                trigger_on_new_container(&external_diff, |cid| {
                    to_revive_in_next_layer.insert(cid);
                });

                diffs.push(InternalContainerDiff {
                    idx: new,
                    bring_back: true,
                    is_container_deleted: false,
                    diff: external_diff.into(),
                });
            }

            to_revive_in_this_layer = std::mem::take(&mut to_revive_in_next_layer);
        }

        diff.diff = diffs.into();
        (*diff.new_version).clone_into(&mut self.frontiers);
        if self.is_recording() {
            self.record_diff(diff)
        }
    }

    pub fn apply_local_op(&mut self, raw_op: &RawOp, op: &Op) -> LoroResult<()> {
        // set parent first, `MapContainer` will only be created for TreeID that does not contain
        self.set_container_parent_by_raw_op(raw_op);
        let state = get_or_create!(self, op.container);
        if self.in_txn {
            self.changed_idx_in_txn.insert(op.container);
        }
        state.apply_local_op(raw_op, op)
    }

    pub(crate) fn start_txn(&mut self, origin: InternalString, trigger: EventTriggerKind) {
        self.pre_txn(origin, trigger);
        self.in_txn = true;
    }

    pub(crate) fn abort_txn(&mut self) {
        self.in_txn = false;
    }

    pub fn iter(&self) -> impl Iterator<Item = &State> {
        self.states.values()
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut State> {
        self.states.values_mut()
    }

    pub(crate) fn init_container(
        &mut self,
        cid: ContainerID,
        decode_ctx: StateSnapshotDecodeContext,
    ) {
        let idx = self.arena.register_container(&cid);
        let state = get_or_create!(self, idx);
        state.import_from_snapshot_ops(decode_ctx);
    }

    pub(crate) fn init_unknown_container(&mut self, cid: ContainerID) {
        let idx = self.arena.register_container(&cid);
        get_or_create!(self, idx);
    }

    pub(crate) fn commit_txn(&mut self, new_frontiers: Frontiers, diff: Option<InternalDocDiff>) {
        self.in_txn = false;
        self.frontiers = new_frontiers;
        if self.is_recording() {
            self.record_diff(diff.unwrap());
        }
    }

    #[inline]
    #[allow(unused)]
    pub(super) fn get_state_mut(&mut self, idx: ContainerIdx) -> Option<&mut State> {
        self.states.get_mut(&idx)
    }

    #[inline]
    #[allow(unused)]
    pub(super) fn get_state(&self, idx: ContainerIdx) -> Option<&State> {
        self.states.get(&idx)
    }

    pub(crate) fn get_value_by_idx(&mut self, container_idx: ContainerIdx) -> LoroValue {
        self.states
            .get_mut(&container_idx)
            .map(|x| x.get_value())
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
        states: FxHashMap<ContainerIdx, State>,
        frontiers: Frontiers,
        oplog: &OpLog,
        unknown_containers: Vec<ContainerIdx>,
    ) {
        assert!(self.states.is_empty(), "overriding states");
        self.pre_txn(Default::default(), EventTriggerKind::Import);
        self.states = states;
        for (idx, state) in self.states.iter() {
            for child_id in state.get_child_containers() {
                let child_idx = self.arena.register_container(&child_id);
                self.arena.set_parent(child_idx, Some(*idx));
            }
        }

        if !unknown_containers.is_empty() {
            let mut diff_calc = DiffCalculator::new();
            let unknown_diffs = diff_calc.calc_diff_internal(
                oplog,
                &Default::default(),
                Some(&Default::default()),
                oplog.vv(),
                Some(&frontiers),
                Some(&|idx| !idx.is_unknown() && unknown_containers.contains(&idx)),
            );
            self.apply_diff(InternalDocDiff {
                origin: Default::default(),
                by: EventTriggerKind::Import,
                diff: unknown_diffs.into(),
                new_version: Cow::Owned(frontiers.clone()),
            })
        }

        if self.is_recording() {
            let diff: Vec<_> = self
                .states
                .iter_mut()
                .map(|(&idx, state)| InternalContainerDiff {
                    idx,
                    bring_back: false,
                    is_container_deleted: false,
                    diff: state
                        .to_diff(&self.arena, &self.global_txn, &self.weak_state)
                        .into(),
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
        let id: ContainerIdRaw = id.into();
        let cid;
        let idx = match id {
            ContainerIdRaw::Root { name } => {
                cid = crate::container::ContainerID::Root {
                    name,
                    container_type: crate::ContainerType::Text,
                };
                Some(self.arena.register_container(&cid))
            }
            ContainerIdRaw::Normal { id: _ } => {
                cid = id.with_type(crate::ContainerType::Text);
                self.arena.id_to_idx(&cid)
            }
        };

        let idx = idx.unwrap();
        self.states
            .entry(idx)
            .or_insert_with(|| State::new_richtext(idx, self.config.text_style_config.clone()))
            .as_richtext_state_mut()
            .map(|x| &mut **x)
    }

    #[inline(always)]
    #[allow(unused)]
    pub(crate) fn with_state<F, R>(&mut self, idx: ContainerIdx, f: F) -> R
    where
        F: FnOnce(&State) -> R,
    {
        let state = self.states.get(&idx);
        if let Some(state) = state {
            f(state)
        } else {
            let state = self.create_state(idx);
            let ans = f(&state);
            self.states.insert(idx, state);
            ans
        }
    }

    #[inline(always)]
    pub(crate) fn with_state_mut<F, R>(&mut self, idx: ContainerIdx, f: F) -> R
    where
        F: FnOnce(&mut State) -> R,
    {
        let state = self.states.get_mut(&idx);
        if let Some(state) = state {
            f(state)
        } else {
            let mut state = self.create_state(idx);
            let ans = f(&mut state);
            self.states.insert(idx, state);
            ans
        }
    }

    pub(super) fn is_in_txn(&self) -> bool {
        self.in_txn
    }

    pub fn is_empty(&self) -> bool {
        !self.in_txn && self.states.is_empty() && self.arena.can_import_snapshot()
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

        LoroValue::Map(Arc::new(ans))
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

        LoroValue::Map(Arc::new(ans))
    }

    pub(crate) fn get_container_deep_value_with_id(
        &mut self,
        container: ContainerIdx,
        id: Option<ContainerID>,
    ) -> LoroValue {
        let id = id.unwrap_or_else(|| self.arena.idx_to_id(container).unwrap());
        let Some(state) = self.states.get_mut(&container) else {
            return container.get_type().default_value();
        };
        let value = state.get_value();
        let cid_str =
            LoroValue::String(Arc::new(format!("idx:{}, id:{}", container.to_index(), id)));
        match value {
            LoroValue::Container(_) => unreachable!(),
            LoroValue::List(mut list) => {
                if list.iter().all(|x| !x.is_container()) {
                    return LoroValue::Map(Arc::new(fx_map!(
                        "cid".into() => cid_str,
                        "value".into() => LoroValue::List(list)
                    )));
                }

                let list_mut = Arc::make_mut(&mut list);
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

                LoroValue::Map(Arc::new(fx_map!(
                    "cid".into() => cid_str,
                    "value".into() => LoroValue::List(list)
                )))
            }
            LoroValue::Map(mut map) => {
                let map_mut = Arc::make_mut(&mut map);
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

                LoroValue::Map(Arc::new(fx_map!(
                    "cid".into() => cid_str,
                    "value".into() => LoroValue::Map(map)
                )))
            }
            _ => LoroValue::Map(Arc::new(fx_map!(
                "cid".into() => cid_str,
                "value".into() => value
            ))),
        }
    }

    pub fn get_container_deep_value(&mut self, container: ContainerIdx) -> LoroValue {
        let Some(state) = self.states.get_mut(&container) else {
            return container.get_type().default_value();
        };
        let value = state.get_value();
        match value {
            LoroValue::Container(_) => unreachable!(),
            LoroValue::List(mut list) => {
                if container.get_type() == ContainerType::Tree {
                    // Each tree node has an associated map container to represent
                    // the metadata of this node. When the user get the deep value,
                    // we need to add a field named `meta` to the tree node,
                    // whose value is deep value of map container.
                    get_meta_value(Arc::make_mut(&mut list), self);
                } else {
                    if list.iter().all(|x| !x.is_container()) {
                        return LoroValue::List(list);
                    }

                    let list_mut = Arc::make_mut(&mut list);
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

                let map_mut = Arc::make_mut(&mut map);
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
                            "⚠️ WARNING: ignore event because cannot find its path {:#?}",
                            &container_diff,
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
        diff.sort_by_key(|x| x.path.len());
        DocDiff {
            from,
            to,
            origin,
            by: triggered_by,
            diff,
        }
    }

    pub(crate) fn get_reachable(&self, id: &ContainerID) -> bool {
        let Some(mut idx) = self.arena.id_to_idx(id) else {
            return false;
        };
        loop {
            let id = self.arena.idx_to_id(idx).unwrap();
            if let Some(parent_idx) = self.arena.get_parent(idx) {
                let Some(parent_state) = self.states.get(&parent_idx) else {
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
    fn get_path(&self, idx: ContainerIdx) -> Option<Vec<(ContainerID, Index)>> {
        let mut ans = Vec::new();
        let mut idx = idx;
        loop {
            let id = self.arena.idx_to_id(idx).unwrap();
            if let Some(parent_idx) = self.arena.get_parent(idx) {
                let parent_state = self.states.get(&parent_idx)?;
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
                    info!(?id, "Missing parent - container is deleted");
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

        if !self.is_empty() {
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
                if let LoroValue::I64(c) = value {
                    if c == 0 {
                        return None;
                    }
                }
            }

            Some((id, (state.container_idx(), value)))
        }

        let self_id_to_states: FxHashMap<ContainerID, (ContainerIdx, LoroValue)> = self
            .states
            .values_mut()
            .filter_map(|state: &mut State| {
                let arena = &self.arena;
                get_entries_for_state(arena, state)
            })
            .collect();
        let mut other_id_to_states: FxHashMap<ContainerID, (ContainerIdx, LoroValue)> = other
            .states
            .values_mut()
            .filter_map(|state: &mut State| {
                let arena = &other.arena;
                get_entries_for_state(arena, state)
            })
            .collect();
        for (id, (idx, this_value)) in self_id_to_states {
            let (_, other_value) = match other_id_to_states.remove(&id) {
                Some(x) => x,
                None => {
                    panic!("id: {:?}, path: {:?} is missing", id, self.get_path(idx));
                }
            };

            assert_eq!(
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
        let state_entries_size = self.states.len()
            * (std::mem::size_of::<State>() + std::mem::size_of::<ContainerIdx>());
        let mut state_size_sum = 0;
        for state in self.states.values() {
            state_size_sum += state.estimate_size();
        }

        eprintln!(
            "ContainerNum: {}\nEstimated state size: \nEntries: {} \nSum: {}",
            self.states.len(),
            state_entries_size,
            state_size_sum
        );
    }

    pub fn create_state(&self, idx: ContainerIdx) -> State {
        match idx.get_type() {
            ContainerType::Map => State::MapState(Box::new(MapState::new(idx))),
            ContainerType::List => State::ListState(Box::new(ListState::new(idx))),
            ContainerType::Text => State::RichtextState(Box::new(RichtextState::new(
                idx,
                self.config.text_style_config.clone(),
            ))),
            ContainerType::Tree => State::TreeState(Box::new(TreeState::new(
                idx,
                self.peer,
                self.config.tree_position_jitter.clone(),
            ))),
            ContainerType::MovableList => {
                State::MovableListState(Box::new(MovableListState::new(idx)))
            }
            #[cfg(feature = "counter")]
            ContainerType::Counter => {
                State::CounterState(Box::new(counter_state::CounterState::new(idx)))
            }
            ContainerType::Unknown(_) => State::UnknownState(Box::new(UnknownState::new(idx))),
        }
    }

    pub fn create_unknown_state(&self, idx: ContainerIdx) -> State {
        State::UnknownState(Box::new(UnknownState::new(idx)))
    }

    pub fn get_relative_position(&mut self, pos: &Cursor) -> Option<usize> {
        let idx = self.arena.register_container(&pos.container);
        let state = self.states.get_mut(&idx)?;
        if let Some(id) = pos.id {
            match state {
                State::ListState(s) => s.get_index_of_id(id),
                State::RichtextState(s) => s.get_event_index_of_id(id),
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
                State::RichtextState(s) => Some(s.len_event()),
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

        let mut state_idx = {
            let root_index = path[0].as_key()?;
            self.arena.get_root_container_idx_by_key(root_index)?
        };

        if path.len() == 1 {
            let cid = self.arena.idx_to_id(state_idx)?;
            return Some(LoroValue::Container(cid));
        }

        for index in path[..path.len() - 1].iter().skip(1) {
            let parent_state = self.states.get(&state_idx)?;
            match parent_state {
                State::ListState(l) => {
                    let Some(LoroValue::Container(c)) = l.get(*index.as_seq()?) else {
                        return None;
                    };
                    state_idx = self.arena.register_container(c);
                }
                State::MovableListState(l) => {
                    let Some(LoroValue::Container(c)) = l.get(*index.as_seq()?, IndexType::ForUser)
                    else {
                        return None;
                    };
                    state_idx = self.arena.register_container(c);
                }
                State::MapState(m) => {
                    let Some(LoroValue::Container(c)) = m.get(index.as_key()?) else {
                        return None;
                    };
                    state_idx = self.arena.register_container(c);
                }
                State::RichtextState(_) => return None,
                State::TreeState(_) => {
                    let id = index.as_node()?;
                    let cid = id.associated_meta_container();
                    state_idx = self.arena.register_container(&cid);
                }
                #[cfg(feature = "counter")]
                State::CounterState(_) => return None,
                State::UnknownState(_) => unreachable!(),
            }
        }

        let parent_state = self.states.get_mut(&state_idx)?;
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
}

fn trigger_on_new_container(state_diff: &Diff, mut listener: impl FnMut(ContainerIdx)) {
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

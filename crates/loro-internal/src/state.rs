use std::{borrow::Cow, mem::take, sync::Arc};

use enum_as_inner::EnumAsInner;
use enum_dispatch::enum_dispatch;
use fxhash::{FxHashMap, FxHashSet};
use loro_common::{ContainerID, LoroResult};

use crate::{
    configure::{DefaultRandom, SecureRandomGenerator},
    container::{idx::ContainerIdx, ContainerIdRaw},
    delta::{Delta, DeltaItem},
    event::InternalContainerDiff,
    event::{Diff, Index},
    fx_map,
    id::PeerID,
    op::{Op, RawOp},
    version::Frontiers,
    ContainerType, InternalString, LoroValue,
};

mod list_state;
mod map_state;
mod richtext_state;

pub(crate) use list_state::ListState;
pub(crate) use map_state::MapState;
pub(crate) use richtext_state::RichtextState;

use super::{
    arena::SharedArena,
    event::{ContainerDiff, DocDiff, InternalDocDiff},
};

#[derive(Clone)]
pub struct DocState {
    pub(super) peer: PeerID,

    pub(super) frontiers: Frontiers,
    pub(super) states: FxHashMap<ContainerIdx, State>,
    pub(super) arena: SharedArena,

    // txn related stuff
    in_txn: bool,
    changed_idx_in_txn: FxHashSet<ContainerIdx>,

    // diff related stuff
    event_recorder: EventRecorder,
}

#[enum_dispatch]
pub trait ContainerState: Clone {
    fn apply_diff(&mut self, diff: &mut Diff, arena: &SharedArena);
    fn apply_op(&mut self, raw_op: &RawOp, op: &Op, arena: &SharedArena);
    /// Convert a state to a diff, such that an empty state will be transformed into the same as this state when it's applied.
    fn to_diff(&self) -> Diff;

    /// Start a transaction
    ///
    /// The transaction may be aborted later, then all the ops during this transaction need to be undone.
    fn start_txn(&mut self);
    fn abort_txn(&mut self);
    /// Commit the transaction and return the diff of the transaction.
    /// If `record_diff` in [Self::start_txn] is false, return None.
    fn commit_txn(&mut self);

    fn get_value(&self) -> LoroValue;

    /// Get the index of the child container
    #[allow(unused)]
    fn get_child_index(&self, id: &ContainerID) -> Option<Index> {
        None
    }

    #[allow(unused)]
    fn get_child_containers(&self) -> Vec<ContainerID> {
        Vec::new()
    }
}

#[allow(clippy::enum_variant_names)]
#[enum_dispatch(ContainerState)]
#[derive(EnumAsInner, Clone, Debug)]
pub enum State {
    ListState,
    MapState,
    RichtextState,
}

impl State {
    #[allow(unused)]
    pub fn new_list(idx: ContainerIdx) -> Self {
        Self::ListState(ListState::new(idx))
    }

    #[allow(unused)]
    pub fn new_map(idx: ContainerIdx) -> Self {
        Self::MapState(MapState::new(idx))
    }

    #[allow(unused)]
    pub fn new_richtext(idx: ContainerIdx) -> Self {
        Self::RichtextState(RichtextState::new(idx))
    }
}

impl DocState {
    #[inline]
    pub fn new(arena: SharedArena) -> Self {
        let peer = DefaultRandom.next_u64();
        // TODO: maybe we should switch to certain version in oplog?
        Self {
            peer,
            arena,
            frontiers: Frontiers::default(),
            states: FxHashMap::default(),
            in_txn: false,
            changed_idx_in_txn: FxHashSet::default(),
            event_recorder: Default::default(),
        }
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
        if !self.event_recorder.recording_diff {
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
    fn pre_txn(&mut self, next_origin: InternalString, next_local: bool) {
        if !self.is_recording() {
            return;
        }

        let Some(last_diff) = self.event_recorder.diffs.last() else {
            return;
        };

        if last_diff.origin == next_origin && last_diff.local == next_local {
            return;
        }

        // current diff batch cannot merge with the incoming diff,
        // need to convert all the current diffs into event
        self.convert_current_batch_diff_into_event()
    }

    fn convert_current_batch_diff_into_event(&mut self) {
        let recorder = &mut self.event_recorder;
        let diffs = std::mem::take(&mut recorder.diffs);
        let start = recorder.diff_start_version.take().unwrap();
        recorder.diff_start_version = Some((*diffs.last().unwrap().new_version).to_owned());
        // debug_dbg!(&diffs);
        let event = self.diffs_to_event(diffs, start);
        // debug_dbg!(&event);
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

    pub(crate) fn apply_diff(&mut self, mut diff: InternalDocDiff<'static>) {
        if self.in_txn {
            panic!("apply_diff should not be called in a transaction");
        }

        self.pre_txn(diff.origin.clone(), diff.local);
        let Cow::Owned(inner) = &mut diff.diff else {
            unreachable!()
        };
        for diff in inner.iter_mut() {
            let state = self
                .states
                .entry(diff.idx)
                .or_insert_with(|| create_state(diff.idx));

            if self.in_txn {
                state.start_txn();
                self.changed_idx_in_txn.insert(diff.idx);
            }

            state.apply_diff(&mut diff.diff, &self.arena);
        }

        self.frontiers = (*diff.new_version).to_owned();

        if self.is_recording() {
            self.record_diff(diff)
        }
    }

    pub fn apply_local_op(&mut self, raw_op: &RawOp, op: &Op) -> LoroResult<()> {
        let state = self
            .states
            .entry(op.container)
            .or_insert_with(|| create_state(op.container));

        if self.in_txn {
            state.start_txn();
            self.changed_idx_in_txn.insert(op.container);
        }

        // TODO: make apply_op return a result
        state.apply_op(raw_op, op, &self.arena);
        Ok(())
    }

    pub(crate) fn start_txn(&mut self, origin: InternalString, local: bool) {
        self.pre_txn(origin, local);
        self.in_txn = true;
    }

    #[inline]
    pub(crate) fn abort_txn(&mut self) {
        for container_idx in std::mem::take(&mut self.changed_idx_in_txn) {
            self.states.get_mut(&container_idx).unwrap().abort_txn();
        }

        self.in_txn = false;
    }

    pub(crate) fn commit_txn(&mut self, new_frontiers: Frontiers, diff: Option<InternalDocDiff>) {
        for container_idx in std::mem::take(&mut self.changed_idx_in_txn) {
            self.states.get_mut(&container_idx).unwrap().commit_txn();
        }

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

    pub(crate) fn get_value_by_idx(&self, container_idx: ContainerIdx) -> LoroValue {
        self.states
            .get(&container_idx)
            .map(|x| x.get_value())
            .unwrap_or_else(|| match container_idx.get_type() {
                ContainerType::Map => LoroValue::Map(Arc::new(Default::default())),
                ContainerType::List => LoroValue::List(Arc::new(Default::default())),
                ContainerType::Text => LoroValue::String(Arc::new(Default::default())),
            })
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
    ) {
        assert!(self.states.is_empty(), "overriding states");
        self.pre_txn(Default::default(), false);
        self.states = states;
        for (idx, state) in self.states.iter() {
            for child_id in state.get_child_containers() {
                let child_idx = self.arena.register_container(&child_id);
                self.arena.set_parent(child_idx, Some(*idx));
            }
        }

        if self.is_recording() {
            self.record_diff(InternalDocDiff {
                origin: Default::default(),
                local: false,
                diff: self
                    .states
                    .iter()
                    .map(|(&idx, state)| InternalContainerDiff {
                        idx,
                        diff: state.to_diff(),
                    })
                    .collect(),
                new_version: Cow::Owned(frontiers.clone()),
            });
        }

        self.frontiers = frontiers;
    }

    /// id can be a str, ContainerID, or ContainerIdRaw.
    /// if it's str it will use Root container, which will not be None
    pub fn get_text<I: Into<ContainerIdRaw>>(
        &mut self,
        id: I,
    ) -> Option<&richtext_state::RichtextState> {
        let id: ContainerIdRaw = id.into();
        let idx = match id {
            ContainerIdRaw::Root { name } => Some(self.arena.register_container(
                &crate::container::ContainerID::Root {
                    name,
                    container_type: crate::ContainerType::Text,
                },
            )),
            ContainerIdRaw::Normal { id: _ } => self
                .arena
                .id_to_idx(&id.with_type(crate::ContainerType::Text)),
        };

        let idx = idx.unwrap();
        self.states
            .entry(idx)
            .or_insert_with(|| State::new_richtext(idx))
            .as_richtext_state()
    }

    #[inline(always)]
    pub(crate) fn with_state<F, R>(&self, idx: ContainerIdx, f: F) -> R
    where
        F: FnOnce(&State) -> R,
    {
        let state = self.states.get(&idx);
        if let Some(state) = state {
            f(state)
        } else {
            f(&create_state(idx))
        }
    }

    pub(super) fn is_in_txn(&self) -> bool {
        self.in_txn
    }

    pub fn is_empty(&self) -> bool {
        !self.in_txn && self.states.is_empty() && self.arena.is_empty()
    }

    pub fn get_deep_value(&self) -> LoroValue {
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

    pub fn get_deep_value_with_id(&self) -> LoroValue {
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
        &self,
        container: ContainerIdx,
        id: Option<ContainerID>,
    ) -> LoroValue {
        let id = id.unwrap_or_else(|| self.arena.idx_to_id(container).unwrap());
        let Some(state) = self.states.get(&container) else {
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

    pub fn get_container_deep_value(&self, container: ContainerIdx) -> LoroValue {
        let Some(state) = self.states.get(&container) else {
            return container.get_type().default_value();
        };
        let value = state.get_value();
        match value {
            LoroValue::Container(_) => unreachable!(),
            LoroValue::List(mut list) => {
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
    fn diffs_to_event(&self, diffs: Vec<InternalDocDiff<'_>>, from: Frontiers) -> DocDiff {
        if diffs.is_empty() {
            panic!("diffs is empty");
        }

        let mut containers = FxHashMap::default();
        let to = (*diffs.last().unwrap().new_version).to_owned();
        let origin = diffs[0].origin.clone();
        let local = diffs[0].local;
        for diff in diffs {
            #[allow(clippy::unnecessary_to_owned)]
            for mut container_diff in diff.diff.into_owned() {
                self.convert_raw(&mut container_diff.diff, container_diff.idx);
                let Some((last_container_diff, _)) = containers.get_mut(&container_diff.idx) else {
                    if let Some(path) = self.get_path(container_diff.idx) {
                        containers.insert(container_diff.idx, (container_diff.diff, path));
                    } else {
                        // if we cannot find the path to the container, the container must be overwritten afterwards.
                        // So we can ignore the diff from it.
                        debug_log::debug_log!(
                            "ignore because cannot find path {:#?}",
                            &container_diff
                        );
                    }
                    continue;
                };

                *last_container_diff = take(last_container_diff)
                    .compose(container_diff.diff)
                    .unwrap();
            }
        }

        let mut diff: Vec<_> = containers
            .into_iter()
            .map(|(idx, (diff, path))| {
                let id = self.arena.get_container_id(idx).unwrap();
                ContainerDiff {
                    id,
                    idx,
                    diff,
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
            local,
            diff,
        }
    }

    // the container may be override, so it may return None
    fn get_path(&self, idx: ContainerIdx) -> Option<Vec<(ContainerID, Index)>> {
        let mut ans = Vec::new();
        let mut idx = idx;
        loop {
            let id = self.arena.idx_to_id(idx).unwrap();
            if let Some(parent_idx) = self.arena.get_parent(idx) {
                let parent_state = self.states.get(&parent_idx).unwrap();
                let prop = parent_state.get_child_index(&id)?;
                ans.push((id, prop));
                idx = parent_idx;
            } else {
                // this container may be deleted
                let prop = id.as_root()?.0.clone();
                ans.push((id, Index::Key(prop)));
                break;
            }
        }

        ans.reverse();
        Some(ans)
    }

    /// convert seq raw to text/list
    pub(crate) fn convert_raw(&self, diff: &mut Diff, idx: ContainerIdx) {
        let seq = match diff {
            Diff::SeqRaw(seq) => seq,
            _ => return,
        };

        match idx.get_type() {
            ContainerType::List => {
                let mut list: Delta<Vec<LoroValue>> = Delta::new();

                for span in seq.iter() {
                    match span {
                        DeltaItem::Retain { len, .. } => {
                            list = list.retain(*len);
                        }
                        DeltaItem::Insert { value, .. } => {
                            let mut arr = Vec::new();
                            for slice in value.0.iter() {
                                let values = self
                                    .arena
                                    .get_values(slice.0.start as usize..slice.0.end as usize);
                                arr.extend_from_slice(&values);
                            }

                            list = list.insert(arr)
                        }
                        DeltaItem::Delete { len, .. } => list = list.delete(*len),
                    }
                }
                *diff = Diff::List(list);
            }
            ContainerType::Map => unreachable!(),
            ContainerType::Text => unimplemented!(),
        }
    }
}

pub fn create_state(idx: ContainerIdx) -> State {
    match idx.get_type() {
        ContainerType::Map => State::MapState(MapState::new(idx)),
        ContainerType::List => State::ListState(ListState::new(idx)),
        ContainerType::Text => State::RichtextState(RichtextState::new(idx)),
    }
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

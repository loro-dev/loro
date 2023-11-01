use std::{borrow::Cow, sync::Arc};

use enum_as_inner::EnumAsInner;
use enum_dispatch::enum_dispatch;
use fxhash::{FxHashMap, FxHashSet};
use loro_common::{ContainerID, LoroResult};

use crate::{
    configure::{DefaultRandom, SecureRandomGenerator},
    container::{idx::ContainerIdx, ContainerIdRaw},
    event::{Diff, DiffVariant, Index},
    event::{InternalContainerDiff, InternalDiff},
    fx_map,
    id::PeerID,
    op::{Op, RawOp},
    version::Frontiers,
    ContainerType, InternalString, LoroValue,
};

mod list_state;
mod map_state;
mod richtext_state;
mod tree_state;

pub(crate) use list_state::ListState;
pub(crate) use map_state::MapState;
pub(crate) use richtext_state::RichtextState;
pub(crate) use tree_state::{get_meta_value, TreeState};

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
    fn apply_diff_and_convert(&mut self, diff: InternalDiff, arena: &SharedArena) -> Diff;

    fn apply_diff(&mut self, diff: InternalDiff, arena: &SharedArena) {
        self.apply_diff_and_convert(diff, arena);
    }

    fn apply_op(&mut self, raw_op: &RawOp, op: &Op, arena: &SharedArena) -> LoroResult<()>;
    /// Convert a state to a diff, such that an empty state will be transformed into the same as this state when it's applied.
    fn to_diff(&mut self) -> Diff;

    /// Start a transaction
    ///
    /// The transaction may be aborted later, then all the ops during this transaction need to be undone.
    fn start_txn(&mut self);
    fn abort_txn(&mut self);
    /// Commit the transaction and return the diff of the transaction.
    /// If `record_diff` in [Self::start_txn] is false, return None.
    fn commit_txn(&mut self);

    fn get_value(&mut self) -> LoroValue;

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
    TreeState,
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

    /// It's expected that diff only contains [`InternalDiff`]
    pub(crate) fn apply_diff(&mut self, mut diff: InternalDocDiff<'static>) {
        if self.in_txn {
            panic!("apply_diff should not be called in a transaction");
        }

        self.pre_txn(diff.origin.clone(), diff.local);
        let Cow::Owned(inner) = &mut diff.diff else {
            unreachable!()
        };
        // TODO: avoid inner insert
        // TODO: avoid empty bring back
        // println!("Inner {:?}", inner);
        let mut idx2state_diff = FxHashMap::default();
        let mut current_inner_idx = 0;
        // cache from version state
        loop {
            let (idx, bring_back) = {
                let Some(diff) = inner.get_mut(current_inner_idx) else {
                    break;
                };
                (diff.idx, diff.bring_back)
            };
            if bring_back {
                let state = self.states.entry(idx).or_insert_with(|| create_state(idx));
                let state_diff = state.to_diff();
                // println!("process {:?}", idx);
                bring_back_sub_container(&state_diff, inner, current_inner_idx, &self.arena);
                idx2state_diff.insert(idx, state_diff);
            }
            current_inner_idx += 1;
        }
        // println!("\nexternal DIFF {:?}", inner);
        let mut current_inner_idx = 0;
        let is_recording = self.is_recording();
        loop {
            let (idx, bring_back, internal_diff) = {
                let Some(diff) = inner.get_mut(current_inner_idx) else {
                    break;
                };
                let Some(internal_diff) = std::mem::take(&mut diff.diff) else {
                    // only bring_back

                    current_inner_idx += 1;
                    continue;
                };
                (diff.idx, diff.bring_back, internal_diff)
            };

            let state = self.states.entry(idx).or_insert_with(|| create_state(idx));

            if self.in_txn {
                state.start_txn();
                self.changed_idx_in_txn.insert(idx);
            }
            if is_recording {
                // process bring_back before apply
                let external_diff = if bring_back {
                    let state_diff = idx2state_diff.remove(&idx).unwrap();
                    // println!(
                    //     "1state {:?} apply diff {:?}",
                    //     idx,
                    //     internal_diff.clone().into_internal().unwrap()
                    // );
                    let external_diff = state.apply_diff_and_convert(
                        internal_diff.into_internal().unwrap(),
                        &self.arena,
                    );

                    state_diff.concat(external_diff)
                } else {
                    // println!(
                    //     "2state {:?} apply diff {:?}",
                    //     idx,
                    //     internal_diff.clone().into_internal().unwrap()
                    // );
                    state
                        .apply_diff_and_convert(internal_diff.into_internal().unwrap(), &self.arena)
                };
                let diff = inner.get_mut(current_inner_idx).unwrap();
                diff.diff = Some(external_diff.into());
            } else {
                state.apply_diff(internal_diff.into_internal().unwrap(), &self.arena);
            }
            current_inner_idx += 1;
        }

        self.frontiers = (*diff.new_version).to_owned();

        if self.is_recording() {
            let mut current_inner_idx = 0;
            // bring back
            loop {
                let state_diff = {
                    let Some(diff) = inner.get_mut(current_inner_idx) else {
                        break;
                    };
                    if !diff.bring_back || diff.diff.is_some() {
                        current_inner_idx += 1;
                        continue;
                    }
                    idx2state_diff.remove(&diff.idx).unwrap()
                };
                let diff = inner.get_mut(current_inner_idx).unwrap();
                // println!("###bring back {:?}", self.arena.idx_to_id(diff.idx));
                diff.diff = Some(state_diff.into());
                // println!("###back diff {:?}", diff);
                current_inner_idx += 1;
            }
            // println!("\nFinal external DIFF {:?}", inner);
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

        state.apply_op(raw_op, op, &self.arena)?;
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
            debug_log::debug_log!("TO DIFF");
            let diff = self
                .states
                .iter_mut()
                .map(|(&idx, state)| InternalContainerDiff {
                    idx,
                    bring_back: true,
                    diff: Some(state.to_diff().into()),
                })
                .collect();
            self.record_diff(InternalDocDiff {
                origin: Default::default(),
                local: false,
                diff,
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
            .as_richtext_state_mut()
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

    #[inline(always)]
    pub(crate) fn with_state_mut<F, R>(&mut self, idx: ContainerIdx, f: F) -> R
    where
        F: FnOnce(&mut State) -> R,
    {
        let state = self.states.get_mut(&idx);
        if let Some(state) = state {
            f(state)
        } else {
            f(&mut create_state(idx))
        }
    }

    pub(super) fn is_in_txn(&self) -> bool {
        self.in_txn
    }

    pub fn is_empty(&self) -> bool {
        !self.in_txn && self.states.is_empty() && self.arena.is_empty()
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

        let mut containers = FxHashMap::default();
        let to = (*diffs.last().unwrap().new_version).to_owned();
        let origin = diffs[0].origin.clone();
        let local = diffs[0].local;
        for diff in diffs {
            #[allow(clippy::unnecessary_to_owned)]
            for container_diff in diff.diff.into_owned() {
                let Some((last_container_diff, _)) = containers.get_mut(&container_diff.idx) else {
                    if let Some(path) = self.get_path(container_diff.idx) {
                        containers.insert(container_diff.idx, (container_diff.diff.unwrap(), path));
                    } else {
                        // if we cannot find the path to the container, the container must be overwritten afterwards.
                        // So we can ignore the diff from it.
                        debug_log::debug_log!(
                            "⚠️ WARNING: ignore because cannot find path {:#?} deep_value {:#?}",
                            &container_diff,
                            self.get_deep_value_with_id()
                        );
                    }
                    continue;
                };

                // TODO: PERF avoid this clone
                *last_container_diff = last_container_diff
                    .clone()
                    .compose(container_diff.diff.unwrap())
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
                    diff: diff.into_external().unwrap(),
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
        debug_log::group!("GET PATH {:?}", idx);
        let mut ans = Vec::new();
        let mut idx = idx;
        loop {
            let id = self.arena.idx_to_id(idx).unwrap();
            debug_log::debug_dbg!(&id);
            if let Some(parent_idx) = self.arena.get_parent(idx) {
                let parent_state = self.states.get(&parent_idx).unwrap();
                let Some(prop) = parent_state.get_child_index(&id) else {
                    debug_log::group_end!();
                    return None;
                };
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
        debug_log::group_end!();
        Some(ans)
    }
}

fn bring_back_sub_container(
    state_diff: &Diff,
    inner: &mut Vec<InternalContainerDiff>,
    current_inner_idx: usize,
    arena: &SharedArena,
) {
    match state_diff {
        Diff::List(list) => {
            for delta in list.iter() {
                if delta.is_insert() {
                    for v in delta.as_insert().unwrap().0.iter() {
                        if v.is_container() {
                            let idx = arena.id_to_idx(v.as_container().unwrap()).unwrap();
                            // print!("sub bring {:?}", v);
                            // TODO: perf
                            let mut has = false;
                            for index in (current_inner_idx + 1)..inner.len() {
                                let other = inner.get_mut(index).unwrap();
                                if other.idx == idx {
                                    // println!(" change flag");
                                    other.bring_back = true;
                                    has = true;
                                    break;
                                }
                            }
                            if !has {
                                // println!(" add");
                                inner.insert(
                                    current_inner_idx + 1,
                                    InternalContainerDiff {
                                        idx,
                                        bring_back: true,
                                        diff: None,
                                    },
                                );
                            }
                        }
                    }
                }
            }
        }
        Diff::NewMap(map) => {
            for (_, v) in map.updated.iter() {
                if let Some(LoroValue::Container(id)) = &v.value {
                    let idx = arena.id_to_idx(id).unwrap();
                    // print!("sub bring {:?}", id);
                    let mut has = false;
                    for index in (current_inner_idx + 1)..inner.len() {
                        let other = inner.get_mut(index).unwrap();
                        if other.idx == idx {
                            // println!(" change flag");
                            other.bring_back = true;
                            has = true;
                            break;
                        }
                    }
                    if !has {
                        // println!(" add");
                        inner.insert(
                            current_inner_idx + 1,
                            InternalContainerDiff {
                                idx,
                                bring_back: true,
                                diff: None,
                            },
                        );
                    }
                }
            }
        }
        // Diff::Tree(tree) => {
        //     for diff in tree.diff {
        //         let target = diff.target;
        //         match diff.action {

        //         }
        //     }
        // }
        _ => {}
    };
}

pub fn create_state(idx: ContainerIdx) -> State {
    match idx.get_type() {
        ContainerType::Map => State::MapState(MapState::new(idx)),
        ContainerType::List => State::ListState(ListState::new(idx)),
        ContainerType::Text => State::RichtextState(RichtextState::new(idx)),
        ContainerType::Tree => State::TreeState(TreeState::new()),
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

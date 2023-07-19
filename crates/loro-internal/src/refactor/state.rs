use std::{borrow::Cow, sync::Arc};

use enum_as_inner::EnumAsInner;
use enum_dispatch::enum_dispatch;
use fxhash::{FxHashMap, FxHashSet};
use ring::rand::SystemRandom;

use crate::{
    configure::SecureRandomGenerator,
    container::{registry::ContainerIdx, ContainerIdRaw},
    event::Diff,
    id::PeerID,
    op::RawOp,
    version::Frontiers,
    ContainerType, LoroValue,
};

mod list_state;
mod map_state;
mod text_state;

pub(crate) use list_state::ListState;
pub(crate) use map_state::MapState;
pub(crate) use text_state::TextState;

use super::{arena::SharedArena, oplog::OpLog};

#[derive(Clone)]
pub struct AppState {
    pub(super) peer: PeerID,

    pub(super) frontiers: Frontiers,
    pub(super) states: FxHashMap<ContainerIdx, State>,
    pub(super) arena: SharedArena,

    // txn related stuff
    in_txn: bool,
    changed_idx_in_txn: FxHashSet<ContainerIdx>,

    // diff related stuff
    recording_diff: bool,
    diff: Vec<AppStateDiff<'static>>,
    record_start_version: Option<Frontiers>,
}

#[enum_dispatch]
pub trait ContainerState: Clone {
    fn apply_diff(&mut self, diff: &Diff, arena: &SharedArena);
    fn apply_op(&mut self, op: RawOp);
    /// Convert a state to a diff that when apply this diff on a empty state,
    /// the state will be the same as this state.
    fn to_diff(&self) -> Diff;

    /// Start a transaction
    ///
    /// The transaction may be aborted later, then all the ops during this transaction need to be undone.
    fn start_txn(&mut self);
    fn abort_txn(&mut self);
    fn commit_txn(&mut self);

    fn get_value(&self) -> LoroValue;
}

#[allow(clippy::enum_variant_names)]
#[enum_dispatch(ContainerState)]
#[derive(EnumAsInner, Clone, Debug)]
pub enum State {
    ListState,
    MapState,
    TextState,
}

impl State {
    pub fn new_list() -> Self {
        Self::ListState(ListState::default())
    }

    pub fn new_map() -> Self {
        Self::MapState(MapState::new())
    }

    pub fn new_text() -> Self {
        Self::TextState(TextState::default())
    }
}

#[derive(Debug, Clone)]
pub struct ContainerStateDiff {
    pub idx: ContainerIdx,
    pub diff: Diff,
}

#[derive(Debug, Clone)]
pub struct AppStateDiff<'a> {
    pub(crate) diff: Cow<'a, [ContainerStateDiff]>,
    pub(crate) new_version: Cow<'a, Frontiers>,
    pub(crate) old_version: Option<Frontiers>,
}

impl<'a> AppStateDiff<'a> {
    pub fn into_owned(self) -> AppStateDiff<'static> {
        AppStateDiff {
            diff: Cow::Owned((*self.diff).to_owned()),
            new_version: Cow::Owned((*self.new_version).to_owned()),
            old_version: self.old_version,
        }
    }
}

impl AppState {
    #[inline]
    pub fn new(oplog: &OpLog) -> Self {
        let peer = SystemRandom::new().next_u64();
        // TODO: maybe we should switch to certain version in oplog
        Self {
            peer,
            frontiers: Frontiers::default(),
            states: FxHashMap::default(),
            arena: oplog.arena.clone(),
            in_txn: false,
            changed_idx_in_txn: FxHashSet::default(),
            diff: Default::default(),
            recording_diff: false,
            record_start_version: None,
        }
    }

    #[inline]
    pub fn new_from_arena(arena: SharedArena) -> Self {
        let peer = SystemRandom::new().next_u64();
        // TODO: maybe we should switch to certain version in oplog
        Self {
            peer,
            arena,
            frontiers: Frontiers::default(),
            states: FxHashMap::default(),
            in_txn: false,
            changed_idx_in_txn: FxHashSet::default(),
            diff: Default::default(),
            recording_diff: false,
            record_start_version: None,
        }
    }

    #[inline]
    pub fn is_recording(&self) -> bool {
        self.recording_diff
    }

    #[inline]
    pub fn start_recording(&mut self) {
        self.recording_diff = true;
        self.record_start_version = Some(self.frontiers.clone());
    }

    /// Stop recording diff and clear the diff
    #[inline]
    pub fn stop_and_clear_recording(&mut self) {
        self.recording_diff = false;
        self.record_start_version = None;
        self.diff.clear();
    }

    #[inline]
    pub fn take_diff(&mut self) -> Vec<AppStateDiff<'static>> {
        let mut diffs = std::mem::take(&mut self.diff);
        let mut last_version = self.record_start_version.take().unwrap();
        for diff in diffs.iter_mut() {
            diff.old_version = Some(last_version);
            last_version = (*diff.new_version).clone();
        }

        diffs
    }

    #[inline]
    pub fn set_peer_id(&mut self, peer: PeerID) {
        self.peer = peer;
    }

    pub fn apply_diff(&mut self, diff: AppStateDiff) {
        if self.in_txn {
            panic!("apply_diff should not be called in a transaction");
        }

        for diff in diff.diff.iter() {
            let state = self.states.entry(diff.idx).or_insert_with(|| {
                let id = self.arena.get_container_id(diff.idx).unwrap();
                create_state(id.container_type())
            });

            if self.in_txn {
                state.start_txn();
                self.changed_idx_in_txn.insert(diff.idx);
            }

            state.apply_diff(&diff.diff, &self.arena);
        }

        self.frontiers = (*diff.new_version).to_owned();

        if self.recording_diff {
            self.diff.push(diff.into_owned());
        }
    }

    pub fn apply_local_op(&mut self, op: RawOp) {
        let state = self.states.entry(op.container).or_insert_with(|| {
            let id = self.arena.get_container_id(op.container).unwrap();
            create_state(id.container_type())
        });

        if self.in_txn {
            state.start_txn();
            self.changed_idx_in_txn.insert(op.container);
        }

        state.apply_op(op);
    }

    pub(crate) fn start_txn(&mut self) {
        self.in_txn = true;
    }

    #[inline]
    pub(crate) fn abort_txn(&mut self) {
        for container_idx in std::mem::take(&mut self.changed_idx_in_txn) {
            self.states.get_mut(&container_idx).unwrap().abort_txn();
        }

        self.in_txn = false;
    }

    pub(crate) fn commit_txn(&mut self, new_frontiers: Frontiers, diff: Option<AppStateDiff>) {
        for container_idx in std::mem::take(&mut self.changed_idx_in_txn) {
            self.states.get_mut(&container_idx).unwrap().commit_txn();
        }

        self.in_txn = false;
        self.frontiers = new_frontiers;
        if self.recording_diff {
            self.diff.push(diff.unwrap().into_owned());
        }
    }

    #[inline]
    pub(super) fn get_state_mut(&mut self, idx: ContainerIdx) -> Option<&mut State> {
        self.states.get_mut(&idx)
    }

    #[inline]
    pub(super) fn get_state(&self, idx: ContainerIdx) -> Option<&State> {
        self.states.get(&idx)
    }

    pub(crate) fn get_value_by_idx(&self, container_idx: ContainerIdx) -> LoroValue {
        self.states
            .get(&container_idx)
            .map(|x| x.get_value())
            .unwrap_or_else(|| match container_idx.get_type() {
                ContainerType::Text => LoroValue::String(Arc::new(Default::default())),
                ContainerType::Map => LoroValue::Map(Arc::new(Default::default())),
                ContainerType::List => LoroValue::List(Arc::new(Default::default())),
            })
    }

    /// Set the state of the container with the given container idx.
    /// This is only used for decode.
    ///
    /// # Panic
    ///
    /// If the state already exists.
    pub(super) fn init_with_states_and_version(
        &mut self,
        states: FxHashMap<ContainerIdx, State>,
        frontiers: Frontiers,
    ) {
        assert!(self.states.is_empty(), "overriding states");
        self.states = states;

        if self.is_recording() {
            self.diff.push(AppStateDiff {
                diff: self
                    .states
                    .iter()
                    .map(|(&idx, state)| ContainerStateDiff {
                        idx,
                        diff: state.to_diff(),
                    })
                    .collect(),
                new_version: Cow::Owned(frontiers.clone()),
                old_version: Default::default(),
            })
        }

        self.frontiers = frontiers;
    }

    /// id can be a str, ContainerID, or ContainerIdRaw.
    /// if it's str it will use Root container, which will not be None
    pub fn get_text<I: Into<ContainerIdRaw>>(&mut self, id: I) -> Option<&text_state::TextState> {
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
            .or_insert_with(State::new_text)
            .as_text_state()
    }

    pub(crate) fn with_state<F, R>(&self, idx: ContainerIdx, f: F) -> R
    where
        F: FnOnce(&State) -> R,
    {
        let state = self.states.get(&idx);
        if let Some(state) = state {
            f(state)
        } else {
            f(&create_state(idx.get_type()))
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
}

pub fn create_state(kind: ContainerType) -> State {
    match kind {
        ContainerType::Text => State::TextState(TextState::new()),
        ContainerType::Map => State::MapState(MapState::new()),
        ContainerType::List => State::ListState(ListState::new()),
    }
}

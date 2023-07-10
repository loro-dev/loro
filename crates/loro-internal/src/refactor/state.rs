use enum_dispatch::enum_dispatch;
use fxhash::{FxHashMap, FxHashSet};
use ring::rand::SystemRandom;

use crate::{
    change::Lamport,
    configure::SecureRandomGenerator,
    container::{registry::ContainerIdx, ContainerID},
    event::Diff,
    id::{Counter, PeerID},
    op::{RawOp, RawOpContent},
    version::{Frontiers, ImVersionVector},
    ContainerType,
};

mod list_state;
mod map_state;
mod text_state;

use list_state::ListState;
use map_state::MapState;
use text_state::TextState;

use super::{arena::SharedArena, oplog::OpLog};

#[derive(Clone)]
pub struct AppState {
    pub(super) peer: PeerID,
    pub(super) next_lamport: Lamport,
    pub(super) next_counter: Counter,

    pub(super) frontiers: Frontiers,
    states: FxHashMap<ContainerIdx, State>,
    pub(super) arena: SharedArena,

    in_txn: bool,
    changed_in_txn: FxHashSet<ContainerIdx>,
}

#[enum_dispatch]
pub trait ContainerState: Clone {
    fn apply_diff(&mut self, diff: Diff);
    fn apply_op(&mut self, op: RawOp);

    /// Start a transaction
    ///
    /// The transaction may be aborted later, then all the ops during this transaction need to be undone.
    fn start_txn(&mut self);
    fn abort_txn(&mut self);
    fn commit_txn(&mut self);
}

#[enum_dispatch(ContainerState)]
#[derive(Clone)]
pub enum State {
    ListState,
    MapState,
    TextState,
}

pub struct ContainerStateDiff {
    pub idx: ContainerIdx,
    pub diff: Diff,
}

impl AppState {
    pub fn new(oplog: &OpLog) -> Self {
        let peer = SystemRandom::new().next_u64();
        // TODO: maybe we should switch to certain version in oplog
        Self {
            peer,
            next_counter: 0,
            next_lamport: oplog.latest_lamport + 1,
            frontiers: Frontiers::default(),
            states: FxHashMap::default(),
            arena: oplog.arena.clone(),
            in_txn: false,
            changed_in_txn: FxHashSet::default(),
        }
    }

    pub fn set_peer_id(&mut self, peer: PeerID) {
        self.peer = peer;
    }

    pub fn apply_diff(&mut self, _diff: &ContainerStateDiff) {
        todo!()
    }

    pub fn apply_local_op(&mut self, op: RawOp) {
        let state = self.states.entry(op.container).or_insert_with(|| {
            let id = self.arena.get_container_id(op.container).unwrap();
            create_state(id.container_type())
        });
        if self.in_txn {
            self.changed_in_txn.insert(op.container);
        }
        state.apply_op(op);
    }

    pub(crate) fn start_txn(&mut self) {
        self.in_txn = true;
    }

    pub(crate) fn abort_txn(&mut self) {
        for container_idx in std::mem::take(&mut self.changed_in_txn) {
            self.states.get_mut(&container_idx).unwrap().abort_txn();
        }

        self.in_txn = false;
    }

    pub(crate) fn commit_txn(&mut self, new_frontiers: Frontiers) {
        for container_idx in std::mem::take(&mut self.changed_in_txn) {
            self.states.get_mut(&container_idx).unwrap().commit_txn();
        }

        self.in_txn = false;
        self.frontiers = new_frontiers;
    }

    pub(super) fn get_state_mut(&mut self, idx: ContainerIdx) -> Option<&mut State> {
        self.states.get_mut(&idx)
    }
}

pub fn create_state(kind: ContainerType) -> State {
    match kind {
        ContainerType::Text => State::TextState(TextState::new()),
        ContainerType::Map => State::MapState(MapState::new()),
        ContainerType::List => State::ListState(ListState::new()),
    }
}

use enum_dispatch::enum_dispatch;
use fxhash::{FxHashMap, FxHashSet};
use ring::rand::SystemRandom;

use crate::{
    configure::SecureRandomGenerator,
    container::{registry::ContainerIdx, ContainerID},
    event::Diff,
    id::PeerID,
    version::{Frontiers, ImVersionVector},
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

    pub(super) frontiers: Frontiers,
    state: FxHashMap<ContainerIdx, State>,
    pub(super) arena: SharedArena,

    in_txn: bool,
    changed_in_txn: FxHashSet<ContainerIdx>,
}

#[enum_dispatch]
pub trait ContainerState: Clone {
    fn apply_diff(&mut self, diff: Diff);

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
        Self {
            peer,
            frontiers: Frontiers::default(),
            state: FxHashMap::default(),
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

    pub(crate) fn start_txn(&mut self) {
        self.in_txn = true;
    }

    pub(crate) fn abort_txn(&mut self) {
        for container_idx in std::mem::take(&mut self.changed_in_txn) {
            self.state.get_mut(&container_idx).unwrap().abort_txn();
        }

        self.in_txn = false;
    }

    pub(crate) fn commit_txn(&mut self, new_frontiers: Frontiers) {
        for container_idx in std::mem::take(&mut self.changed_in_txn) {
            self.state.get_mut(&container_idx).unwrap().commit_txn();
        }

        self.in_txn = false;
        self.frontiers = new_frontiers;
    }

    pub(super) fn get_state_mut(&mut self, idx: ContainerIdx) -> Option<&mut State> {
        self.state.get_mut(&idx)
    }
}

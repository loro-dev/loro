use enum_dispatch::enum_dispatch;
use fxhash::{FxHashMap, FxHashSet};
use ring::rand::SystemRandom;

use crate::{
    change::Lamport,
    configure::SecureRandomGenerator,
    container::{registry::ContainerIdx, ContainerID},
    event::Diff,
    id::{PeerID, ID},
    version::{Frontiers, ImVersionVector},
};

use super::arena::ReadonlyArena;

mod list_state;
mod map_state;
mod text_state;

use list_state::ListState;
use map_state::MapState;
use text_state::TextState;

#[derive(Clone)]
pub struct AppState {
    pub(super) peer: PeerID,

    pub(super) frontiers: Frontiers,
    state: FxHashMap<ContainerIdx, State>,
    arena: Option<ReadonlyArena>,

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

pub struct AppStateDiff {
    pub changes: Vec<ContainerStateDiff>,
    pub new_arena: ReadonlyArena,
    pub new_frontiers: Frontiers,
    pub new_vv: ImVersionVector,
}

pub struct ContainerStateDiff {
    pub idx: ContainerID,
    pub diff: Diff,
}

impl AppState {
    pub fn new() -> Self {
        let peer = SystemRandom::new().next_u64();
        Self {
            peer,
            frontiers: Frontiers::default(),
            state: FxHashMap::default(),
            arena: None,
            in_txn: false,
            changed_in_txn: FxHashSet::default(),
        }
    }

    pub fn set_peer_id(&mut self, peer: PeerID) {
        self.peer = peer;
    }

    pub fn apply_diff(&mut self, _diff: &AppStateDiff) {
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

    pub(crate) fn commit_txn(&mut self) {
        for container_idx in std::mem::take(&mut self.changed_in_txn) {
            self.state.get_mut(&container_idx).unwrap().commit_txn();
        }

        self.in_txn = false;
    }

    pub(super) fn get_state_mut(&mut self, idx: ContainerIdx) -> Option<&mut State> {
        self.state.get_mut(&idx)
    }
}

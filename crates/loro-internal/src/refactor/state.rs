use enum_dispatch::enum_dispatch;
use fxhash::{FxHashMap, FxHashSet};
use ring::rand::SystemRandom;

use crate::{
    change::Lamport,
    configure::SecureRandomGenerator,
    container::registry::ContainerIdx,
    event::Diff,
    id::{Counter, PeerID},
    op::RawOp,
    version::Frontiers,
    ContainerType, LoroValue,
};

mod list_state;
mod map_state;
mod text_state;

use list_state::List;
use map_state::Map;
use text_state::Text;

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
    fn apply_diff(&mut self, diff: &Diff, arena: &SharedArena);
    fn apply_op(&mut self, op: RawOp);

    /// Start a transaction
    ///
    /// The transaction may be aborted later, then all the ops during this transaction need to be undone.
    fn start_txn(&mut self);
    fn abort_txn(&mut self);
    fn commit_txn(&mut self);

    fn get_value(&self) -> LoroValue;
}

#[enum_dispatch(ContainerState)]
#[derive(Clone)]
pub enum State {
    List,
    Map,
    Text,
}

#[derive(Debug)]
pub struct ContainerStateDiff {
    pub idx: ContainerIdx,
    pub diff: Diff,
}

pub struct AppStateDiff<'a> {
    pub(crate) diff: &'a [ContainerStateDiff],
    pub(crate) frontiers: &'a Frontiers,
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

    pub fn apply_diff(&mut self, AppStateDiff { diff, frontiers }: AppStateDiff) {
        for diff in diff {
            let state = self.states.entry(diff.idx).or_insert_with(|| {
                let id = self.arena.get_container_id(diff.idx).unwrap();
                create_state(id.container_type())
            });

            if self.in_txn {
                state.start_txn();
                self.changed_in_txn.insert(diff.idx);
            }

            state.apply_diff(&diff.diff, &self.arena);
        }

        self.frontiers = frontiers.clone();
    }

    pub fn apply_local_op(&mut self, op: RawOp) {
        let state = self.states.entry(op.container).or_insert_with(|| {
            let id = self.arena.get_container_id(op.container).unwrap();
            create_state(id.container_type())
        });

        if self.in_txn {
            state.start_txn();
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

    pub(crate) fn get_value_by_idx(&self, container_idx: ContainerIdx) -> LoroValue {
        self.states.get(&container_idx).unwrap().get_value()
    }

    pub(super) fn is_in_txn(&self) -> bool {
        self.in_txn
    }
}

pub fn create_state(kind: ContainerType) -> State {
    match kind {
        ContainerType::Text => State::Text(Text::new()),
        ContainerType::Map => State::Map(Map::new()),
        ContainerType::List => State::List(List::new()),
    }
}

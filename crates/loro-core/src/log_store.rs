use fxhash::FxHashMap;
use ring::rand::SystemRandom;
use rle::{HasLength, RleVec};
use smallvec::SmallVec;
use string_cache::{Atom, DefaultAtom, EmptyStaticAtomSet};

use crate::{
    change::{Change, ChangeMergeCfg},
    configure::Configure,
    container::{Container, ContainerID, ContainerManager},
    id::{ClientID, Counter},
    id_span::IdSpan,
    Lamport, Op, Timestamp, ID,
};
const YEAR: u64 = 365 * 24 * 60 * 60;
const MONTH: u64 = 30 * 24 * 60 * 60;

pub struct GcConfig {
    pub gc: bool,
    pub interval: u64,
}

impl Default for GcConfig {
    fn default() -> Self {
        GcConfig {
            gc: false,
            interval: 6 * MONTH,
        }
    }
}

/// Entry of the loro inner state.
pub struct LogStore {
    changes: FxHashMap<ClientID, RleVec<Change, ChangeMergeCfg>>,
    cfg: Configure,
    latest_lamport: Lamport,
    latest_timestamp: Timestamp,
    pub(crate) this_client_id: ClientID,
    frontier: SmallVec<[ID; 2]>,

    /// CRDT container manager
    container: ContainerManager,
}

impl LogStore {
    pub fn new(mut cfg: Configure, client_id: Option<ClientID>) -> Self {
        let this_client_id = client_id.unwrap_or_else(|| cfg.rand.next_u64());
        Self {
            cfg,
            this_client_id,
            changes: FxHashMap::default(),
            latest_lamport: 0,
            latest_timestamp: 0,
            container: Default::default(),
            frontier: Default::default(),
        }
    }

    pub fn lookup_change(&self, id: ID) -> Option<&Change> {
        self.changes
            .get(&id.client_id)
            .map(|changes| changes.get(id.counter as usize).unwrap().element)
    }

    pub fn next_lamport(&self) -> Lamport {
        self.latest_lamport + 1
    }

    pub fn append_local_ops(&mut self, ops: Vec<Op>) {
        let lamport = self.next_lamport();
        let timestamp = (self.cfg.get_time)();
        let id = ID {
            client_id: self.this_client_id,
            counter: self.get_next_counter(self.this_client_id),
        };
        let mut change = Change {
            id,
            ops: ops.into(),
            deps: std::mem::take(&mut self.frontier),
            lamport,
            timestamp,
            freezed: false,
        };

        change.deps.push(ID::new(
            self.this_client_id,
            id.counter + change.len() as Counter - 1,
        ));
        self.latest_lamport = lamport + change.len() as u32 - 1;
        self.latest_timestamp = timestamp;
        self.changes
            .entry(self.this_client_id)
            .or_insert_with(RleVec::new)
            .push(change);
    }

    pub fn apply_remote_change(&mut self, mut change: Change) {
        change.freezed = true;
        if self.includes(change.last_id()) {
            return;
        }

        for dep in &change.deps {
            if !self.includes(*dep) {
                unimplemented!("need impl pending changes");
            }
        }

        self.frontier = self
            .frontier
            .iter()
            .filter(|x| !change.deps.contains(x))
            .copied()
            .collect();
        self.frontier.push(change.last_id());

        if change.last_lamport() > self.latest_lamport {
            self.latest_lamport = change.last_lamport();
        }

        if change.timestamp > self.latest_timestamp {
            self.latest_timestamp = change.timestamp;
        }

        for op in change.ops.iter() {
            self.apply_remote_op(op);
        }

        self.push_change(change);
    }

    #[inline]
    fn push_change(&mut self, change: Change) {
        self.changes
            .entry(change.id.client_id)
            .or_insert_with(RleVec::new)
            .push(change);
    }

    /// this function assume op is not included in the log, and its deps are included.
    fn apply_remote_op(&mut self, op: &Op) {
        todo!()
    }

    pub fn includes(&self, id: ID) -> bool {
        self.changes
            .get(&id.client_id)
            .map_or(0, |changes| changes.len())
            > id.counter as usize
    }

    fn get_next_counter(&self, client_id: ClientID) -> Counter {
        self.changes
            .get(&client_id)
            .map(|changes| changes.len())
            .unwrap_or(0) as Counter
    }
}

impl Default for LogStore {
    fn default() -> Self {
        Self::new(
            Configure {
                change: Default::default(),
                gc: Default::default(),
                get_time: || 0,
                rand: Box::new(SystemRandom::new()),
            },
            None,
        )
    }
}

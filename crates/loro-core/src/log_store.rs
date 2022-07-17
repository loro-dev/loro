use fxhash::FxHashMap;
use rle::RleVec;
use smallvec::SmallVec;
use string_cache::{Atom, DefaultAtom, EmptyStaticAtomSet};

use crate::{
    change::{Change, ChangeMergeCfg},
    container::{Container, ContainerID},
    id::ClientID,
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

pub struct Configure {
    pub change: ChangeMergeCfg,
    pub gc: GcConfig,
    get_time: fn() -> Timestamp,
}

pub struct LogStore {
    ops: FxHashMap<ClientID, RleVec<Change, ChangeMergeCfg>>,
    cfg: Configure,
    latest_lamport: Lamport,
    latest_timestamp: Lamport,
    pub(crate) this_client_id: ClientID,
    frontier: SmallVec<[ID; 2]>,

    containers: FxHashMap<ContainerID, Box<dyn Container>>,
}

impl LogStore {
    pub fn new(cfg: Configure, client_id: Option<ClientID>) -> Self {
        Self {
            cfg,
            ops: FxHashMap::default(),
            latest_lamport: 0,
            latest_timestamp: 0,
            containers: Default::default(),
            frontier: Default::default(),
            // TODO: or else random id
            this_client_id: client_id.unwrap_or_else(|| 0),
        }
    }

    pub fn lookup_change(&self, id: ID) -> Option<&Change> {
        self.ops
            .get(&id.client_id)
            .map(|changes| changes.get(id.counter as usize).unwrap().element)
    }

    pub fn next_lamport(&self) -> Lamport {
        self.latest_lamport + 1
    }

    pub fn append_local_change(&mut self, change: Change) {
        self.ops
            .entry(change.id.client_id)
            .or_insert(RleVec::new())
            .push(change);
        todo!("set frontier timestamp and lamport");
        todo!("frontier of the same client can be dropped, if only itself is included");
    }

    pub fn append_local_op(&mut self, op: Op) {
        // TODO: we can check change mergeable before append
        let change = Change {
            id: op.id,
            ops: vec![op].into(),
            deps: self.frontier.clone(),
            lamport: self.next_lamport(),
            timestamp: (self.cfg.get_time)(),
            freezed: false,
        };
        self.append_local_change(change);
    }
}

impl Default for LogStore {
    fn default() -> Self {
        Self::new(
            Configure {
                change: Default::default(),
                gc: Default::default(),
                get_time: || 0,
            },
            None,
        )
    }
}

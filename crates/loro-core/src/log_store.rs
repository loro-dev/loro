use fxhash::FxHashMap;
use rle::RleVec;
use string_cache::{Atom, DefaultAtom, EmptyStaticAtomSet};

use crate::{
    change::{Change, ChangeMergeCfg},
    container::Container,
    id::ClientID,
    Lamport, ID,
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
}

pub struct LogStore {
    ops: FxHashMap<ClientID, RleVec<Change, ChangeMergeCfg>>,
    cfg: Configure,
    latest_lamport: Lamport,
    latest_timestamp: Lamport,

    containers: FxHashMap<ID, Box<dyn Container>>,
}

impl LogStore {
    pub fn new(cfg: Configure) -> Self {
        Self {
            cfg,
            ops: FxHashMap::default(),
            latest_lamport: 0,
            latest_timestamp: 0,
            containers: Default::default(),
        }
    }

    pub fn lookup_change(&self, id: ID) -> Option<&Change> {
        self.ops
            .get(&id.client_id)
            .map(|changes| changes.get(id.counter as usize).unwrap().element)
    }
}

impl Default for LogStore {
    fn default() -> Self {
        Self::new(Configure {
            change: Default::default(),
            gc: Default::default(),
        })
    }
}

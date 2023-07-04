mod dag;

use std::mem::take;
use std::sync::atomic::{self, AtomicBool};
use std::sync::Mutex;

use fxhash::FxHashMap;
use rle::{HasIndex, HasLength, Mergable, RleVec, Sliceable};
use smallvec::SmallVec;

use crate::change::{Lamport, Timestamp};
use crate::dag::{Dag, DagNode};
use crate::event::Diff;
use crate::id::{ClientID, Counter, ID};
use crate::log_store::ClientChanges;
use crate::span::{HasId, HasLamport};
use crate::version::{Frontiers, VersionVector};

use super::diff_calc::DiffCalculator;

/// [OpLog] store all the ops i.e. the history.
/// It allows multiple [AppState] to attach to it.
/// So you can derive different versions of the state from the [OpLog].
/// It allows us to build a version control system.
///
pub struct OpLog {
    pub(crate) dag: AppDag,
    pub(crate) changes: ClientChanges,
    pub(crate) latest_lamport: Lamport,
    pub(crate) latest_timestamp: Timestamp,
    cache_diff: AtomicBool,
    diff_calculator: Mutex<Option<DiffCalculator>>,
}

/// [AppDag] maintains the causal graph of the app.
/// It's faster to answer the question like what's the LCA version
#[derive(Debug, Clone, Default)]
pub struct AppDag {
    map: FxHashMap<ClientID, RleVec<[AppDagNode; 1]>>,
    frontiers: Frontiers,
    vv: VersionVector,
}

#[derive(Debug, Clone)]
pub struct AppDagNode {
    client: ClientID,
    cnt: Counter,
    lamport: Lamport,
    parents: SmallVec<[ID; 2]>,
    len: usize,
}

impl Clone for OpLog {
    fn clone(&self) -> Self {
        Self {
            dag: self.dag.clone(),
            changes: self.changes.clone(),
            latest_lamport: self.latest_lamport,
            latest_timestamp: self.latest_timestamp,
            cache_diff: AtomicBool::new(false),
            diff_calculator: Mutex::new(None),
        }
    }
}

impl std::fmt::Debug for OpLog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpLog")
            .field("dag", &self.dag)
            .field("changes", &self.changes)
            .field("latest_lamport", &self.latest_lamport)
            .field("latest_timestamp", &self.latest_timestamp)
            .field("cache_diff", &self.cache_diff)
            .finish()
    }
}

impl OpLog {
    pub fn diff(&self, before: &VersionVector, after: &VersionVector) -> Vec<Diff> {
        let diff = take(&mut *self.diff_calculator.lock().unwrap()).unwrap_or_default();
        let ans = diff.calc(self, before, after);
        if self.cache_diff.load(atomic::Ordering::Relaxed) {
            self.diff_calculator.lock().unwrap().replace(diff);
        }

        ans
    }

    pub fn toggle_fast_diff_mode(&self, on: bool) {
        self.cache_diff.store(on, atomic::Ordering::Relaxed)
    }

    pub fn new() -> Self {
        Self {
            dag: AppDag::default(),
            changes: ClientChanges::default(),
            latest_lamport: Lamport::default(),
            latest_timestamp: Timestamp::default(),
            cache_diff: AtomicBool::new(false),
            diff_calculator: Mutex::new(None),
        }
    }
}

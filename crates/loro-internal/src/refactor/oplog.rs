mod dag;

use fxhash::FxHashMap;
use rle::{HasIndex, HasLength, Mergable, RleVec, Sliceable};
use smallvec::SmallVec;

use crate::change::{Change, Lamport, Timestamp};
use crate::dag::{Dag, DagNode};
use crate::id::{ClientID, Counter, ID};
use crate::log_store::ClientChanges;
use crate::span::{HasId, HasLamport};
use crate::version::{Frontiers, VersionVector};

/// [OpLog] store all the ops i.e. the history.
/// It allows multiple [AppState] to attach to it.
/// So you can derive different versions of the state from the [OpLog].
/// It allows us to build a version control system.
///
#[derive(Debug, Clone)]
pub struct OpLog {
    pub(crate) dag: AppDag,
    pub(crate) changes: ClientChanges,
    pub(crate) latest_lamport: Lamport,
    pub(crate) latest_timestamp: Timestamp,
}

/// [AppDag] maintains the causal graph of the app.
/// It's faster to answer the question like what's the LCA version
#[derive(Debug, Clone)]
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

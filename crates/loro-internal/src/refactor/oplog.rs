pub(crate) mod dag;

use fxhash::FxHashMap;
use rle::RleVec;
use smallvec::SmallVec;
// use tabled::measurment::Percent;

use crate::change::{Change, Lamport, Timestamp};
use crate::id::{Counter, PeerID, ID};
use crate::log_store::ClientChanges;
use crate::op::RemoteOp;
use crate::version::{Frontiers, ImVersionVector, VersionVector};
use crate::LoroError;

use super::arena::SharedArena;

/// [OpLog] store all the ops i.e. the history.
/// It allows multiple [AppState] to attach to it.
/// So you can derive different versions of the state from the [OpLog].
/// It allows us to build a version control system.
///
pub struct OpLog {
    pub(crate) dag: AppDag,
    pub(super) arena: SharedArena,
    pub(crate) changes: ClientChanges,
    pub(crate) latest_lamport: Lamport,
    pub(crate) latest_timestamp: Timestamp,
    /// Pending changes that haven't been applied to the dag.
    /// A change can be imported only when all its deps are already imported.
    /// Key is the ID of the missing dep
    pending_changes: FxHashMap<ID, Vec<Change>>,
}

/// [AppDag] maintains the causal graph of the app.
/// It's faster to answer the question like what's the LCA version
#[derive(Debug, Clone, Default)]
pub struct AppDag {
    map: FxHashMap<PeerID, RleVec<[AppDagNode; 1]>>,
    frontiers: Frontiers,
    vv: VersionVector,
}

#[derive(Debug, Clone)]
pub struct AppDagNode {
    client: PeerID,
    cnt: Counter,
    lamport: Lamport,
    parents: SmallVec<[ID; 2]>,
    vv: ImVersionVector,
    len: usize,
}

impl Clone for OpLog {
    fn clone(&self) -> Self {
        Self {
            dag: self.dag.clone(),
            arena: Default::default(),
            changes: self.changes.clone(),
            latest_lamport: self.latest_lamport,
            latest_timestamp: self.latest_timestamp,
            pending_changes: Default::default(),
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
            .finish()
    }
}

impl OpLog {
    pub fn new() -> Self {
        Self {
            dag: AppDag::default(),
            arena: Default::default(),
            changes: ClientChanges::default(),
            latest_lamport: Lamport::default(),
            latest_timestamp: Timestamp::default(),
            pending_changes: Default::default(),
        }
    }

    /// Import a change.
    ///
    /// Pending changes that haven't been applied to the dag.
    /// A change can be imported only when all its deps are already imported.
    /// Key is the ID of the missing dep
    ///
    /// # Err
    ///
    /// Return Err(LoroError::UsedOpID) when the change's id is occupied
    pub fn import_change(&mut self, change: Change) -> Result<(), LoroError> {
        self.check_id_valid(change.id)?;
        if let Err(id) = self.check_deps(&change.deps) {
            self.pending_changes.entry(id).or_default().push(change);
            return Ok(());
        }

        self.changes.entry(change.id.peer).or_default().push(change);
        Ok(())
    }

    fn check_id_valid(&self, id: ID) -> Result<(), LoroError> {
        let cur_end = self.dag.vv.get(&id.peer).cloned().unwrap_or(0);
        if cur_end > id.counter {
            return Err(LoroError::UsedOpID { id });
        }

        Ok(())
    }

    fn check_deps(&self, deps: &Frontiers) -> Result<(), ID> {
        for dep in deps.iter() {
            if !self.dag.vv.includes_id(*dep) {
                return Err(*dep);
            }
        }

        Ok(())
    }

    fn convert_change(&mut self, change: Change<RemoteOp>) -> Change {
        let mut ops = RleVec::new();
        for op in change.ops {
            for content in op.contents.into_iter() {
                ops.push(
                    self.arena
                        .convert_single_op(&op.container, op.counter, content),
                );
            }
        }

        Change {
            ops,
            id: change.id,
            deps: change.deps,
            lamport: change.lamport,
            timestamp: change.timestamp,
        }
    }

    pub fn get_timestamp(&self) -> Timestamp {
        // TODO: get timestamp
        0
    }

    pub fn next_lamport(&self) -> Lamport {
        self.latest_lamport + 1
    }

    pub fn next_id(&self, peer: PeerID) -> ID {
        let cnt = self.dag.vv.get(&peer).copied().unwrap_or(0);
        ID::new(peer, cnt)
    }
}

mod dag;

use fxhash::FxHashMap;
use rle::{HasIndex, HasLength, Mergable, RleVec, Sliceable};
use smallvec::SmallVec;

use crate::change::{Change, Lamport, Timestamp};
use crate::container::list::list_op::{InnerListOp, ListOp};
use crate::container::map::InnerMapSet;
use crate::dag::{Dag, DagNode};
use crate::id::{Counter, PeerID, ID};
use crate::log_store::ClientChanges;
use crate::op::{Op, RemoteOp};
use crate::span::{HasId, HasLamport};
use crate::text::text_content::SliceRange;
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
    pub fn import_change(&mut self, change: Change<RemoteOp>) -> Result<(), LoroError> {
        self.check_id_valid(change.id)?;
        let change = self.convert_change(change);
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
        Change {
            id: change.id,
            deps: change.deps,
            lamport: change.lamport,
            timestamp: change.timestamp,
            ops: change
                .ops
                .into_iter()
                .flat_map(|op| self.convert_op(op))
                .collect(),
        }
    }

    fn convert_op<'a, 'b>(&'a mut self, op: RemoteOp<'b>) -> SmallVec<[Op; 3]> {
        let container = self.arena.register_container(&op.container);
        let counter = op.counter;
        op.contents
            .into_iter()
            .map(move |content| match content {
                crate::op::RemoteContent::Map(map) => {
                    let value = self.arena.alloc_value(map.value) as u32;
                    Op {
                        counter,
                        container,
                        content: crate::op::InnerContent::Map(InnerMapSet {
                            key: map.key,
                            value,
                        }),
                    }
                }
                crate::op::RemoteContent::List(list) => match list {
                    ListOp::Insert { slice, pos } => match slice {
                        crate::text::text_content::ListSlice::RawData(values) => {
                            let (from, to) = self.arena.alloc_values(values.iter().cloned());
                            Op {
                                counter,
                                container,
                                content: crate::op::InnerContent::List(InnerListOp::Insert {
                                    slice: SliceRange::from(from as u32..to as u32),
                                    pos,
                                }),
                            }
                        }
                        crate::text::text_content::ListSlice::RawStr(str) => {
                            let bytes = self.arena.alloc_str(&str);
                            Op {
                                counter,
                                container,
                                content: crate::op::InnerContent::List(InnerListOp::Insert {
                                    slice: SliceRange::from(bytes.start as u32..bytes.end as u32),
                                    pos,
                                }),
                            }
                        }
                        crate::text::text_content::ListSlice::Unknown(u) => Op {
                            counter,
                            container,
                            content: crate::op::InnerContent::List(InnerListOp::Insert {
                                slice: SliceRange::new_unknown(u as u32),
                                pos,
                            }),
                        },
                    },
                    ListOp::Delete(span) => Op {
                        counter,
                        container,
                        content: crate::op::InnerContent::List(InnerListOp::Delete(span)),
                    },
                },
            })
            .collect()
    }
}

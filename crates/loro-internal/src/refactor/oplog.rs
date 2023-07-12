pub(crate) mod dag;

use std::cell::RefCell;
use std::rc::Rc;

use debug_log::debug_dbg;
use fxhash::FxHashMap;
use rle::{HasLength, RleVec};
// use tabled::measurment::Percent;

use crate::change::{Change, Lamport, Timestamp};
use crate::container::list::list_op;
use crate::dag::DagUtils;
use crate::id::{Counter, PeerID, ID};
use crate::log_store::{decode_oplog, encode_oplog};
use crate::log_store::{ClientChanges, RemoteClientChanges};
use crate::op::{RawOpContent, RemoteOp};
use crate::span::{HasCounterSpan, HasIdSpan, HasLamportSpan};
use crate::version::{Frontiers, ImVersionVector, VersionVector};
use crate::LoroError;

use super::arena::SharedArena;

/// [OpLog] store all the ops i.e. the history.
/// It allows multiple [AppState] to attach to it.
/// So you can derive different versions of the state from the [OpLog].
/// It allows us to build a version control system.
///
/// The causal graph should always be a DAG and complete. So we can always find the LCA.
/// If deps are missing, we can't import the change. It will be put into the `pending_changes`.
pub struct OpLog {
    pub(crate) dag: AppDag,
    pub(crate) arena: SharedArena,
    pub(crate) changes: ClientChanges,
    pub(crate) next_lamport: Lamport,
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
    peer: PeerID,
    cnt: Counter,
    lamport: Lamport,
    deps: Frontiers,
    vv: ImVersionVector,
    len: usize,
}

impl Clone for OpLog {
    fn clone(&self) -> Self {
        Self {
            dag: self.dag.clone(),
            arena: Default::default(),
            changes: self.changes.clone(),
            next_lamport: self.next_lamport,
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
            .field("next_lamport", &self.next_lamport)
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
            next_lamport: 0,
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
    pub fn import_local_change(&mut self, change: Change) -> Result<(), LoroError> {
        self.check_id_valid(change.id)?;
        if let Err(id) = self.check_deps(&change.deps) {
            self.pending_changes.entry(id).or_default().push(change);
            return Err(LoroError::DecodeError(
                format!("Missing dep {:?}", id).into_boxed_str(),
            ));
        }

        self.dag.vv.extend_to_include_last_id(change.id_last());
        self.next_lamport = self.next_lamport.max(change.lamport_end());
        self.latest_timestamp = self.latest_timestamp.max(change.timestamp);
        self.dag.frontiers.retain_non_included(&change.deps);
        self.dag.frontiers.filter_included(change.id);
        self.dag.frontiers.push(change.id_last());
        let vv = self.dag.frontiers_to_im_vv(&change.deps);
        let len = change.content_len();
        self.dag
            .map
            .entry(change.id.peer)
            .or_default()
            .push(AppDagNode {
                vv,
                peer: change.id.peer,
                cnt: change.id.counter,
                lamport: change.lamport,
                deps: change.deps.clone(),
                len,
            });
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
        self.next_lamport
    }

    pub fn next_id(&self, peer: PeerID) -> ID {
        let cnt = self.dag.vv.get(&peer).copied().unwrap_or(0);
        ID::new(peer, cnt)
    }

    pub(crate) fn vv(&self) -> &VersionVector {
        &self.dag.vv
    }

    pub(crate) fn frontiers(&self) -> &Frontiers {
        &self.dag.frontiers
    }

    pub(crate) fn export_changes_from(&self, from: &VersionVector) -> RemoteClientChanges {
        let mut changes = RemoteClientChanges::default();
        for (&peer, &cnt) in self.vv().iter() {
            let start_cnt = from.get(&peer).copied().unwrap_or(0);
            if cnt <= start_cnt {
                continue;
            }

            let mut temp = Vec::new();
            if let Some(peer_changes) = self.changes.get(&peer) {
                if let Some(result) = peer_changes.get_by_atom_index(start_cnt) {
                    for change in &peer_changes.vec()[result.merged_index..] {
                        temp.push(self.convert_change_to_remote(change))
                    }
                }
            }

            if !temp.is_empty() {
                changes.insert(peer, temp);
            }
        }

        debug_dbg!(&changes);

        changes
    }

    pub(crate) fn get_change_since(&self, id: ID) -> Vec<Change> {
        let mut changes = Vec::new();
        if let Some(peer_changes) = self.changes.get(&id.peer) {
            if let Some(result) = peer_changes.get_by_atom_index(id.counter) {
                for change in &peer_changes.vec()[result.merged_index..] {
                    changes.push(change.clone())
                }
            }
        }

        changes
    }

    fn convert_change_to_remote(&self, change: &Change) -> Change<RemoteOp> {
        let mut ops = RleVec::new();
        for op in change.ops.iter() {
            let raw_op = self.local_op_to_remote(op);
            ops.push(raw_op);
        }

        Change {
            ops,
            id: change.id,
            deps: change.deps.clone(),
            lamport: change.lamport,
            timestamp: change.timestamp,
        }
    }

    fn convert_change_to_local(&self, change: Change<RemoteOp>) -> Change {
        let mut ops = RleVec::new();
        for op in change.ops {
            for content in op.contents.into_iter() {
                let op = self
                    .arena
                    .convert_single_op(&op.container, op.counter, content);
                ops.push(op);
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

    pub(crate) fn local_op_to_remote(&self, op: &crate::op::Op) -> RemoteOp<'_> {
        let container = self.arena.get_container_id(op.container).unwrap();
        let mut contents = RleVec::new();
        match &op.content {
            crate::op::InnerContent::List(list) => match list {
                list_op::InnerListOp::Insert { slice, pos } => {
                    contents.push(RawOpContent::List(list_op::ListOp::Insert {
                        slice: crate::container::text::text_content::ListSlice::RawBytes(
                            self.arena
                                .slice_bytes(slice.0.start as usize..slice.0.end as usize),
                        ),
                        pos: *pos,
                    }))
                }
                list_op::InnerListOp::Delete(del) => {
                    contents.push(RawOpContent::List(list_op::ListOp::Delete(*del)))
                }
            },
            crate::op::InnerContent::Map(map) => {
                let value = self.arena.get_value(map.value as usize);
                contents.push(RawOpContent::Map(crate::container::map::MapSet {
                    key: map.key.clone(),
                    value: value.unwrap_or(crate::LoroValue::Null), // TODO: decide map delete value
                }))
            }
        };

        RemoteOp {
            container,
            contents,
            counter: op.counter,
        }
    }

    pub(crate) fn import_remote_changes(
        &mut self,
        changes: RemoteClientChanges,
    ) -> Result<(), LoroError> {
        let len = changes.iter().fold(0, |last, this| last + this.1.len());
        let mut change_causal_arr = Vec::with_capacity(len);
        for (peer, changes) in changes {
            let cur_end_cnt = self.changes.get(&peer).map(|x| x.atom_len()).unwrap_or(0);
            for change in changes {
                if change.id.counter < cur_end_cnt {
                    continue;
                }

                let change = self.convert_change_to_local(change);
                change_causal_arr.push(change);
            }
        }

        // TODO: Perf
        change_causal_arr.sort_by_key(|x| x.lamport);
        debug_dbg!(&change_causal_arr);
        for change in change_causal_arr {
            debug_dbg!(&change);
            self.import_local_change(change)?;
        }

        Ok(())
    }

    /// lookup change by id.
    ///
    /// if id does not included in this oplog, return None
    pub(crate) fn lookup_change(&self, id: ID) -> Option<&Change> {
        self.changes.get(&id.peer).and_then(|changes| {
            // Because get_by_atom_index would return Some if counter is at the end,
            // we cannot use it directly.
            // TODO: maybe we should refactor this
            if id.counter <= changes.last().unwrap().id_last().counter {
                Some(changes.get_by_atom_index(id.counter).unwrap().element)
            } else {
                None
            }
        })
    }

    pub fn export_from(&self, vv: &VersionVector) -> Vec<u8> {
        encode_oplog(self, crate::EncodeMode::Auto(vv.clone()))
    }

    pub fn decode(&mut self, data: &[u8]) -> Result<(), LoroError> {
        decode_oplog(self, data)
    }

    /// iterates over all changes between LCA(common ancestors) to the merged version of (`from` and `to`) causally
    ///
    /// Tht iterator will include a version vector when the change is applied
    ///
    /// returns: (common_ancestor_vv, iterator)
    ///
    /// If frontiers are provided, it will be faster (because we don't need to calculate it from version vector
    pub(crate) fn iter_from_lca_causally(
        &self,
        from: &VersionVector,
        from_frontiers: Option<&Frontiers>,
        to: &VersionVector,
        to_frontiers: Option<&Frontiers>,
    ) -> (
        VersionVector,
        impl Iterator<Item = (&Change, Rc<RefCell<VersionVector>>)>,
    ) {
        debug_log::group!("iter_from_lca_causally");
        let mut merged_vv = from.clone();
        merged_vv.merge(to);
        let from_frontiers_inner;
        let to_frontiers_inner;

        let from_frontiers = match from_frontiers {
            Some(f) => f,
            None => {
                from_frontiers_inner = Some(from.to_frontiers(&self.dag));
                from_frontiers_inner.as_ref().unwrap()
            }
        };

        let to_frontiers = match to_frontiers {
            Some(t) => t,
            None => {
                to_frontiers_inner = Some(to.to_frontiers(&self.dag));
                to_frontiers_inner.as_ref().unwrap()
            }
        };

        let common_ancestors = self.dag.find_common_ancestor(from_frontiers, to_frontiers);
        let common_ancestors_vv = self.dag.frontiers_to_vv(&common_ancestors);
        // go from lca to merged_vv
        let diff = common_ancestors_vv.diff(&merged_vv).right;
        let mut iter = self.dag.iter_causal(&common_ancestors, diff);
        let mut node = iter.next();
        let mut cur_cnt = 0;
        // reuse the allocated memory in merged_vv...
        let vv = Rc::new(RefCell::new(merged_vv));
        (
            common_ancestors_vv,
            std::iter::from_fn(move || {
                if let Some(inner) = &node {
                    let mut inner_vv = vv.borrow_mut();
                    inner_vv.clear();
                    inner_vv.extend_to_include_vv(inner.data.vv.iter());
                    let peer = inner.data.peer;
                    let cnt = inner.data.cnt.max(cur_cnt);
                    let end = inner.data.cnt + inner.data.len as Counter;
                    let change = self
                        .changes
                        .get(&peer)
                        .and_then(|x| x.get_by_atom_index(cnt).map(|x| x.element))
                        .unwrap();

                    if change.ctr_end() < end {
                        cur_cnt = change.ctr_end();
                    } else {
                        node = iter.next();
                        cur_cnt = 0;
                    }

                    inner_vv.extend_to_include_end_id(change.id);
                    // debug_log::debug_dbg!(&change, &inner_vv);
                    Some((change, vv.clone()))
                } else {
                    debug_log::group_end!();
                    None
                }
            }),
        )
    }
}

impl Default for OpLog {
    fn default() -> Self {
        Self::new()
    }
}

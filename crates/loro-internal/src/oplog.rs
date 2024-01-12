pub(crate) mod dag;
mod pending_changes;

use std::borrow::Cow;
use std::cell::RefCell;
use std::cmp::Ordering;
use std::mem::take;
use std::rc::Rc;

use fxhash::FxHashMap;
use loro_common::{HasCounter, HasId};
use rle::{HasLength, RleCollection, RlePush, RleVec, Sliceable};
use smallvec::SmallVec;
// use tabled::measurment::Percent;

use crate::change::{Change, Lamport, Timestamp};
use crate::container::list::list_op;
use crate::dag::{Dag, DagUtils};
use crate::encoding::ParsedHeaderAndBody;
use crate::encoding::{decode_oplog, encode_oplog, EncodeMode};
use crate::group::OpGroups;
use crate::id::{Counter, PeerID, ID};
use crate::op::{ListSlice, RawOpContent, RemoteOp};
use crate::span::{HasCounterSpan, HasIdSpan, HasLamportSpan};
use crate::version::{Frontiers, ImVersionVector, VersionVector};
use crate::LoroError;

type ClientChanges = FxHashMap<PeerID, Vec<Change>>;
use self::pending_changes::PendingChanges;

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
    changes: ClientChanges,
    pub(crate) op_groups: OpGroups,
    /// **lamport starts from 0**
    pub(crate) next_lamport: Lamport,
    pub(crate) latest_timestamp: Timestamp,
    /// Pending changes that haven't been applied to the dag.
    /// A change can be imported only when all its deps are already imported.
    /// Key is the ID of the missing dep
    pub(crate) pending_changes: PendingChanges,
    /// Whether we are importing a batch of changes.
    /// If so the Dag's frontiers won't be updated until the batch is finished.
    pub(crate) batch_importing: bool,
}

/// [AppDag] maintains the causal graph of the app.
/// It's faster to answer the question like what's the LCA version
#[derive(Debug, Clone, Default)]
pub struct AppDag {
    pub(crate) map: FxHashMap<PeerID, Vec<AppDagNode>>,
    pub(crate) frontiers: Frontiers,
    pub(crate) vv: VersionVector,
}

#[derive(Debug, Clone)]
pub struct AppDagNode {
    pub(crate) peer: PeerID,
    pub(crate) cnt: Counter,
    pub(crate) lamport: Lamport,
    pub(crate) deps: Frontiers,
    pub(crate) vv: ImVersionVector,
    /// A flag indicating whether any other nodes depend on this node.
    /// The calculation of frontiers is based on this value.
    pub(crate) has_succ: bool,
    pub(crate) len: usize,
}

impl Clone for OpLog {
    fn clone(&self) -> Self {
        Self {
            dag: self.dag.clone(),
            arena: Default::default(),
            changes: self.changes.clone(),
            op_groups: self.op_groups.clone(),
            next_lamport: self.next_lamport,
            latest_timestamp: self.latest_timestamp,
            pending_changes: Default::default(),
            batch_importing: false,
        }
    }
}

impl AppDag {
    pub fn get_mut(&mut self, id: ID) -> Option<&mut AppDagNode> {
        let ID {
            peer: client_id,
            counter,
        } = id;
        self.map.get_mut(&client_id).and_then(|rle| {
            if counter >= rle.sum_atom_len() {
                return None;
            }

            let index = rle.search_atom_index(counter);
            Some(&mut rle[index])
        })
    }

    pub(crate) fn refresh_frontiers(&mut self) {
        self.frontiers = self
            .map
            .iter()
            .filter(|(_, vec)| {
                if vec.is_empty() {
                    return false;
                }

                !vec.last().unwrap().has_succ
            })
            .map(|(peer, vec)| ID::new(*peer, vec.last().unwrap().ctr_last()))
            .collect();
    }

    /// If the lamport of change can be calculated, return Ok, otherwise, Err
    pub(crate) fn calc_unknown_lamport_change(&self, change: &mut Change) -> Result<(), ()> {
        for dep in change.deps.iter() {
            match self.get_lamport(dep) {
                Some(lamport) => {
                    change.lamport = change.lamport.max(lamport + 1);
                }
                None => return Err(()),
            }
        }
        Ok(())
    }
}

impl std::fmt::Debug for OpLog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpLog")
            .field("dag", &self.dag)
            .field("changes", &self.changes)
            .field("pending_changes", &self.pending_changes)
            .field("next_lamport", &self.next_lamport)
            .field("latest_timestamp", &self.latest_timestamp)
            .finish()
    }
}

pub(crate) struct EnsureChangeDepsAreAtTheEnd;

impl OpLog {
    #[inline]
    pub fn new() -> Self {
        Self {
            dag: AppDag::default(),
            arena: Default::default(),
            changes: ClientChanges::default(),
            op_groups: OpGroups::default(),
            next_lamport: 0,
            latest_timestamp: Timestamp::default(),
            pending_changes: Default::default(),
            batch_importing: false,
        }
    }

    #[inline]
    pub fn new_with_arena(arena: SharedArena) -> Self {
        Self {
            dag: AppDag::default(),
            arena,
            next_lamport: 0,
            ..Default::default()
        }
    }

    #[inline]
    pub fn latest_timestamp(&self) -> Timestamp {
        self.latest_timestamp
    }

    #[inline]
    pub fn dag(&self) -> &AppDag {
        &self.dag
    }

    pub fn get_timestamp_of_version(&self, f: &Frontiers) -> Timestamp {
        let mut timestamp = Timestamp::default();
        for id in f.iter() {
            if let Some(change) = self.lookup_change(*id) {
                timestamp = timestamp.max(change.timestamp);
            }
        }

        timestamp
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.dag.map.is_empty() && self.arena.can_import_snapshot()
    }

    #[inline]
    pub fn changes(&self) -> &ClientChanges {
        &self.changes
    }

    /// This is the only place to update the `OpLog.changes`
    pub(crate) fn insert_new_change(&mut self, mut change: Change, _: EnsureChangeDepsAreAtTheEnd) {
        self.op_groups.insert_by_change(&change);
        let entry = self.changes.entry(change.id.peer).or_default();
        match entry.last_mut() {
            Some(last) => {
                assert_eq!(
                    change.id.counter,
                    last.ctr_end(),
                    "change id is not continuous"
                );
                let timestamp_change = change.timestamp - last.timestamp;
                // TODO: make this a config
                if !last.has_dependents && change.deps_on_self() && timestamp_change < 1000 {
                    for op in take(change.ops.vec_mut()) {
                        last.ops.push(op);
                    }
                } else {
                    entry.push(change);
                }
            }
            None => {
                assert!(change.id.counter == 0, "change id is not continuous");
                entry.push(change);
            }
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
    /// - Return Err(LoroError::UsedOpID) when the change's id is occupied
    /// - Return Err(LoroError::DecodeError) when the change's deps are missing
    pub fn import_local_change(&mut self, change: Change) -> Result<(), LoroError> {
        let Some(change) = self.trim_the_known_part_of_change(change) else {
            return Ok(());
        };
        self.check_id_is_not_duplicated(change.id)?;
        if let Err(id) = self.check_deps(&change.deps) {
            return Err(LoroError::DecodeError(
                format!("Missing dep {:?}", id).into_boxed_str(),
            ));
        }

        if cfg!(debug_assertions) {
            let lamport = self.dag.frontiers_to_next_lamport(&change.deps);
            assert_eq!(
                lamport, change.lamport,
                "{:#?}\nDAG={:#?}",
                &change, &self.dag
            );
        }

        self.next_lamport = self.next_lamport.max(change.lamport_end());
        self.latest_timestamp = self.latest_timestamp.max(change.timestamp);
        self.dag.vv.extend_to_include_last_id(change.id_last());
        self.dag.frontiers.retain_non_included(&change.deps);
        self.dag.frontiers.filter_peer(change.id.peer);
        self.dag.frontiers.push(change.id_last());
        let mark = self.update_dag_on_new_change(&change);
        self.insert_new_change(change, mark);
        Ok(())
    }

    /// Every time we import a new change, it should run this function to update the dag
    pub(crate) fn update_dag_on_new_change(
        &mut self,
        change: &Change,
    ) -> EnsureChangeDepsAreAtTheEnd {
        let len = change.content_len();
        if change.deps_on_self() {
            // don't need to push new element to dag because it only depends on itself
            let nodes = self.dag.map.get_mut(&change.id.peer).unwrap();
            let last = nodes.last_mut().unwrap();
            assert_eq!(last.peer, change.id.peer, "peer id is not the same");
            assert_eq!(
                last.cnt + last.len as Counter,
                change.id.counter,
                "counter is not continuous"
            );
            assert_eq!(
                last.lamport + last.len as Lamport,
                change.lamport,
                "lamport is not continuous"
            );
            last.len = (change.id.counter - last.cnt) as usize + len;
            last.has_succ = false;
        } else {
            let vv = self.dag.frontiers_to_im_vv(&change.deps);
            let dag_row = &mut self.dag.map.entry(change.id.peer).or_default();
            if change.id.counter > 0 {
                assert_eq!(
                    dag_row.last().unwrap().ctr_end(),
                    change.id.counter,
                    "counter is not continuous"
                );
            }
            dag_row.push_rle_element(AppDagNode {
                vv,
                peer: change.id.peer,
                cnt: change.id.counter,
                lamport: change.lamport,
                deps: change.deps.clone(),
                has_succ: false,
                len,
            });

            for dep in change.deps.iter() {
                self.ensure_dep_on_change_end(change.id.peer, *dep);
                let target = self.dag.get_mut(*dep).unwrap();
                if target.ctr_last() == dep.counter {
                    target.has_succ = true;
                }
            }
        }

        EnsureChangeDepsAreAtTheEnd
    }

    fn ensure_dep_on_change_end(&mut self, src: PeerID, dep: ID) {
        let changes = self.changes.get_mut(&dep.peer).unwrap();
        match changes.binary_search_by(|c| c.ctr_last().cmp(&dep.counter)) {
            Ok(index) => {
                if src != dep.peer {
                    changes[index].has_dependents = true;
                }
            }
            Err(index) => {
                // This operation is slow in some rare cases, but I guess it's fine for now.
                //
                // It's only slow when you import an old concurrent change.
                // And once it's imported, because it's old, it has small lamport timestamp, so it
                // won't be slow again in the future imports.
                let change = &mut changes[index];
                let offset = (dep.counter - change.id.counter + 1) as usize;
                let left = change.slice(0, offset);
                let right = change.slice(offset, change.atom_len());
                assert_ne!(left.atom_len(), 0);
                assert_ne!(right.atom_len(), 0);
                *change = left;
                changes.insert(index + 1, right);
            }
        }
    }

    /// Trim the known part of change
    pub(crate) fn trim_the_known_part_of_change(&self, change: Change) -> Option<Change> {
        let Some(changes) = self.changes.get(&change.id.peer) else {
            return Some(change);
        };

        if changes.is_empty() {
            return Some(change);
        }

        let end = changes.last().unwrap().ctr_end();
        if change.id.counter >= end {
            return Some(change);
        }

        if change.ctr_end() <= end {
            return None;
        }

        let offset = (end - change.id.counter) as usize;
        Some(change.slice(offset, change.atom_len()))
    }

    fn check_id_is_not_duplicated(&self, id: ID) -> Result<(), LoroError> {
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

    pub fn next_lamport(&self) -> Lamport {
        self.next_lamport
    }

    pub fn next_id(&self, peer: PeerID) -> ID {
        let cnt = self.dag.vv.get(&peer).copied().unwrap_or(0);
        ID::new(peer, cnt)
    }

    pub fn get_peer_changes(&self, peer: PeerID) -> Option<&Vec<Change>> {
        self.changes.get(&peer)
    }

    pub(crate) fn vv(&self) -> &VersionVector {
        &self.dag.vv
    }

    pub(crate) fn frontiers(&self) -> &Frontiers {
        &self.dag.frontiers
    }

    /// - Ordering::Less means self is less than target or parallel
    /// - Ordering::Equal means versions equal
    /// - Ordering::Greater means self's version is greater than target
    pub fn cmp_frontiers(&self, other: &Frontiers) -> Ordering {
        self.dag.cmp_frontiers(other)
    }

    pub(crate) fn get_min_lamport_at(&self, id: ID) -> Lamport {
        self.get_change_at(id).map(|c| c.lamport).unwrap_or(0)
    }

    pub(crate) fn get_max_lamport_at(&self, id: ID) -> Lamport {
        self.get_change_at(id)
            .map(|c| {
                let change_counter = c.id.counter as u32;
                c.lamport + c.ops().last().map(|op| op.counter).unwrap_or(0) as u32 - change_counter
            })
            .unwrap_or(Lamport::MAX)
    }

    pub fn get_change_at(&self, id: ID) -> Option<&Change> {
        if let Some(peer_changes) = self.changes.get(&id.peer) {
            if let Some(result) = peer_changes.get_by_atom_index(id.counter) {
                return Some(&peer_changes[result.merged_index]);
            }
        }

        None
    }

    pub fn get_remote_change_at(&self, id: ID) -> Option<Change<RemoteOp>> {
        let change = self.get_change_at(id)?;
        Some(self.convert_change_to_remote(change))
    }

    fn convert_change_to_remote(&self, change: &Change) -> Change<RemoteOp> {
        let mut ops = RleVec::new();
        for op in change.ops.iter() {
            for op in self.local_op_to_remote(op) {
                ops.push(op);
            }
        }

        Change {
            ops,
            id: change.id,
            deps: change.deps.clone(),
            lamport: change.lamport,
            timestamp: change.timestamp,
            has_dependents: false,
        }
    }

    pub(crate) fn local_op_to_remote(&self, op: &crate::op::Op) -> SmallVec<[RemoteOp<'_>; 1]> {
        let container = self.arena.get_container_id(op.container).unwrap();
        let mut contents: SmallVec<[_; 1]> = SmallVec::new();
        match &op.content {
            crate::op::InnerContent::List(list) => match list {
                list_op::InnerListOp::Insert { slice, pos } => match container.container_type() {
                    loro_common::ContainerType::Text => {
                        let str = self.arena.slice_str_by_unicode_range(
                            slice.0.start as usize..slice.0.end as usize,
                        );
                        contents.push(RawOpContent::List(list_op::ListOp::Insert {
                            slice: ListSlice::RawStr {
                                unicode_len: str.chars().count(),
                                str: Cow::Owned(str),
                            },
                            pos: *pos,
                        }));
                    }
                    loro_common::ContainerType::List => {
                        contents.push(RawOpContent::List(list_op::ListOp::Insert {
                            slice: ListSlice::RawData(Cow::Owned(
                                self.arena
                                    .get_values(slice.0.start as usize..slice.0.end as usize),
                            )),
                            pos: *pos,
                        }))
                    }
                    loro_common::ContainerType::Map => unreachable!(),
                    loro_common::ContainerType::Tree => unreachable!(),
                },
                list_op::InnerListOp::InsertText {
                    slice,
                    unicode_len: len,
                    unicode_start: _,
                    pos,
                } => match container.container_type() {
                    loro_common::ContainerType::Text => {
                        contents.push(RawOpContent::List(list_op::ListOp::Insert {
                            slice: ListSlice::RawStr {
                                unicode_len: *len as usize,
                                str: Cow::Owned(std::str::from_utf8(slice).unwrap().to_owned()),
                            },
                            pos: *pos as usize,
                        }));
                    }
                    loro_common::ContainerType::List
                    | loro_common::ContainerType::Map
                    | loro_common::ContainerType::Tree => {
                        unreachable!()
                    }
                },
                list_op::InnerListOp::Delete(del) => {
                    contents.push(RawOpContent::List(list_op::ListOp::Delete(*del)))
                }
                list_op::InnerListOp::StyleStart {
                    start,
                    end,
                    key,
                    value,
                    info,
                } => contents.push(RawOpContent::List(list_op::ListOp::StyleStart {
                    start: *start,
                    end: *end,
                    key: key.clone(),
                    value: value.clone(),
                    info: *info,
                })),
                list_op::InnerListOp::StyleEnd => {
                    contents.push(RawOpContent::List(list_op::ListOp::StyleEnd))
                }
            },
            crate::op::InnerContent::Map(map) => {
                let value = map.value.clone();
                contents.push(RawOpContent::Map(crate::container::map::MapSet {
                    key: map.key.clone(),
                    value,
                }))
            }
            crate::op::InnerContent::Tree(tree) => contents.push(RawOpContent::Tree(*tree)),
        };

        let mut ans = SmallVec::with_capacity(contents.len());
        for content in contents {
            ans.push(RemoteOp {
                container: container.clone(),
                content,
                counter: op.counter,
            })
        }
        ans
    }

    pub(crate) fn import_unknown_lamport_pending_changes(
        &mut self,
        remote_changes: Vec<Change>,
    ) -> Result<(), LoroError> {
        let latest_vv = self.dag.vv.clone();
        self.extend_pending_changes_with_unknown_lamport(remote_changes, &latest_vv);
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

    #[allow(unused)]
    pub(crate) fn lookup_op(&self, id: ID) -> Option<&crate::op::Op> {
        self.lookup_change(id)
            .and_then(|change| change.ops.get_by_atom_index(id.counter).map(|x| x.element))
    }

    #[inline(always)]
    pub(crate) fn export_from(&self, vv: &VersionVector) -> Vec<u8> {
        encode_oplog(self, vv, EncodeMode::Auto)
    }

    #[inline(always)]
    pub(crate) fn decode(&mut self, data: ParsedHeaderAndBody) -> Result<(), LoroError> {
        decode_oplog(self, data)
    }

    /// Iterates over all changes between `a` and `b` peer by peer (not in causal order, fast)
    pub(crate) fn for_each_change_within(
        &self,
        a: &VersionVector,
        b: &VersionVector,
        mut f: impl FnMut(&Change),
    ) {
        for (peer, changes) in self.changes.iter() {
            let mut from_cnt = a.get(peer).copied().unwrap_or(0);
            let mut to_cnt = b.get(peer).copied().unwrap_or(0);
            if from_cnt == to_cnt {
                continue;
            }

            if to_cnt < from_cnt {
                std::mem::swap(&mut from_cnt, &mut to_cnt);
            }

            let Some(result) = changes.get_by_atom_index(from_cnt) else {
                continue;
            };

            for change in &changes[result.merged_index..changes.len()] {
                if change.id.counter >= to_cnt {
                    break;
                }

                f(change)
            }
        }
    }

    /// iterates over all changes between LCA(common ancestors) to the merged version of (`from` and `to`) causally
    ///
    /// Tht iterator will include a version vector when the change is applied
    ///
    /// returns: (common_ancestor_vv, iterator)
    ///
    /// Note: the change returned by the iterator may include redundant ops at the beginning, you should trim it by yourself.
    /// You can trim it by the provided counter value. It should start with the counter.
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
        impl Iterator<Item = (&Change, Counter, Rc<RefCell<VersionVector>>)>,
    ) {
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
        let common_ancestors_vv = self.dag.frontiers_to_vv(&common_ancestors).unwrap();
        // go from lca to merged_vv
        let diff = common_ancestors_vv.diff(&merged_vv).right;
        let mut iter = self.dag.iter_causal(&common_ancestors, diff);
        let mut node = iter.next();
        let mut cur_cnt = 0;
        let vv = Rc::new(RefCell::new(VersionVector::default()));
        (
            common_ancestors_vv.clone(),
            std::iter::from_fn(move || {
                if let Some(inner) = &node {
                    let mut inner_vv = vv.borrow_mut();
                    inner_vv.clear();
                    inner_vv.extend_to_include_vv(inner.data.vv.iter());
                    let peer = inner.data.peer;
                    let cnt = inner
                        .data
                        .cnt
                        .max(cur_cnt)
                        .max(common_ancestors_vv.get(&peer).copied().unwrap_or(0));
                    let end = (inner.data.cnt + inner.data.len as Counter)
                        .min(merged_vv.get(&peer).copied().unwrap_or(0));
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
                    Some((change, cnt, vv.clone()))
                } else {
                    None
                }
            }),
        )
    }

    pub fn len_changes(&self) -> usize {
        self.changes.values().map(|x| x.len()).sum()
    }

    pub fn diagnose_size(&self) -> SizeInfo {
        let mut total_changes = 0;
        let mut total_ops = 0;
        let mut total_atom_ops = 0;
        let total_dag_node = self.dag.map.len();
        for changes in self.changes.values() {
            total_changes += changes.len();
            for change in changes.iter() {
                total_ops += change.ops.len();
                total_atom_ops += change.atom_len();
            }
        }

        println!("total changes: {}", total_changes);
        println!("total ops: {}", total_ops);
        println!("total atom ops: {}", total_atom_ops);
        println!("total dag node: {}", total_dag_node);
        SizeInfo {
            total_changes,
            total_ops,
            total_atom_ops,
            total_dag_node,
        }
    }

    #[allow(unused)]
    pub(crate) fn debug_check(&self) {
        for (_, changes) in self.changes().iter() {
            let c = changes.last().unwrap();
            let node = self.dag.get(c.id_start()).unwrap();
            assert_eq!(c.id_end(), node.id_end());
        }
    }

    pub(crate) fn iter_changes<'a>(
        &'a self,
        from: &VersionVector,
        to: &VersionVector,
    ) -> impl Iterator<Item = &'a Change> + 'a {
        let spans: Vec<_> = from.diff_iter(to).1.collect();
        spans.into_iter().flat_map(move |span| {
            let peer = span.client_id;
            let cnt = span.counter.start;
            let end_cnt = span.counter.end;
            let peer_changes = self.changes.get(&peer).unwrap();
            let index = peer_changes.search_atom_index(cnt);
            peer_changes[index..]
                .iter()
                .take_while(move |x| x.ctr_start() < end_cnt)
        })
    }
}

#[derive(Debug)]
pub struct SizeInfo {
    pub total_changes: usize,
    pub total_ops: usize,
    pub total_atom_ops: usize,
    pub total_dag_node: usize,
}

impl Default for OpLog {
    fn default() -> Self {
        Self::new()
    }
}

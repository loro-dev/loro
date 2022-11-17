//! [LogStore] stores all the [Change]s and [Op]s. It's also a [DAG][crate::dag];
//!
//!
mod encoding;
mod iter;
use std::{
    marker::PhantomPinned,
    sync::{Arc, Mutex, MutexGuard, RwLock},
};

use fxhash::{FxHashMap, FxHashSet};

use rle::{HasLength, RleVec, RleVecWithIndex, Sliceable};

use smallvec::SmallVec;

use crate::{
    change::{Change, ChangeMergeCfg},
    configure::Configure,
    container::{
        registry::{ContainerInstance, ContainerRegistry},
        Container, ContainerID,
    },
    dag::{remove_included_frontiers, Dag, DagUtils},
    debug_log,
    id::{ClientID, ContainerIdx, Counter},
    op::{Content, RemoteOp, RichOp},
    span::{HasCounter, HasCounterSpan, HasIdSpan, HasLamportSpan, IdSpan},
    version::are_frontiers_eq,
    ContainerType, Lamport, Op, Timestamp, VersionVector, ID,
};

const _YEAR: u64 = 365 * 24 * 60 * 60;
const MONTH: u64 = 30 * 24 * 60 * 60;

#[derive(Debug)]
pub struct GcConfig {
    pub gc: bool,
    pub snapshot_interval: u64,
}

impl Default for GcConfig {
    fn default() -> Self {
        GcConfig {
            gc: true,
            snapshot_interval: 6 * MONTH,
        }
    }
}

#[derive(Debug)]
/// LogStore stores the full history of Loro
///
/// This is a self-referential structure. So it need to be pinned.
///
/// `frontier`s are the Changes without children in the DAG (there is no dep pointing to them)
///
/// TODO: Refactor we need to move the things about the current state out of LogStore (container, latest_lamport, ..)
pub struct LogStore {
    changes: FxHashMap<ClientID, RleVecWithIndex<Change, ChangeMergeCfg>>,
    vv: VersionVector,
    cfg: Configure,
    latest_lamport: Lamport,
    latest_timestamp: Timestamp,
    pub(crate) this_client_id: ClientID,
    frontiers: SmallVec<[ID; 2]>,
    /// CRDT container manager
    pub(crate) reg: ContainerRegistry,
    _pin: PhantomPinned,
}

type ContainerGuard<'a> = MutexGuard<'a, ContainerInstance>;

impl LogStore {
    pub(crate) fn new(mut cfg: Configure, client_id: Option<ClientID>) -> Arc<RwLock<Self>> {
        let this_client_id = client_id.unwrap_or_else(|| cfg.rand.next_u64());
        Arc::new(RwLock::new(Self {
            cfg,
            this_client_id,
            changes: FxHashMap::default(),
            latest_lamport: 0,
            latest_timestamp: 0,
            frontiers: Default::default(),
            vv: Default::default(),
            reg: ContainerRegistry::new(),
            _pin: PhantomPinned,
        }))
    }

    #[inline]
    pub fn lookup_change(&self, id: ID) -> Option<&Change> {
        self.changes
            .get(&id.client_id)
            .map(|changes| changes.get(id.counter as usize).unwrap().element)
    }

    pub fn import(
        &mut self,
        mut changes: FxHashMap<ClientID, RleVecWithIndex<Change<RemoteOp>, ChangeMergeCfg>>,
    ) {
        debug_log!(
            "======================================================LOGSTORE CLIENT {} ====== IMPORT CHANGES {:#?}",
            self.this_client_id,
            &changes
        );
        // tailor changes
        changes.retain(|_, v| !v.is_empty());
        if changes.is_empty() {
            return;
        }

        for (client_id, changes) in changes.iter_mut() {
            let self_end_ctr = self.vv.get(client_id).copied().unwrap_or(0);
            let other_start_ctr = changes.first().unwrap().ctr_start();
            match other_start_ctr.cmp(&self_end_ctr) {
                std::cmp::Ordering::Less => {
                    *changes = changes.slice(
                        (self_end_ctr - other_start_ctr) as usize,
                        changes.atom_len(),
                    );
                }
                std::cmp::Ordering::Equal => {}
                std::cmp::Ordering::Greater => {
                    unimplemented!("cache pending changes");
                }
            }
        }

        changes.retain(|_, v| !v.is_empty());
        // get related containers, and acquire their locks
        let mut container_map: FxHashMap<ContainerID, ContainerGuard> = Default::default();
        for (_, changes) in changes.iter() {
            for change in changes.iter() {
                for op in change.ops.iter() {
                    if !container_map.contains_key(&op.container) {
                        let guard = self.reg.get_or_create(&op.container).lock().unwrap();
                        container_map
                            // SAFETY: ignore lifetime issues here, because it's safe for us to store the mutex guard here
                            .insert(op.container.clone(), unsafe { std::mem::transmute(guard) });
                    }
                }
            }
        }

        // calculate latest frontiers
        let mut next_vv: VersionVector = self.vv.clone();
        let mut next_frontiers: VersionVector = self.frontiers.iter().copied().collect();
        for (_, changes) in changes.iter() {
            next_frontiers.set_end(changes.last().unwrap().id_end());
            next_vv.set_end(changes.last().unwrap().id_end());
        }

        // push changes to log stores
        let cfg = self.get_change_merge_cfg();
        for (client_id, changes) in changes.iter() {
            let mut inner_changes = Vec::with_capacity(changes.len());
            for change in changes.iter() {
                remove_included_frontiers(&mut next_frontiers, &change.deps);
                let change = self.change_to_imported_format(change, &mut container_map);
                inner_changes.push(change);
            }

            let rle = self
                .changes
                .entry(*client_id)
                .or_insert_with(|| RleVecWithIndex::new_cfg(cfg.clone()));
            for change in inner_changes {
                rle.push(change);
            }
        }

        let mut container_map: FxHashMap<ContainerIdx, ContainerGuard> = container_map
            .into_iter()
            .map(|(k, v)| (self.reg.get_idx(&k).unwrap(), v))
            .collect();

        // apply changes to containers
        'apply: {
            let latest_frontiers = next_frontiers.get_frontiers();
            // 0. calculate common ancestors
            debug_log!(
                "FIND COMMON ANCESTORS self={:?} latest={:?}",
                &self.frontiers,
                &latest_frontiers
            );
            let common_ancestors = self.find_common_ancestor(&self.frontiers, &latest_frontiers);
            if are_frontiers_eq(&common_ancestors, &self.frontiers) {
                // we may apply changes directly into state
                let target_spans = next_vv.diff(&self.vv).left;
                // TODO: can reuse this path
                let causal_visit_path: Vec<_> =
                    self.iter_causal(&common_ancestors, target_spans).collect();
                if causal_visit_path
                    .iter()
                    .all(|x| x.retreat.is_empty() && x.forward.is_empty())
                {
                    // can update containers state directly without consulting CRDT
                    for iter in causal_visit_path {
                        let start = iter.slice.start;
                        let end = iter.slice.end;
                        let change = iter.data;

                        for op in change.ops.iter() {
                            let rich_op = RichOp::new_by_slice_on_change(change, op, start, end);
                            if rich_op.atom_len() == 0 {
                                continue;
                            }

                            let container = container_map.get_mut(&op.container).unwrap();
                            container.update_state_directly(&rich_op);
                        }
                    }

                    break 'apply;
                }
            }

            // 1. record all the changes to the trackers
            let mut common_ancestors_vv = self.vv.clone();
            common_ancestors_vv.retreat(&self.find_path(&common_ancestors, &self.frontiers).right);
            for (_, container) in container_map.iter_mut() {
                container.tracker_checkout(&common_ancestors_vv);
            }

            for iter in self.iter_causal(&common_ancestors, next_vv.diff(&common_ancestors_vv).left)
            {
                let start = iter.slice.start;
                let end = iter.slice.end;
                let change = iter.data;
                debug_log!("iter {:#?}", &iter);
                for (_, container) in container_map.iter_mut() {
                    // TODO: perf: can give a hint here, to cache needless retreat and forward
                    container.track_retreat(&iter.retreat);
                    container.track_forward(&iter.forward);
                }

                for op in change.ops.iter() {
                    let rich_op = RichOp::new_by_slice_on_change(change, op, start, end);
                    if rich_op.atom_len() == 0 {
                        continue;
                    }

                    if let Some(container) = container_map.get_mut(&op.container) {
                        container.track_apply(&rich_op);
                    }
                }
            }

            // 2. calculate and apply the effects to the state
            debug_log!("LOGSTORE STAGE 2",);
            let path = next_vv.diff(&self.vv).left;
            for (_, container) in container_map.iter_mut() {
                container.apply_tracked_effects_from(&self.vv, &path);
            }
        }

        // update the rest of log store states
        self.vv = next_vv;
        self.frontiers = next_frontiers.get_frontiers();
        self.latest_lamport = self
            .changes
            .iter()
            .map(|(_, v)| v.last().unwrap().lamport_last())
            .max()
            .unwrap();
        self.latest_timestamp = self
            .changes
            .iter()
            .map(|(_, v)| v.last().unwrap().timestamp)
            .max()
            .unwrap();
    }

    pub fn export(
        &self,
        remote_vv: &VersionVector,
    ) -> FxHashMap<ClientID, RleVecWithIndex<Change<RemoteOp>, ChangeMergeCfg>> {
        let mut ans: FxHashMap<ClientID, RleVecWithIndex<Change<RemoteOp>, ChangeMergeCfg>> =
            Default::default();
        let self_vv = self.vv();
        let diff = self_vv.diff(remote_vv);
        for span in diff.left.iter() {
            let changes = self.get_changes_slice(span.id_span());
            for change in changes.iter() {
                let vec = ans
                    .entry(change.id.client_id)
                    .or_insert_with(|| RleVecWithIndex::new_cfg(self.get_change_merge_cfg()));

                vec.push(self.change_to_export_format(change));
            }
        }

        debug_log!("export {:#?}", &ans);
        ans
    }

    fn get_changes_slice(&self, id_span: IdSpan) -> Vec<Change> {
        if let Some(changes) = self.changes.get(&id_span.client_id) {
            let mut ans = Vec::with_capacity(id_span.atom_len() / 30);
            for change in changes.slice_iter(
                id_span.counter.min() as usize,
                id_span.counter.end() as usize,
            ) {
                let change = change.value.slice(change.start, change.end);
                ans.push(change);
            }

            ans
        } else {
            vec![]
        }
    }

    fn change_to_imported_format(
        &mut self,
        change: &Change<RemoteOp>,
        containers: &mut FxHashMap<ContainerID, ContainerGuard>,
    ) -> Change {
        let mut new_ops = RleVec::new();
        for op in change.ops.iter() {
            let container = containers.get_mut(&op.container).unwrap();
            // TODO: avoid this clone
            let mut op = op.clone();
            container.to_import(&mut op);
            for op in op.convert(self) {
                new_ops.push(op);
            }
        }

        Change {
            ops: new_ops,
            deps: change.deps.clone(),
            id: change.id,
            lamport: change.lamport,
            timestamp: change.timestamp,
        }
    }

    fn change_to_export_format(&self, change: &Change) -> Change<RemoteOp> {
        let mut ops = RleVec::new();
        for op in change.ops.iter() {
            ops.push(self.to_remote_op(op));
        }

        Change {
            ops,
            deps: change.deps.clone(),
            id: change.id,
            lamport: change.lamport,
            timestamp: change.timestamp,
        }
    }

    fn to_remote_op(&self, op: &Op) -> RemoteOp {
        let container = self.reg.get_by_idx(op.container).unwrap();
        let mut container = container.lock().unwrap();
        let mut op = op.clone().convert(self);
        container.to_export(&mut op, self.cfg.gc.gc);
        op
    }

    pub(crate) fn create_container(
        &mut self,
        container_type: ContainerType,
        parent: ContainerID,
    ) -> ContainerID {
        let id = self.next_id();
        let container_id = ContainerID::new_normal(id, container_type);
        let parent_idx = self.get_container_idx(&parent).unwrap();
        self.append_local_ops(&[Op::new(
            id,
            Content::Container(container_id.clone()),
            parent_idx,
        )]);
        self.reg.register(&container_id);
        container_id
    }

    #[inline(always)]
    pub fn next_lamport(&self) -> Lamport {
        self.latest_lamport + 1
    }

    #[inline(always)]
    pub fn next_id(&self) -> ID {
        ID {
            client_id: self.this_client_id,
            counter: self.get_next_counter(self.this_client_id),
        }
    }

    #[inline(always)]
    pub fn next_id_for(&self, client: ClientID) -> ID {
        ID {
            client_id: client,
            counter: self.get_next_counter(client),
        }
    }

    #[inline(always)]
    pub fn this_client_id(&self) -> ClientID {
        self.this_client_id
    }

    #[inline(always)]
    pub fn frontiers(&self) -> &[ID] {
        &self.frontiers
    }

    fn get_change_merge_cfg(&self) -> ChangeMergeCfg {
        ChangeMergeCfg {
            max_change_length: self.cfg.change.max_change_length,
            max_change_interval: self.cfg.change.max_change_interval,
        }
    }

    /// this method would not get the container and apply op
    pub fn append_local_ops(&mut self, ops: &[Op]) {
        if ops.is_empty() {
            return;
        }

        let lamport = self.next_lamport();
        let timestamp = (self.cfg.get_time)();
        let id = ID {
            client_id: self.this_client_id,
            counter: self.get_next_counter(self.this_client_id),
        };
        let last = ops.last().unwrap();
        let last_ctr = last.ctr_last();
        let last_id = ID::new(self.this_client_id, last_ctr);
        let change = Change {
            id,
            deps: std::mem::replace(&mut self.frontiers, smallvec::smallvec![last_id]),
            ops: ops.into(),
            lamport,
            timestamp,
        };

        self.latest_lamport = lamport + change.content_len() as u32 - 1;
        self.latest_timestamp = timestamp;
        self.vv.set_end(change.id_end());
        let cfg = self.get_change_merge_cfg();
        self.changes
            .entry(self.this_client_id)
            .or_insert_with(|| RleVecWithIndex::new_with_conf(cfg))
            .push(change);

        debug_log!("CHANGES---------------- site {}", self.this_client_id);
    }

    #[inline]
    pub fn contains(&self, id: ID) -> bool {
        self.changes
            .get(&id.client_id)
            .map_or(0, |changes| changes.atom_len())
            > id.counter as usize
    }

    #[inline]
    fn get_next_counter(&self, client_id: ClientID) -> Counter {
        self.changes
            .get(&client_id)
            .map(|changes| changes.atom_len())
            .unwrap_or(0) as Counter
    }

    #[inline]
    #[allow(dead_code)]
    pub(crate) fn iter_client_op(&self, client_id: ClientID) -> iter::ClientOpIter<'_> {
        iter::ClientOpIter {
            change_index: 0,
            op_index: 0,
            changes: self.changes.get(&client_id),
        }
    }

    pub(crate) fn iter_ops_at_id_span(
        &self,
        id_span: IdSpan,
        container: ContainerID,
    ) -> iter::OpSpanIter<'_> {
        let idx = self.get_container_idx(&container).unwrap();
        iter::OpSpanIter::new(&self.changes, id_span, idx)
    }

    #[inline(always)]
    pub fn get_vv(&self) -> &VersionVector {
        &self.vv
    }

    #[cfg(feature = "test_utils")]
    pub fn debug_inspect(&mut self) {
        println!(
            "LogStore:\n- Clients={}\n- Changes={}\n- Ops={}\n- Atoms={}",
            self.changes.len(),
            self.changes
                .values()
                .map(|v| format!("{}", v.vec().len()))
                .collect::<Vec<_>>()
                .join(", "),
            self.changes
                .values()
                .map(|v| format!("{}", v.vec().iter().map(|x| x.ops.len()).sum::<usize>()))
                .collect::<Vec<_>>()
                .join(", "),
            self.changes
                .values()
                .map(|v| format!("{}", v.atom_len()))
                .collect::<Vec<_>>()
                .join(", "),
        );

        self.reg.debug_inspect();
    }

    // TODO: remove
    #[inline(always)]
    pub(crate) fn get_container_idx(&self, container: &ContainerID) -> Option<ContainerIdx> {
        self.reg.get_idx(container)
    }

    pub fn get_or_create_container(
        &mut self,
        container: &ContainerID,
    ) -> &Arc<Mutex<ContainerInstance>> {
        self.reg.get_or_create(container)
    }

    #[inline(always)]
    pub fn get_container(&self, container: &ContainerID) -> Option<&Arc<Mutex<ContainerInstance>>> {
        self.reg.get(container)
    }

    pub(crate) fn get_or_create_container_idx(&mut self, container: &ContainerID) -> ContainerIdx {
        self.reg.get_or_create_container_idx(container)
    }
}

impl Dag for LogStore {
    type Node = Change;

    fn get(&self, id: ID) -> Option<&Self::Node> {
        self.changes
            .get(&id.client_id)
            .and_then(|x| x.get(id.counter as usize).map(|x| x.element))
    }

    fn frontier(&self) -> &[ID] {
        &self.frontiers
    }

    fn vv(&self) -> crate::VersionVector {
        self.vv.clone()
    }
}

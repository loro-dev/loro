//! [LogStore] stores all the [Change]s and [Op]s. It's also a [DAG][crate::dag];
//!
//!
mod encoding;
mod import;
mod iter;
use std::{
    marker::PhantomPinned,
    sync::{Arc, Mutex, MutexGuard, RwLock},
};

use fxhash::FxHashMap;

use rle::{HasLength, RleVec, RleVecWithIndex, Sliceable};

use smallvec::SmallVec;

use crate::{
    change::{Change, ChangeMergeCfg},
    configure::Configure,
    container::{
        registry::{ContainerInstance, ContainerRegistry},
        ContainerID,
    },
    dag::Dag,
    debug_log,
    hierarchy::Hierarchy,
    id::{ClientID, ContainerIdx, Counter},
    op::RemoteOp,
    span::{HasCounterSpan, HasIdSpan, IdSpan},
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

type ClientChanges = FxHashMap<ClientID, RleVecWithIndex<Change, ChangeMergeCfg>>;
type RemoteClientChanges = FxHashMap<ClientID, RleVecWithIndex<Change<RemoteOp>, ChangeMergeCfg>>;

#[derive(Debug)]
/// LogStore stores the full history of Loro
///
/// This is a self-referential structure. So it need to be pinned.
///
/// `frontier`s are the Changes without children in the DAG (there is no dep pointing to them)
///
/// TODO: Refactor we need to move the things about the current state out of LogStore (container, latest_lamport, ..)
pub struct LogStore {
    changes: ClientChanges,
    vv: VersionVector,
    cfg: Configure,
    latest_lamport: Lamport,
    latest_timestamp: Timestamp,
    frontiers: SmallVec<[ID; 2]>,
    pub(crate) this_client_id: ClientID,
    /// CRDT container manager
    pub(crate) reg: ContainerRegistry,
    pub(crate) hierarchy: Hierarchy,
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
            hierarchy: Default::default(),
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
            let container_idx = self.get_container_idx(&op.container).unwrap();
            for op in op.clone().convert(container, container_idx) {
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
        op.clone().convert(&mut container, self.cfg.gc.gc)
    }

    pub(crate) fn create_container(&mut self, container_type: ContainerType) -> ContainerID {
        let id = self.next_id();
        let container_id = ContainerID::new_normal(id, container_type);
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

    pub(crate) fn iter_ops_at_id_span(&self, id_span: IdSpan) -> iter::OpSpanIter<'_> {
        iter::OpSpanIter::new(&self.changes, id_span)
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

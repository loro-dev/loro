//! [LogStore] stores all the [Change]s and [Op]s. It's also a [DAG][crate::dag];
//!
//!
mod encoding;
mod import;
mod iter;

use crate::LoroValue;
pub use encoding::{EncodeConfig, EncodeMode, LoroEncoder};
pub(crate) use import::ImportContext;
use std::{
    marker::PhantomPinned,
    sync::{Arc, Mutex, MutexGuard, RwLock, Weak},
};

use fxhash::FxHashMap;

use rle::{HasLength, RleVec, RleVecWithIndex, Sliceable};
use smallvec::SmallVec;

use crate::{
    change::{Change, ChangeMergeCfg},
    configure::Configure,
    container::{
        registry::{ContainerIdx, ContainerInstance, ContainerRegistry},
        ContainerID,
    },
    dag::Dag,
    id::{ClientID, Counter},
    op::RemoteOp,
    span::{HasCounterSpan, HasIdSpan, IdSpan},
    ContainerType, Lamport, Op, Timestamp, VersionVector, ID,
};

const _YEAR: u64 = 365 * 24 * 60 * 60;
const MONTH: u64 = 30 * 24 * 60 * 60;

#[derive(Debug, Clone, Copy)]
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

impl GcConfig {
    #[inline(always)]
    pub fn with_gc(self, gc: bool) -> Self {
        Self { gc, ..self }
    }
}

type ClientChanges = FxHashMap<ClientID, RleVecWithIndex<Change, ChangeMergeCfg>>;
type RemoteClientChanges = FxHashMap<ClientID, Vec<Change<RemoteOp>>>;

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
    _pin: PhantomPinned,
}

type ContainerGuard<'a> = MutexGuard<'a, ContainerInstance>;

impl LogStore {
    pub(crate) fn new(cfg: Configure, client_id: Option<ClientID>) -> Arc<RwLock<Self>> {
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

    pub fn export(&self, remote_vv: &VersionVector) -> FxHashMap<ClientID, Vec<Change<RemoteOp>>> {
        let mut ans: FxHashMap<ClientID, Vec<Change<RemoteOp>>> = Default::default();
        let self_vv = self.vv();
        for span in self_vv.sub_iter(remote_vv) {
            let changes = self.get_changes_slice(span.id_span());
            for change in changes.iter() {
                let vec = ans.entry(change.id.client_id).or_insert_with(Vec::new);
                vec.push(self.change_to_export_format(change));
            }
        }

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

    pub(crate) fn change_to_export_format(&self, change: &Change) -> Change<RemoteOp> {
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
        let mut container = container.try_lock().unwrap();
        op.clone().convert(&mut container, self.cfg.gc.gc)
    }

    pub(crate) fn create_container(
        &mut self,
        container_type: ContainerType,
    ) -> (ContainerID, ContainerIdx) {
        let id = self.next_id();
        let container_id = ContainerID::new_normal(id, container_type);
        let idx = self.reg.register(&container_id);
        (container_id, idx)
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

    /// this method would not get the container and apply op
    pub fn append_local_ops(&mut self, ops: &[Op]) -> (SmallVec<[ID; 2]>, &[ID]) {
        let old_version = self.frontiers.clone();
        if ops.is_empty() {
            return (old_version, self.frontier());
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
        (old_version, self.frontier())
    }

    #[inline]
    pub fn contains_container(&self, id: &ContainerID) -> bool {
        self.reg.contains(id)
    }

    #[inline]
    pub fn contains_id(&self, id: ID) -> bool {
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

    fn get_change_merge_cfg(&self) -> ChangeMergeCfg {
        ChangeMergeCfg {
            max_change_length: self.cfg.change.max_change_length,
            max_change_interval: self.cfg.change.max_change_interval,
        }
    }

    pub(crate) fn max_change_length(&mut self, max_change_length: usize) {
        self.cfg.change.max_change_length = max_change_length
    }

    pub(crate) fn max_change_interval(&mut self, max_change_interval: usize) {
        self.cfg.change.max_change_interval = max_change_interval
    }

    pub(crate) fn gc(&mut self, gc: bool) {
        self.cfg.gc.gc = gc;
    }

    pub(crate) fn snapshot_interval(&mut self, snapshot_interval: u64) {
        self.cfg.gc.snapshot_interval = snapshot_interval;
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
    ) -> Weak<Mutex<ContainerInstance>> {
        self.reg.get_or_create(container)
    }

    #[inline(always)]
    pub fn get_container(&self, container: &ContainerID) -> Option<Weak<Mutex<ContainerInstance>>> {
        self.reg.get(container)
    }

    pub(crate) fn get_or_create_container_idx(&mut self, container: &ContainerID) -> ContainerIdx {
        self.reg.get_or_create_container_idx(container)
    }

    pub fn to_json(&self) -> LoroValue {
        self.reg.to_json()
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

//! [LogStore] stores all the [Change]s and [Op]s. It's also a [DAG][crate::dag];
//!
//!
mod iter;
use std::{
    marker::PhantomPinned,
    sync::{Arc, RwLock, Weak},
};

use fxhash::FxHashMap;

use rle::{HasLength, RleVec, Sliceable};

use smallvec::SmallVec;

use crate::{
    change::{Change, ChangeMergeCfg},
    configure::Configure,
    container::{manager::ContainerManager, Container},
    dag::Dag,
    id::{ClientID, Counter},
    op::{OpContent, OpProxy},
    span::{HasIdSpan, IdSpan},
    Lamport, Op, Timestamp, VersionVector, ID,
};

const _YEAR: u64 = 365 * 24 * 60 * 60;
const MONTH: u64 = 30 * 24 * 60 * 60;

#[derive(Debug)]
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

pub type LogStoreRef = Arc<RwLock<LogStore>>;
pub type LogStoreWeakRef = Weak<RwLock<LogStore>>;

#[derive(Debug)]
/// LogStore stores the full history of Loro
///
/// This is a self-referential structure. So it need to be pinned.
///
/// `frontier`s are the Changes without children in the DAG (there is no dep pointing to them)
///
/// TODO: Refactor we need to move the things about the current state out of LogStore (container, latest_lamport, ..)
pub struct LogStore {
    changes: FxHashMap<ClientID, RleVec<Change, ChangeMergeCfg>>,
    vv: VersionVector,
    cfg: Configure,
    latest_lamport: Lamport,
    latest_timestamp: Timestamp,
    pub(crate) this_client_id: ClientID,
    frontier: SmallVec<[ID; 2]>,
    /// CRDT container manager
    pub(crate) container: Arc<RwLock<ContainerManager>>,
    to_self: Weak<RwLock<LogStore>>,
    _pin: PhantomPinned,
}

impl LogStore {
    pub fn new(
        mut cfg: Configure,
        client_id: Option<ClientID>,
        container: Arc<RwLock<ContainerManager>>,
    ) -> Arc<RwLock<Self>> {
        let this_client_id = client_id.unwrap_or_else(|| cfg.rand.next_u64());
        Arc::new_cyclic(|x| {
            RwLock::new(Self {
                cfg,
                this_client_id,
                changes: FxHashMap::default(),
                latest_lamport: 0,
                latest_timestamp: 0,
                frontier: Default::default(),
                container,
                to_self: x.clone(),
                vv: Default::default(),
                _pin: PhantomPinned,
            })
        })
    }

    #[inline]
    pub fn lookup_change(&self, id: ID) -> Option<&Change> {
        self.changes
            .get(&id.client_id)
            .map(|changes| changes.get(id.counter as usize).unwrap().element)
    }

    pub fn import(&mut self, mut changes: Vec<Change>) {
        let self_vv = self.vv();
        changes.sort_by_cached_key(|x| x.lamport);
        for change in changes
            .into_iter()
            .filter(|x| !self_vv.includes_id(x.last_id()))
        {
            check_import_change_valid(&change);
            // TODO: cache pending changes
            assert!(change.deps.iter().all(|x| self_vv.includes_id(*x)));
            self.apply_remote_change(change)
        }
    }

    pub fn export(&self, remote_vv: &VersionVector) -> Vec<Change> {
        let mut ans = Vec::default();
        let self_vv = self.vv();
        let diff = self_vv.diff(remote_vv);
        for span in diff.left.iter() {
            let mut changes = self.get_changes_slice(span.id_span());
            ans.append(&mut changes);
        }
        for change in ans.iter_mut() {
            self.change_to_export_format(change);
        }

        ans
    }

    fn get_changes_slice(&self, id_span: IdSpan) -> Vec<Change> {
        if let Some(changes) = self.changes.get(&id_span.client_id) {
            let mut ans = Vec::new();
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

    fn change_to_export_format(&self, change: &mut Change) {
        let container_manager = self.container.read().unwrap();
        for op in change.ops.vec_mut().iter_mut() {
            let container = container_manager.get(op.container.clone()).unwrap();
            container.to_export(op);
        }
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
    pub fn frontier(&self) -> &[ID] {
        &self.frontier
    }

    /// this method would not get the container and apply op
    pub fn append_local_ops(&mut self, ops: Vec<Op>) {
        let lamport = self.next_lamport();
        let timestamp = (self.cfg.get_time)();
        let id = ID {
            client_id: self.this_client_id,
            counter: self.get_next_counter(self.this_client_id),
        };
        let change = Change {
            id,
            ops: ops.into(),
            deps: std::mem::take(&mut self.frontier),
            lamport,
            timestamp,
            freezed: false,
            break_points: Default::default(),
        };

        self.frontier.push(ID::new(
            self.this_client_id,
            id.counter + change.len() as Counter - 1,
        ));
        self.latest_lamport = lamport + change.len() as u32 - 1;
        self.latest_timestamp = timestamp;
        self.vv.set_end(change.id_end());
        self.changes
            .entry(self.this_client_id)
            .or_insert_with(RleVec::new)
            .push(change);
    }

    pub fn apply_remote_change(&mut self, mut change: Change) {
        change.freezed = true;
        if self.contains(change.last_id()) {
            return;
        }

        for dep in &change.deps {
            if !self.contains(*dep) {
                unimplemented!("need impl pending changes");
            }
        }

        // TODO: find a way to remove this clone? we don't need change in apply method actually
        let change = self.push_change(change).clone();

        // Apply ops.
        // NOTE: applying expects that log_store has store the Change, but has not updated its vv yet
        for op in change.ops.iter() {
            self.apply_remote_op(&change, op);
            self.vv.set_end(op.id_end());
        }

        self.frontier = self
            .frontier
            .iter()
            .filter(|x| !change.deps.contains(x))
            .copied()
            .collect();
        self.frontier.push(change.last_id());

        if change.last_lamport() > self.latest_lamport {
            self.latest_lamport = change.last_lamport();
        }

        if change.timestamp > self.latest_timestamp {
            self.latest_timestamp = change.timestamp;
        }
    }

    #[inline]
    fn push_change(&mut self, change: Change) -> &Change {
        let v = self
            .changes
            .entry(change.id.client_id)
            .or_insert_with(RleVec::new);
        v.push(change);
        v.vec().last().unwrap()
    }

    /// this function assume op is not included in the log, and its deps are included.
    #[inline]
    fn apply_remote_op(&mut self, change: &Change, op: &Op) {
        let mut container = self.container.write().unwrap();
        let container = container.get_or_create(op.container(), self.to_self.clone());
        container.apply(&OpProxy::new(change, op, None), self);
    }

    #[inline]
    pub fn contains(&self, id: ID) -> bool {
        self.changes
            .get(&id.client_id)
            .map_or(0, |changes| changes.len())
            > id.counter as usize
    }

    #[inline]
    fn get_next_counter(&self, client_id: ClientID) -> Counter {
        self.changes
            .get(&client_id)
            .map(|changes| changes.len())
            .unwrap_or(0) as Counter
    }

    #[inline]
    pub(crate) fn iter_client_op(&self, client_id: ClientID) -> iter::ClientOpIter<'_> {
        iter::ClientOpIter {
            change_index: 0,
            op_index: 0,
            changes: self.changes.get(&client_id),
        }
    }

    #[inline]
    pub(crate) fn iter_op(&self) -> iter::OpIter<'_> {
        iter::OpIter::new(&self.changes)
    }

    #[inline(always)]
    pub fn get_vv(&self) -> &VersionVector {
        &self.vv
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
        &self.frontier
    }

    fn vv(&self) -> crate::VersionVector {
        self.vv.clone()
    }
}

fn check_import_change_valid(change: &Change) {
    for op in change.ops.iter() {
        if let Some((slice, _)) = op
            .content
            .as_normal()
            .and_then(|x| x.as_list())
            .and_then(|x| x.as_insert())
        {
            assert!(slice.as_raw_str().is_some())
        }
    }
}

//! [LogStore] stores all the [Change]s and [Op]s. It's also a [DAG][crate::dag];
//!
//!
mod encoding;
mod iter;
use std::{
    marker::PhantomPinned,
    sync::{Arc, Mutex, RwLock, Weak},
};

use fxhash::{FxHashMap, FxHashSet};

use rle::{HasLength, RleVec, RleVecWithIndex, Sliceable};

use smallvec::SmallVec;

use crate::{
    change::{Change, ChangeMergeCfg},
    configure::Configure,
    container::{
        registry::{ContainerInstance, ContainerRegistry},
        text::text_content::ListSlice,
        Container, ContainerID,
    },
    dag::Dag,
    debug_log,
    id::{ClientID, ContainerIdx, Counter},
    op::{Content, OpContent, RemoteOp},
    span::{HasCounterSpan, HasIdSpan, HasLamportSpan, IdSpan},
    ContainerType, Lamport, Op, Timestamp, VersionVector, ID,
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

pub(crate) type LogStoreRef = Arc<RwLock<LogStore>>;
pub(crate) type LogStoreWeakRef = Weak<RwLock<LogStore>>;

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
    frontier: SmallVec<[ID; 2]>,
    /// CRDT container manager
    pub(crate) reg: ContainerRegistry,
    _pin: PhantomPinned,
}

impl LogStore {
    pub(crate) fn new(mut cfg: Configure, client_id: Option<ClientID>) -> Arc<RwLock<Self>> {
        let this_client_id = client_id.unwrap_or_else(|| cfg.rand.next_u64());
        Arc::new(RwLock::new(Self {
            cfg,
            this_client_id,
            changes: FxHashMap::default(),
            latest_lamport: 0,
            latest_timestamp: 0,
            frontier: Default::default(),
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

    pub fn import(&mut self, mut changes: Vec<Change<RemoteOp>>) {
        let self_vv = self.vv();
        // guarantee that changes are applied in causal order
        changes.sort_by_cached_key(|x| x.lamport);
        for change in changes
            .into_iter()
            .filter(|x| !self_vv.includes_id(x.id_last()))
        {
            check_import_change_valid(&change);
            // TODO: cache pending changes
            assert!(change.deps.iter().all(|x| self.vv().includes_id(*x)));
            self.apply_remote_change(change)
        }
    }

    pub fn export(&self, remote_vv: &VersionVector) -> Vec<Change<RemoteOp>> {
        let mut ans = Vec::default();
        let self_vv = self.vv();
        let diff = self_vv.diff(remote_vv);
        for span in diff.left.iter() {
            let changes = self.get_changes_slice(span.id_span());
            for change in changes.iter() {
                ans.push(self.change_to_export_format(change))
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

    fn change_to_imported_format(&mut self, change: Change<RemoteOp>) -> Change {
        let mut new_ops = RleVec::new();
        for mut op in change.ops.into_iter() {
            let container = self.reg.get_or_create(&op.container);
            let mut container = container.lock().unwrap();
            container.to_import(&mut op);
            drop(container);
            for op in op.convert(self) {
                new_ops.push(op);
            }
        }

        Change {
            ops: new_ops,
            deps: change.deps,
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
        container.to_export(&mut op);
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
            OpContent::Normal {
                content: Content::Container(container_id.clone()),
            },
            parent_idx,
        )]);
        self.reg.get_or_create(&container_id);
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
    pub fn frontier(&self) -> &[ID] {
        &self.frontier
    }

    fn update_frontier(&mut self, clear: &[ID], new: &[ID]) {
        self.frontier.retain(|x| {
            !clear
                .iter()
                .any(|y| x.client_id == y.client_id && x.counter <= y.counter)
                && !new
                    .iter()
                    .any(|y| x.client_id == y.client_id && x.counter <= y.counter)
        });
        for next in new.iter() {
            if self
                .frontier
                .iter()
                .any(|x| x.client_id == next.client_id && x.counter >= next.counter)
            {
                continue;
            }

            self.frontier.push(*next);
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
            deps: std::mem::replace(&mut self.frontier, smallvec::smallvec![last_id]),
            ops: ops.into(),
            lamport,
            timestamp,
        };

        self.latest_lamport = lamport + change.content_len() as u32 - 1;
        self.latest_timestamp = timestamp;
        self.vv.set_end(change.id_end());
        self.changes
            .entry(self.this_client_id)
            .or_insert_with(|| RleVecWithIndex::new_with_conf(ChangeMergeCfg::new()))
            .push(change);

        debug_log!("CHANGES---------------- site {}", self.this_client_id);
    }

    pub fn apply_remote_change(&mut self, change: Change<RemoteOp>) {
        if self.contains(change.id_last()) {
            return;
        }

        debug_log!("Client {} Apply {:#?}", self.this_client_id, &change);
        for dep in &change.deps {
            if !self.contains(*dep) {
                unimplemented!("need impl pending changes");
            }
        }

        // TODO: find a way to remove this clone? we don't need change in apply method actually
        let change = self.change_to_imported_format(change);
        let changes = self
            .changes
            .entry(change.id.client_id)
            .or_insert_with(RleVecWithIndex::new);
        changes.push(change);
        // TODO: avoid this clone?
        let change = changes.vec().last().unwrap().clone();

        // Apply ops.
        // NOTE: applying expects that log_store has store the Change, and updated self vv
        let mut set = FxHashSet::default();
        for op in change.ops.iter() {
            set.insert(&op.container);
        }

        for container in set {
            let mut container = self.reg.get_by_idx(*container).unwrap().lock().unwrap();
            container.apply(change.id_span(), self);
        }

        self.vv.set_end(change.id_end());
        self.update_frontier(&change.deps, &[change.id_last()]);

        if change.lamport_last() > self.latest_lamport {
            self.latest_lamport = change.lamport_last();
        }

        if change.timestamp > self.latest_timestamp {
            self.latest_timestamp = change.timestamp;
        }
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

    #[inline(always)]
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
        &self.frontier
    }

    fn vv(&self) -> crate::VersionVector {
        self.vv.clone()
    }
}

fn check_import_change_valid(change: &Change<RemoteOp>) {
    if cfg!(test) {
        for op in change.ops.iter() {
            for content in op.contents.iter() {
                if let Some((slice, _)) = content
                    .as_normal()
                    .and_then(|x| x.as_list())
                    .and_then(|x| x.as_insert())
                {
                    assert!(matches!(
                        slice,
                        ListSlice::RawData(_) | ListSlice::RawStr(_) | ListSlice::Unknown(_)
                    ))
                }
            }
        }
    }
}

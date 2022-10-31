//! [LogStore] stores all the [Change]s and [Op]s. It's also a [DAG][crate::dag];
//!
//!
mod iter;
use std::marker::PhantomPinned;

use fxhash::{FxHashMap, FxHashSet};

use rle::{HasLength, RleVecWithIndex, Sliceable};

use smallvec::SmallVec;

use crate::{
    change::{Change, ChangeMergeCfg},
    configure::Configure,
    container::{manager::ContainerManager, Container, ContainerID},
    dag::Dag,
    debug_log,
    id::{ClientID, Counter},
    isomorph::{Irc, IsoRw, IsoWeak},
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

pub(crate) type LogStoreRef = Irc<IsoRw<LogStore>>;
pub(crate) type LogStoreWeakRef = IsoWeak<IsoRw<LogStore>>;

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
    pub(crate) container: IsoWeak<IsoRw<ContainerManager>>,
    to_self: IsoWeak<IsoRw<LogStore>>,
    _pin: PhantomPinned,
}

impl LogStore {
    pub(crate) fn new(
        mut cfg: Configure,
        client_id: Option<ClientID>,
        container: IsoWeak<IsoRw<ContainerManager>>,
    ) -> Irc<IsoRw<Self>> {
        let this_client_id = client_id.unwrap_or_else(|| cfg.rand.next_u64());
        Irc::new_cyclic(|x| {
            IsoRw::new(Self {
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
            assert!(change.deps.iter().all(|x| self.vv().includes_id(*x)));
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
        &self,
        container_manager: &mut ContainerManager,
        change: &mut Change,
    ) {
        for op in change.ops.vec_mut().iter_mut() {
            let container = container_manager
                .get_or_create(&op.container, self.to_self.clone())
                .unwrap();
            container.to_import(op);
        }
    }

    fn change_to_export_format(&self, change: &mut Change) {
        let upgraded = self.container.upgrade().unwrap();
        let container_manager = upgraded.read();
        for op in change.ops.vec_mut().iter_mut() {
            let container = container_manager.get(&op.container).unwrap();
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
        let last_id = ops.last().unwrap().id_last();
        let change = Change {
            id,
            deps: std::mem::replace(&mut self.frontier, smallvec::smallvec![last_id]),
            ops: ops.into(),
            lamport,
            timestamp,
            break_points: Default::default(),
        };

        self.latest_lamport = lamport + change.content_len() as u32 - 1;
        self.latest_timestamp = timestamp;
        self.vv.set_end(change.id_end());
        self.changes
            .entry(self.this_client_id)
            .or_insert_with(|| RleVecWithIndex::new_with_conf(ChangeMergeCfg::new(true)))
            .push(change);

        debug_log!("CHANGES---------------- site {}", self.this_client_id);
    }

    pub fn apply_remote_change(&mut self, mut change: Change) {
        if self.contains(change.last_id()) {
            return;
        }

        for dep in &change.deps {
            if !self.contains(*dep) {
                unimplemented!("need impl pending changes");
            }
        }

        // TODO: find a way to remove this clone? we don't need change in apply method actually
        let upgraded = self.container.upgrade().unwrap();
        let mut container_manager = upgraded.write();
        #[cfg(feature = "slice")]
        self.change_to_imported_format(&mut container_manager, &mut change);
        let v = self
            .changes
            .entry(change.id.client_id)
            .or_insert_with(RleVecWithIndex::new);
        v.push(change);
        let change = v.vec().last().unwrap().clone();

        // Apply ops.
        // NOTE: applying expects that log_store has store the Change, and updated self vv
        let mut set = FxHashSet::default();
        for op in change.ops.iter() {
            set.insert(&op.container);
        }

        for container in set {
            let container = container_manager
                .get_or_create(container, self.to_self.clone())
                .unwrap();
            container.apply(change.id_span(), self);
        }

        drop(container_manager);
        self.vv.set_end(change.id_end());
        self.update_frontier(&change.deps, &[change.last_id()]);

        if change.last_lamport() > self.latest_lamport {
            self.latest_lamport = change.last_lamport();
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
        iter::OpSpanIter::new(&self.changes, id_span, container)
    }

    #[inline(always)]
    pub fn get_vv(&self) -> &VersionVector {
        &self.vv
    }

    #[cfg(feature = "fuzzing")]
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

//! [LogStore] stores all the [Change]s and [Op]s. It's also a [DAG][crate::dag];
//!
//!
mod iter;
use pin_project::pin_project;
use std::{collections::BinaryHeap, marker::PhantomPinned, pin::Pin, ptr::NonNull};

use fxhash::FxHashMap;
use moveit::New;
use ring::rand::SystemRandom;
use rle::{HasLength, RleVec};
use smallvec::SmallVec;
use string_cache::{Atom, DefaultAtom, EmptyStaticAtomSet};

use crate::{
    change::{Change, ChangeMergeCfg},
    configure::Configure,
    container::{Container, ContainerID, ContainerManager},
    id::{ClientID, Counter},
    op::OpProxy,
    span::IdSpan,
    version::TotalOrderStamp,
    Lamport, Op, Timestamp, ID,
};
const YEAR: u64 = 365 * 24 * 60 * 60;
const MONTH: u64 = 30 * 24 * 60 * 60;

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

/// LogStore stores the full history of Loro
///
/// This is a self-referential structure. So it need to be pinned.
///
/// `frontier`s are the Changes without children in the DAG (there is no dep pointing to them)
///
/// TODO: Refactor we need to move the things about the current state out of LogStore (container, latest_lamport, ..)
#[pin_project]
pub struct LogStore {
    changes: FxHashMap<ClientID, RleVec<Change, ChangeMergeCfg>>,
    cfg: Configure,
    latest_lamport: Lamport,
    latest_timestamp: Timestamp,
    pub(crate) this_client_id: ClientID,
    frontier: SmallVec<[ID; 2]>,

    /// CRDT container manager
    pub(crate) container: ContainerManager,

    _pin: PhantomPinned,
}

impl LogStore {
    pub fn new(mut cfg: Configure, client_id: Option<ClientID>) -> Pin<Box<Self>> {
        let this_client_id = client_id.unwrap_or_else(|| cfg.rand.next_u64());
        let mut this = Box::pin(Self {
            cfg,
            this_client_id,
            changes: FxHashMap::default(),
            latest_lamport: 0,
            latest_timestamp: 0,
            container: ContainerManager {
                containers: Default::default(),
                store: NonNull::dangling(),
            },
            frontier: Default::default(),
            _pin: PhantomPinned,
        });

        let p = this.as_ref().get_ref();
        let p = p as *const _ as *mut LogStore;
        this.container.store = unsafe { NonNull::new_unchecked(p) };
        this
    }

    #[inline]
    pub fn lookup_change(&self, id: ID) -> Option<&Change> {
        self.changes
            .get(&id.client_id)
            .map(|changes| changes.get(id.counter as usize).unwrap().element)
    }

    #[inline(always)]
    pub fn next_lamport(&self) -> Lamport {
        self.latest_lamport + 1
    }

    #[inline(always)]
    pub fn next_id(&self, client_id: ClientID) -> ID {
        ID {
            client_id,
            counter: self.get_next_counter(client_id),
        }
    }

    pub fn append_local_ops(&mut self, ops: Vec<Op>) {
        let lamport = self.next_lamport();
        let timestamp = (self.cfg.get_time)();
        let id = ID {
            client_id: self.this_client_id,
            counter: self.get_next_counter(self.this_client_id),
        };
        let mut change = Change {
            id,
            ops: ops.into(),
            deps: std::mem::take(&mut self.frontier),
            lamport,
            timestamp,
            freezed: false,
            break_points: Default::default(),
        };

        change.deps.push(ID::new(
            self.this_client_id,
            id.counter + change.len() as Counter - 1,
        ));
        self.latest_lamport = lamport + change.len() as u32 - 1;
        self.latest_timestamp = timestamp;
        self.changes
            .entry(self.this_client_id)
            .or_insert_with(RleVec::new)
            .push(change);
    }

    pub fn apply_remote_change(self: &mut Pin<&mut Self>, mut change: Change) {
        change.freezed = true;
        if self.contains(change.last_id()) {
            return;
        }

        for dep in &change.deps {
            if !self.contains(*dep) {
                unimplemented!("need impl pending changes");
            }
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

        for op in change.ops.iter() {
            self.apply_remote_op(&change, op);
        }

        self.push_change(change);
    }

    #[inline]
    fn push_change(&mut self, change: Change) {
        self.changes
            .entry(change.id.client_id)
            .or_insert_with(RleVec::new)
            .push(change);
    }

    /// this function assume op is not included in the log, and its deps are included.
    #[inline]
    fn apply_remote_op(self: &mut Pin<&mut Self>, change: &Change, op: &Op) {
        let container = self.container.get_or_create(op.container());
        container.get_mut().apply(&OpProxy::new(change, op, None));
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
}

use std::{
    cell::RefCell,
    rc::Rc,
    sync::{Arc, Mutex, RwLock, Weak},
};

use crate::{
    container::{registry::ContainerIdx, ContainerID},
    event::{Diff, EventDiff, RawEvent},
    hierarchy::Hierarchy,
    id::PeerID,
    log_store::{LoroEncoder, RemoteClientChanges},
    version::Frontiers,
    ContainerType, InternalString, List, LogStore, LoroCore, LoroError, Map, Text,
};
use fxhash::FxHashMap;
use serde::Serialize;
use smallvec::SmallVec;

pub trait Transact {
    fn transact(&self) -> TransactionWrap;
    fn transact_with(&self, origin: Option<Origin>) -> TransactionWrap;
}

impl Transact for LoroCore {
    fn transact(&self) -> TransactionWrap {
        TransactionWrap(Rc::new(RefCell::new(Transaction::new(
            Arc::downgrade(&self.log_store),
            Arc::downgrade(&self.hierarchy),
        ))))
    }

    fn transact_with(&self, origin: Option<Origin>) -> TransactionWrap {
        TransactionWrap(Rc::new(RefCell::new(
            Transaction::new(
                Arc::downgrade(&self.log_store),
                Arc::downgrade(&self.hierarchy),
            )
            .set_origin(origin),
        )))
    }
}

impl Transact for TransactionWrap {
    fn transact(&self) -> TransactionWrap {
        TransactionWrap(Rc::clone(&self.0))
    }

    fn transact_with(&self, _origin: Option<Origin>) -> TransactionWrap {
        unreachable!()
    }
}

impl AsMut<Transaction> for Transaction {
    fn as_mut(&mut self) -> &mut Transaction {
        self
    }
}

pub struct TransactionWrap(pub(crate) Rc<RefCell<Transaction>>);

impl TransactionWrap {
    pub fn get_text_by_idx(&self, idx: ContainerIdx) -> Option<Text> {
        let txn = self.0.borrow();
        let instance = txn.with_store(|s| s.get_container_by_idx(&idx));
        instance.map(|i| Text::from_instance(i, txn.client_id))
    }

    pub fn get_list_by_idx(&self, idx: ContainerIdx) -> Option<List> {
        let txn = self.0.borrow();
        let instance = txn.with_store(|s| s.get_container_by_idx(&idx));
        instance.map(|i| List::from_instance(i, txn.client_id))
    }

    pub fn get_map_by_idx(&self, idx: ContainerIdx) -> Option<Map> {
        let txn = self.0.borrow();
        let instance = txn.with_store(|s| s.get_container_by_idx(&idx));
        instance.map(|i| Map::from_instance(i, txn.client_id))
    }

    pub fn commit(&self) -> Result<(), LoroError> {
        self.0.borrow_mut().commit()
    }

    pub fn decode(&mut self, input: &[u8]) -> Result<(), LoroError> {
        let mut txn = self.0.borrow_mut();
        txn.decode(input)
    }
}

// TODO: use String as Origin for now
#[derive(Debug, Clone, Serialize)]
pub struct Origin(InternalString);

impl Origin {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<T: AsRef<str>> From<T> for Origin {
    fn from(value: T) -> Self {
        Self(value.as_ref().into())
    }
}

pub struct Transaction {
    pub(crate) client_id: PeerID,
    pub(crate) store: Weak<RwLock<LogStore>>,
    pub(crate) hierarchy: Weak<Mutex<Hierarchy>>,
    pub(crate) origin: Option<Origin>,
    pending_event_diff: FxHashMap<ContainerID, FxHashMap<bool, Diff>>,
    start_frontier: Frontiers,
    committed: bool,
}

impl Transaction {
    pub(crate) fn new(store: Weak<RwLock<LogStore>>, hierarchy: Weak<Mutex<Hierarchy>>) -> Self {
        let (client_id, start_vv): (u64, Frontiers) = {
            let store = store.upgrade().unwrap();
            let store = store.try_read().unwrap();
            (store.this_client_id, store.frontiers().clone())
        };
        Self {
            client_id,
            store,
            hierarchy,
            pending_event_diff: Default::default(),
            start_frontier: start_vv,
            origin: None,
            committed: false,
        }
    }

    pub(crate) fn set_origin(mut self, origin: Option<Origin>) -> Self {
        self.origin = origin;
        self
    }

    pub(crate) fn with_store<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&LogStore) -> R,
    {
        let store = self.store.upgrade().unwrap();
        let store = store.try_read().unwrap();
        f(&store)
    }

    #[allow(unused)]
    pub(crate) fn with_store_mut<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut LogStore) -> R,
    {
        let store = self.store.upgrade().unwrap();
        let mut store = store.try_write().unwrap();
        f(&mut store)
    }

    #[allow(unused)]
    pub(crate) fn with_hierarchy<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&Hierarchy) -> R,
    {
        let hierarchy = self.hierarchy.upgrade().unwrap();
        let hierarchy = hierarchy.try_lock().unwrap();
        f(&hierarchy)
    }

    #[allow(unused)]
    pub(crate) fn with_hierarchy_mut<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut Hierarchy) -> R,
    {
        let hierarchy = self.hierarchy.upgrade().unwrap();
        let mut hierarchy = hierarchy.try_lock().unwrap();
        f(&mut hierarchy)
    }

    pub(crate) fn with_store_hierarchy_mut<F, R>(&mut self, f: F) -> R
    where
        F: FnOnce(&mut Self, &mut LogStore, &mut Hierarchy) -> R,
    {
        let store = self.store.upgrade().unwrap();
        let mut store = store.try_write().unwrap();
        let hierarchy = self.hierarchy.upgrade().unwrap();
        let mut hierarchy = hierarchy.try_lock().unwrap();
        f(self, &mut store, &mut hierarchy)
    }

    pub(crate) fn append_event_diff(&mut self, id: &ContainerID, diff: Diff, local: bool) {
        // cache events
        if let Some(old_diff) = self.pending_event_diff.get_mut(id) {
            if let Some(old_diff) = old_diff.get_mut(&local) {
                compose_two_event_diff(old_diff, diff);
                return;
            }
        }

        if !self.pending_event_diff.contains_key(id) {
            self.pending_event_diff
                .insert(id.clone(), Default::default());
        }

        self.pending_event_diff
            .get_mut(id)
            .unwrap()
            .insert(local, diff);
    }

    fn append_batch_event_diff(&mut self, events: Vec<EventDiff>) {
        for event in events {
            let EventDiff { id, diff, local } = event;
            for d in diff {
                self.append_event_diff(&id, d, local);
            }
        }
    }

    fn emit_events(&mut self) {
        if self.pending_event_diff.is_empty() {
            return;
        }

        let pending_events = std::mem::take(&mut self.pending_event_diff);
        let mut events: SmallVec<[_; 2]> = SmallVec::new();
        self.with_store_hierarchy_mut(|txn, store, hierarchy| {
            for (id, event) in pending_events {
                for (local, diff) in event {
                    if let Some(abs_path) = hierarchy.get_abs_path(&store.reg, &id) {
                        let event = RawEvent {
                            diff,
                            local,
                            old_version: txn.start_frontier.clone(),
                            new_version: store.frontiers().clone(),
                            container_id: id.clone(),
                            abs_path,
                            origin: txn.origin.as_ref().cloned(),
                        };
                        events.push(event);
                    }
                }
            }
        });

        let hierarchy = self.hierarchy.upgrade().unwrap();
        // notify event in the order of path length
        // otherwise, the paths to children may be incorrect when the parents are affected by some of the events
        events.sort_by_cached_key(|x| x.abs_path.len());
        for event in events {
            Hierarchy::notify_without_lock(&hierarchy, event);
        }
    }

    pub(crate) fn register_container(
        &mut self,
        parent_id: &ContainerID,
        type_: ContainerType,
    ) -> (ContainerID, ContainerIdx) {
        self.with_store_hierarchy_mut(|_txn, s, h| {
            let (container_id, idx) = s.create_container(type_);
            h.add_child(parent_id, &container_id);
            (container_id, idx)
        })
    }

    pub(crate) fn delete_container(&mut self, id: &ContainerID) {
        self.pending_event_diff.remove(id);
    }

    pub(crate) fn import(&mut self, changes: RemoteClientChanges<'static>) {
        self.with_store_hierarchy_mut(|txn, store, hierarchy| {
            let events = store.import(hierarchy, changes);
            txn.append_batch_event_diff(events);
        });
    }

    pub fn decode(&mut self, input: &[u8]) -> Result<(), LoroError> {
        self.with_store_hierarchy_mut(|txn, store, hierarchy| {
            let events = LoroEncoder::decode(store, hierarchy, input)?;
            txn.append_batch_event_diff(events);
            Ok(())
        })
    }

    pub fn decode_batch(&mut self, input: &[Vec<u8>]) -> Result<(), LoroError> {
        self.with_store_hierarchy_mut(|txn, store, hierarchy| {
            let events = LoroEncoder::decode_batch(store, hierarchy, input)?;
            txn.append_batch_event_diff(events);
            Ok(())
        })
    }

    pub fn commit(&mut self) -> Result<(), LoroError> {
        if self.committed {
            return Err(LoroError::TransactionError(
                "Transaction already committed".into(),
            ));
        }
        self.committed = true;
        self.emit_events();
        Ok(())
    }
}

impl Drop for Transaction {
    fn drop(&mut self) {
        if !self.committed {
            self.commit().unwrap();
        }
    }
}

// TODO: perf slow
fn compose_two_event_diff(this_diff: &mut Diff, other_diff: Diff) {
    let diff = match other_diff {
        Diff::List(x) => {
            let inner = std::mem::take(this_diff.as_list_mut().unwrap());
            let diff = inner.compose(x);
            Diff::List(diff)
        }
        Diff::Map(x) => {
            let inner = std::mem::take(this_diff.as_map_mut().unwrap());
            let diff = inner.compose(x);
            Diff::Map(diff)
        }
        Diff::Text(x) => {
            let inner = std::mem::take(this_diff.as_text_mut().unwrap());
            let diff = inner.compose(x);
            Diff::Text(diff)
        }
        Diff::NewMap(x) => {
            let inner = std::mem::take(this_diff.as_new_map_mut().unwrap());
            let diff = inner.compose(x);
            Diff::NewMap(diff)
        }
    };
    *this_diff = diff;
}

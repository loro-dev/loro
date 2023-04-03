use std::{
    cell::RefCell,
    collections::BTreeMap,
    rc::Rc,
    sync::{Arc, Mutex, MutexGuard, RwLock, RwLockWriteGuard, Weak},
};

use crate::{
    container::{registry::ContainerIdx, ContainerID},
    event::{Diff, RawEvent},
    hierarchy::Hierarchy,
    id::ClientID,
    log_store::LoroEncoder,
    version::Frontiers,
    ContainerType, InternalString, List, LogStore, LoroCore, LoroError, Map, Text,
};
use fxhash::FxHashMap;
use serde::Serialize;
use smallvec::{smallvec, SmallVec};

pub trait Transact {
    fn transact<'s: 'a, 'a>(&'s self) -> TransactionWrap<'a>;
    fn transact_with<'s: 'a, 'a>(&'s self, origin: Option<Origin>) -> TransactionWrap<'a>;
}

impl Transact for LoroCore {
    fn transact<'s: 'a, 'a>(&'s self) -> TransactionWrap<'a> {
        let store = self.log_store.try_write().unwrap();
        let hierarchy = self.hierarchy.try_lock().unwrap();
        TransactionWrap(Rc::new(RefCell::new(Transaction::new(store, hierarchy))))
    }

    fn transact_with<'s: 'a, 'a>(&'s self, origin: Option<Origin>) -> TransactionWrap<'a> {
        let store = self.log_store.try_write().unwrap();
        let hierarchy = self.hierarchy.try_lock().unwrap();
        TransactionWrap(Rc::new(RefCell::new(
            Transaction::new(store, hierarchy).set_origin(origin),
        )))
    }
}

impl<'pre> Transact for TransactionWrap<'pre> {
    fn transact<'s: 'a, 'a>(&'s self) -> TransactionWrap<'a> {
        // use unsafe to avoid lifetime issues 'a: 'pre
        // Safety: we are cloning the Rc, so the lifetime is not affected
        unsafe { std::mem::transmute(TransactionWrap(Rc::clone(&self.0))) }
    }

    fn transact_with<'s: 'a, 'a>(&'s self, origin: Option<Origin>) -> TransactionWrap<'a> {
        unreachable!()
    }
}

impl<'a> AsMut<Transaction<'a>> for Transaction<'a> {
    fn as_mut(&mut self) -> &mut Transaction<'a> {
        self
    }
}

pub struct TransactionWrap<'a>(pub(crate) Rc<RefCell<Transaction<'a>>>);

impl<'a> TransactionWrap<'a> {
    pub fn get_text_by_idx(&self, idx: ContainerIdx) -> Option<Text> {
        let txn = self.0.borrow();
        let instance = txn.store.get_container_by_idx(&idx);
        instance.map(|i| Text::from_instance(i, txn.client_id))
    }

    pub fn get_list_by_idx(&self, idx: ContainerIdx) -> Option<List> {
        let txn = self.0.borrow();
        let instance = txn.store.get_container_by_idx(&idx);
        instance.map(|i| List::from_instance(i, txn.client_id))
    }

    pub fn get_map_by_idx(&self, idx: ContainerIdx) -> Option<Map> {
        let txn = self.0.borrow();
        let instance = txn.store.get_container_by_idx(&idx);
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

pub struct Transaction<'a> {
    pub(crate) client_id: ClientID,
    pub(crate) store: RwLockWriteGuard<'a, LogStore>,
    pub(crate) hierarchy: MutexGuard<'a, Hierarchy>,
    pub(crate) origin: Option<Origin>,
    // sort by [ContainerIdx]
    // TODO Origin, now use local bool
    pending_event_diff: BTreeMap<ContainerIdx, FxHashMap<bool, Diff>>,
    start_frontier: Frontiers,
    latest_frontier: Frontiers,
    committed: bool,
}

impl<'a> Transaction<'a> {
    pub(crate) fn new(
        store: RwLockWriteGuard<'a, LogStore>,
        hierarchy: MutexGuard<'a, Hierarchy>,
    ) -> Self {
        let (client_id, start_vv): (u64, Frontiers) =
            { (store.this_client_id, store.frontiers().into()) };
        Self {
            client_id,
            store,
            hierarchy,
            pending_event_diff: Default::default(),
            latest_frontier: start_vv.clone(),
            start_frontier: start_vv,
            origin: None,
            committed: false,
        }
    }

    pub(crate) fn store_mut(&mut self) -> &mut LogStore {
        &mut self.store
    }

    pub(crate) fn hierarchy_mut(&mut self) -> &mut Hierarchy {
        &mut self.hierarchy
    }

    pub(crate) fn set_origin(mut self, origin: Option<Origin>) -> Self {
        self.origin = origin;
        self
    }

    pub(crate) fn update_version(&mut self, new_version: Frontiers) {
        self.latest_frontier = new_version;
    }

    pub(crate) fn append_event_diff(&mut self, idx: ContainerIdx, diff: Diff, local: bool) {
        // cache events
        if let Some(old_diff) = self.pending_event_diff.get_mut(&idx) {
            if let Some(old_diff) = old_diff.get_mut(&local) {
                // println!("old event {:?}", old_diff);
                // println!("new event {:?}", diff);
                compose_two_event_diff(old_diff, diff);
                // println!("res {:?}\n", old_diff)
                return;
            }
        }
        self.pending_event_diff
            .entry(idx)
            .or_insert_with(FxHashMap::default)
            .insert(local, diff);
    }

    fn emit_events(&mut self) {
        if self.pending_event_diff.is_empty() {
            return;
        }

        let pending_events = std::mem::take(&mut self.pending_event_diff);
        let mut events: SmallVec<[_; 2]> = SmallVec::new();

        for (idx, event) in pending_events {
            let id = self.store.reg.get_id(idx).unwrap();
            for (local, diff) in event {
                if let Some(abs_path) = self.hierarchy.get_abs_path(&self.store.reg, id) {
                    let event = RawEvent {
                        diff: smallvec![diff],
                        local,
                        old_version: self.start_frontier.clone(),
                        new_version: self.latest_frontier.clone(),
                        container_id: id.clone(),
                        abs_path,
                        origin: self.origin.as_ref().cloned(),
                    };
                    events.push(event);
                }
            }
        }

        for event in events {
            Hierarchy::notify_without_lock_impl(&mut self.hierarchy, event);
        }
    }

    pub(crate) fn register_container(
        &mut self,
        parent_id: &ContainerID,
        type_: ContainerType,
    ) -> (ContainerID, ContainerIdx) {
        let (container_id, idx) = self.store.create_container(type_);
        self.hierarchy.add_child(parent_id, &container_id);
        (container_id, idx)
    }

    pub(crate) fn delete_container(&mut self, idx: ContainerIdx) {
        self.pending_event_diff.remove(&idx);
    }

    pub fn decode(&mut self, input: &[u8]) -> Result<(), LoroError> {
        let events = LoroEncoder::decode(&mut self.store, &mut self.hierarchy, input)?;
        // TODO decode just gets diff
        for event in events {
            let idx = self.store.get_container_idx(&event.container_id).unwrap();
            let local = event.local;
            for d in event.diff {
                self.append_event_diff(idx, d, local);
            }
        }
        Ok(())
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

impl<'a> Drop for Transaction<'a> {
    fn drop(&mut self) {
        if !self.committed {
            self.commit().unwrap();
        }
    }
}

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
    };
    *this_diff = diff;
}

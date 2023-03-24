use std::{
    cell::RefCell,
    collections::BTreeMap,
    rc::Rc,
    sync::{Arc, Mutex, RwLock, Weak},
};

use crate::{
    container::{registry::ContainerIdx, ContainerID},
    event::{Diff, RawEvent},
    hierarchy::Hierarchy,
    id::{ClientID, ID},
    log_store::LoroEncoder,
    version::Frontiers,
    ContainerType, InternalString, List, LogStore, LoroCore, LoroError, Map, Text,
};
use fxhash::FxHashMap;
use serde::Serialize;
use smallvec::smallvec;

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
            .with_origin(origin),
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
    pub(crate) client_id: ClientID,
    pub(crate) store: Weak<RwLock<LogStore>>,
    pub(crate) hierarchy: Weak<Mutex<Hierarchy>>,
    pub(crate) origin: Option<Origin>,
    pending_ops: FxHashMap<ContainerIdx, Vec<ID>>,
    // sort by [ContainerIdx]
    // TODO Origin, now use local bool
    pending_event_diff: BTreeMap<ContainerIdx, FxHashMap<bool, Diff>>,
    start_vv: Frontiers,
    latest_vv: Frontiers,
    committed: bool,
}

impl Transaction {
    pub(crate) fn new(store: Weak<RwLock<LogStore>>, hierarchy: Weak<Mutex<Hierarchy>>) -> Self {
        let (client_id, start_vv): (u64, Frontiers) = {
            let store = store.upgrade().unwrap();
            let store = store.try_read().unwrap();
            (store.this_client_id, store.frontiers().into())
        };
        Self {
            client_id,
            store,
            hierarchy,
            pending_ops: Default::default(),
            pending_event_diff: Default::default(),
            latest_vv: start_vv.clone(),
            start_vv,
            origin: None,
            committed: false,
        }
    }

    pub(crate) fn with_origin(mut self, origin: Option<Origin>) -> Self {
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

    pub(crate) fn update_version(&mut self, new_version: Frontiers) {
        self.latest_vv = new_version;
    }

    pub(crate) fn push(&mut self, idx: ContainerIdx, op_id: ID) {
        self.pending_ops
            .entry(idx)
            .or_insert_with(Vec::new)
            .push(op_id);
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
        let pending_events = std::mem::take(&mut self.pending_event_diff);
        let mut events = Vec::with_capacity(pending_events.len() * 2);
        self.with_store_hierarchy_mut(|txn, store, hierarchy| {
            for (idx, event) in pending_events {
                let id = store.reg.get_id(idx).unwrap();
                for (local, diff) in event {
                    if let Some(abs_path) = hierarchy.get_abs_path(&store.reg, id) {
                        let event = RawEvent {
                            diff: smallvec![diff],
                            local,
                            old_version: txn.start_vv.clone(),
                            new_version: txn.latest_vv.clone(),
                            container_id: id.clone(),
                            abs_path,
                            origin: txn.origin.as_ref().cloned(),
                        };
                        events.push(event);
                    }
                }
            }
        });
        for event in events {
            let hierarchy = self.hierarchy.upgrade().unwrap();
            Hierarchy::notify_without_lock(hierarchy, event);
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

    pub(crate) fn delete_container(&mut self, idx: ContainerIdx) {
        self.pending_event_diff.remove(&idx);
    }

    pub fn decode(&mut self, input: &[u8]) -> Result<(), LoroError> {
        let store = self.store.upgrade().unwrap();
        let mut store = store.try_write().unwrap();
        let hierarchy = self.hierarchy.upgrade().unwrap();
        let mut hierarchy = hierarchy.try_lock().unwrap();
        let events = LoroEncoder::decode(&mut store, &mut hierarchy, input)?;
        // TODO decode just gets diff
        for event in events {
            let idx = store.get_container_idx(&event.container_id).unwrap();
            let local = event.local;
            for d in event.diff {
                self.append_event_diff(idx, d, local);
            }
        }
        Ok(())
    }

    pub fn commit(&mut self) {
        if self.committed {
            return;
        }
        self.committed = true;
        self.emit_events();
    }
}

impl Drop for Transaction {
    fn drop(&mut self) {
        self.commit()
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

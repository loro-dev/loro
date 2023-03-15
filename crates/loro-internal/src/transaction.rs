use std::{
    cell::RefCell,
    collections::BTreeMap,
    rc::Rc,
    sync::{Arc, Mutex, RwLock, Weak},
};

use fxhash::{FxHashMap, FxHashSet};

use crate::{
    container::{registry::ContainerIdx, ContainerID},
    event::{Diff, RawEvent},
    hierarchy::Hierarchy,
    id::{ClientID, ID},
    log_store::LoroEncoder,
    version::Frontiers,
    ContainerType, LogStore, LoroCore, LoroError,
};

pub trait Transact {
    fn transact(&self) -> TransactionWrap;
}

impl Transact for LoroCore {
    fn transact(&self) -> TransactionWrap {
        TransactionWrap(Rc::new(RefCell::new(Transaction::new(
            Arc::downgrade(&self.log_store),
            Arc::downgrade(&self.hierarchy),
        ))))
    }
}

impl Transact for TransactionWrap {
    fn transact(&self) -> TransactionWrap {
        TransactionWrap(Rc::clone(&self.0))
    }
}

impl AsMut<Transaction> for Transaction {
    fn as_mut(&mut self) -> &mut Transaction {
        self
    }
}

pub struct TransactionWrap(pub(crate) Rc<RefCell<Transaction>>);

pub struct Transaction {
    pub(crate) client_id: ClientID,
    pub(crate) store: Weak<RwLock<LogStore>>,
    pub(crate) hierarchy: Weak<Mutex<Hierarchy>>,
    // sort by [ContainerIdx]
    pending_ops: FxHashMap<ContainerIdx, Vec<ID>>,
    created_container: FxHashMap<ContainerIdx, FxHashSet<ContainerIdx>>,
    deleted_container: FxHashSet<ContainerIdx>,
    pending_events: BTreeMap<ContainerIdx, RawEvent>,
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
            created_container: Default::default(),
            deleted_container: Default::default(),
            pending_events: Default::default(),
            latest_vv: start_vv.clone(),
            start_vv,
            committed: false,
        }
    }

    pub(crate) fn with_store<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&LogStore) -> R,
    {
        let store = self.store.upgrade().unwrap();
        let store = store.try_read().unwrap();
        f(&store)
    }

    pub(crate) fn with_store_mut<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut LogStore) -> R,
    {
        let store = self.store.upgrade().unwrap();
        let mut store = store.try_write().unwrap();
        f(&mut store)
    }

    pub(crate) fn with_hierarchy<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&Hierarchy) -> R,
    {
        let hierarchy = self.hierarchy.upgrade().unwrap();
        let hierarchy = hierarchy.try_lock().unwrap();
        f(&hierarchy)
    }

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

    pub(crate) fn append_event(&mut self, idx: ContainerIdx, event: RawEvent) {
        // cache events
        if let Some(old) = self.pending_events.get_mut(&idx) {
            compose_two_events(old, event);
        } else {
            self.pending_events.insert(idx, event);
        }
    }

    fn emit_events(&mut self) {
        let pending_events = std::mem::take(&mut self.pending_events);
        for (_, mut event) in pending_events.into_iter() {
            event.old_version = self.start_vv.clone();
            event.new_version = self.latest_vv.clone();
            let hierarchy = self.hierarchy.upgrade().unwrap();
            Hierarchy::notify_without_lock(hierarchy, event);
        }
    }

    pub(crate) fn register_container(
        &mut self,
        parent_id: &ContainerID,
        type_: ContainerType,
    ) -> (ContainerID, ContainerIdx) {
        self.with_store_hierarchy_mut(|txn, s, h| {
            let (container_id, idx) = s.create_container(type_);
            let parent_idx = s.reg.get_idx(parent_id).unwrap();
            txn.created_container
                .entry(parent_idx)
                .or_insert_with(FxHashSet::default)
                .insert(idx);
            h.add_child(parent_id, &container_id);
            (container_id, idx)
        })
    }

    pub fn decode(&mut self, input: &[u8]) -> Result<(), LoroError> {
        let store = self.store.upgrade().unwrap();
        let mut store = store.try_write().unwrap();
        let hierarchy = self.hierarchy.upgrade().unwrap();
        let mut hierarchy = hierarchy.try_lock().unwrap();
        let events = LoroEncoder::decode(&mut store, &mut hierarchy, input)?;
        for event in events {
            let idx = store.get_container_idx(&event.container_id).unwrap();
            self.append_event(idx, event)
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

fn compose_two_events(a: &mut RawEvent, mut b: RawEvent) {
    let this_diff = std::mem::take(&mut a.diff).pop().unwrap();
    let other_diff = std::mem::take(&mut b.diff).pop().unwrap();
    let diff = match other_diff {
        Diff::List(x) => Diff::List(this_diff.into_list().unwrap().compose(x)),
        Diff::Map(x) => Diff::Map(this_diff.into_map().unwrap().compose(x)),
        Diff::Text(x) => Diff::Text(this_diff.into_text().unwrap().compose(x)),
    };
    a.diff.push(diff);
}

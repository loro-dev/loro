use std::{
    collections::BTreeMap,
    ops::{Deref, DerefMut},
    sync::{Arc, Mutex, RwLock, Weak},
};

use fxhash::{FxHashMap, FxHashSet};
use rle::RleVec;

use crate::{
    container::{registry::ContainerIdx, Container, ContainerID},
    delta::{DeltaItem, SeqDelta},
    event::{Diff, RawEvent},
    hierarchy::Hierarchy,
    id::{ClientID, ID},
    transaction::op::Value,
    ContainerType, LogStore, LoroCore, LoroError, LoroValue,
};

use self::{
    checker::Checker,
    container::TransactionalContainer,
    op::{ListTxnOp, MapTxnOp, TransactionOp},
};

mod checker;
pub mod container;
pub(crate) mod op;

pub trait Transact {
    fn transact(&self) -> TransactionWrap;
}

impl Transact for LoroCore {
    fn transact(&self) -> TransactionWrap {
        TransactionWrap(Arc::new(Mutex::new(Transaction::new(
            Arc::downgrade(&self.log_store),
            Arc::downgrade(&self.hierarchy),
        ))))
    }
}

impl Transact for TransactionWrap {
    fn transact(&self) -> TransactionWrap {
        Self(Arc::clone(&self.0))
    }
}

impl AsMut<Transaction> for Transaction {
    fn as_mut(&mut self) -> &mut Transaction {
        self
    }
}

pub struct TransactionWrap(pub(crate) Arc<Mutex<Transaction>>);

pub struct Transaction {
    client_id: ClientID,
    pub(crate) store: Weak<RwLock<LogStore>>,
    pub(crate) hierarchy: Weak<Mutex<Hierarchy>>,
    // sort by [ContainerIdx]
    pub(crate) pending_ops: BTreeMap<ContainerIdx, Vec<TransactionOp>>,
    compressed_op: Vec<TransactionOp>,
    checker: Checker,
    created_container: FxHashMap<ContainerIdx, FxHashSet<ContainerIdx>>,
    deleted_container: FxHashSet<ContainerIdx>,
    committed: bool,
}

impl Transaction {
    pub(crate) fn new(store: Weak<RwLock<LogStore>>, hierarchy: Weak<Mutex<Hierarchy>>) -> Self {
        let client_id = {
            let store = store.upgrade().unwrap();
            let store = store.try_read().unwrap();
            store.this_client_id
        };
        Self {
            client_id,
            store,
            hierarchy,
            pending_ops: Default::default(),
            compressed_op: Default::default(),
            checker: Default::default(),
            created_container: Default::default(),
            deleted_container: Default::default(),
            committed: false,
        }
    }

    pub fn next_container_idx(&mut self) -> ContainerIdx {
        let store = self.store.upgrade().unwrap();
        let store = store.try_read().unwrap();
        store.next_container_idx()
    }

    pub(crate) fn client_id(&self) -> ClientID {
        self.client_id
    }

    pub(crate) fn push(
        &mut self,
        op: TransactionOp,
        created_container: Option<ContainerIdx>,
    ) -> Result<(), LoroError> {
        self.checker.check(&op)?;
        if let Some(idx) = created_container {
            self.created_container
                .entry(op.container_idx())
                .or_insert_with(FxHashSet::default)
                .insert(idx);
        }
        self.pending_ops
            .entry(op.container_idx())
            .or_insert_with(Vec::new)
            .push(op);
        Ok(())
    }

    pub(crate) fn get_container_idx_by_id(&self, id: &ContainerID) -> Option<ContainerIdx> {
        self.with_store(|store| store.reg.get_idx(id))
    }

    fn with_store<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&LogStore) -> R,
    {
        let store = self.store.upgrade().unwrap();
        let store = store.read().unwrap();
        f(&store)
    }

    fn with_store_hierarchy_mut<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&Self, &mut LogStore, &mut Hierarchy) -> R,
    {
        let store = self.store.upgrade().unwrap();
        let mut store = store.try_write().unwrap();
        let hierarchy = self.hierarchy.upgrade().unwrap();
        let mut hierarchy = hierarchy.try_lock().unwrap();
        f(self, &mut store, &mut hierarchy)
    }

    fn apply_ops_emit_event(&mut self) {
        let compressed_op = std::mem::take(&mut self.compressed_op);
        let events = self.with_store_hierarchy_mut(|_txn, store, hierarchy| {
            let mut events = Vec::with_capacity(compressed_op.len());
            for op in compressed_op {
                let idx = op.container_idx();
                let container = store.reg.get_by_idx(&idx).unwrap();
                let container = container.upgrade().unwrap();
                let mut container = container.try_lock().unwrap();
                let container_id = container.id().clone();
                let type_ = container_id.container_type();
                let store_ops = container.apply_txn_op(store, &op);
                drop(container);
                let (old_version, new_version) = store.append_local_ops(&store_ops);
                let new_version = new_version.into();
                let event = if hierarchy.should_notify(&container_id) {
                    match type_ {
                        ContainerType::List => {
                            let delta = op.into_list().unwrap().1.into_event_format();
                            hierarchy
                                .get_abs_path(&store.reg, &container_id)
                                .map(|abs_path| RawEvent {
                                    container_id: container_id.clone(),
                                    old_version,
                                    new_version,
                                    diff: vec![Diff::List(delta)],
                                    local: true,
                                    abs_path,
                                })
                        }
                        _ => unimplemented!(),
                    }
                } else {
                    None
                };
                events.push(event);
            }
            events
        });
        for event in events.into_iter().flatten() {
            Hierarchy::notify_without_lock(self.hierarchy.upgrade().unwrap(), event);
        }
    }

    fn register_container(
        &mut self,
        idx: ContainerIdx,
        type_: ContainerType,
        parent_idx: ContainerIdx,
    ) -> ContainerID {
        self.with_store_hierarchy_mut(|_txn, s, h| {
            let id = s.next_id();
            let container_id = ContainerID::new_normal(id, type_);

            let parent_id = s.reg.get_id(parent_idx).unwrap();
            h.add_child(parent_id, &container_id);

            s.reg.register_txn(idx, container_id.clone());
            container_id
        })
    }

    // We merge the Ops by [ContainerIdx] order, and
    // First iteration
    // - Multiple `InsertValue` are merged into a `InsertBatch`
    // - If a container which was created newly is deleted, we remove the [ContainerIdx] in `pending_ops` immediately.
    // Second iteration
    // - If a container really needs to be created, we create it at once and convert this op into InsertValue with `LoroValue::Unresolved`
    // - merge this op with
    fn compress_ops(&mut self) {
        let pending_ops = std::mem::take(&mut self.pending_ops);
        for (idx, ops) in pending_ops {
            if self.deleted_container.remove(&idx) {
                continue;
            }
            let type_ = ops.first().unwrap().container_type();
            match type_ {
                ContainerType::List => {
                    let new_op = ops.into_iter().fold(ListTxnOp::new(), |a, mut b| {
                        self.convert_op_container(&mut b);
                        let list_op = b.list_inner();
                        a.compose(list_op)
                    });
                    self.compressed_op.push(TransactionOp::List {
                        container: idx,
                        ops: new_op,
                    })
                }
                _ => unimplemented!(),
            };
            // The rest containers that are still in `created_container` have been deleted.
            if let Some(deleted_containers) = self.created_container.remove(&idx) {
                self.deleted_container.extend(deleted_containers);
            }
        }
    }

    fn convert_op_container(&mut self, op: &mut TransactionOp) {
        match op {
            TransactionOp::List { container, ops } => {
                for item in ops.iter_mut() {
                    if let Some((value, _)) = item.as_insert_mut() {
                        assert_eq!(value.len(), 1);
                        if let Some((type_, idx)) = value.first().unwrap().as_container() {
                            self.created_container
                                .get_mut(container)
                                .unwrap()
                                .remove(idx);
                            let id = self.register_container(*idx, *type_, *container);
                            *value = vec![Value::Value(LoroValue::Unresolved(id.into()))];
                        }
                    }
                }
            }
            _ => unimplemented!(),
        };
    }

    pub fn commit(&mut self) {
        if self.committed {
            return;
        }
        self.committed = true;
        // TODO: transaction commit
        // 1. compress op
        // 2. maybe rebase
        // 3. batch apply op
        // 4. aggregate event

        // apply op

        // convert InsertContainer op to Insert op with LoroValue::Unresolved

        // compress op
        self.compress_ops();

        // TODO: when merge the ops of the newly created container, the deleted container need to be recorded
        // TODO: merge the ops

        self.apply_ops_emit_event();
    }
}

impl Drop for Transaction {
    fn drop(&mut self) {
        self.commit()
    }
}

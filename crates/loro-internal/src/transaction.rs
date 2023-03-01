use std::{
    collections::BTreeMap,
    ops::{Deref, DerefMut},
    sync::{Arc, Mutex, RwLock, Weak},
};

use fxhash::FxHashSet;

use crate::{
    container::{registry::ContainerIdx, Container, ContainerID},
    delta::Delta,
    event::{Diff, RawEvent},
    hierarchy::Hierarchy,
    id::ClientID,
    ContainerType, LogStore, LoroCore, LoroError,
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

impl Deref for TransactionWrap {
    type Target = Arc<Mutex<Transaction>>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

pub struct TransactionWrap(pub(crate) Arc<Mutex<Transaction>>);

pub struct Transaction {
    client_id: ClientID,
    pub(crate) store: Weak<RwLock<LogStore>>,
    pub(crate) hierarchy: Weak<Mutex<Hierarchy>>,
    // sort by [ContainerIdx]
    pub(crate) pending_ops: BTreeMap<ContainerIdx, Vec<TransactionOp>>,
    checker: Checker,
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
            deleted_container: Default::default(),
            checker: Default::default(),
            committed: false,
        }
    }

    pub(crate) fn client_id(&self) -> ClientID {
        self.client_id
    }

    pub(crate) fn insert(
        &mut self,
        op: TransactionOp,
    ) -> Result<Option<TransactionalContainer>, LoroError> {
        self.checker.check(&op)?;
        let ans = match op {
            TransactionOp::List { container, .. } => self.insert_list(container, op),
            TransactionOp::Map { container, .. } => self.insert_map(container, op),
            TransactionOp::Text { container, .. } => self.insert_text(container, op),
        };
        Ok(ans)
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

    fn insert_text(
        &mut self,
        container_idx: ContainerIdx,
        op: TransactionOp,
    ) -> Option<TransactionalContainer> {
        self.pending_ops
            .entry(container_idx)
            .or_insert_with(Vec::new)
            .push(op);
        None
    }

    fn insert_map(
        &mut self,
        container_idx: ContainerIdx,
        mut op: TransactionOp,
    ) -> Option<TransactionalContainer> {
        let ans = match op.as_map_mut().unwrap().1 {
            MapTxnOp::Insert { key, value } => None,
            MapTxnOp::InsertContainer {
                key,
                type_,
                container,
            } => {
                let next_container_idx = self
                    .store
                    .upgrade()
                    .unwrap()
                    .try_read()
                    .unwrap()
                    .next_container_idx();
                *container = Some(next_container_idx);
                Some(TransactionalContainer::new(*type_, next_container_idx))
            }
            MapTxnOp::Delete {
                key,
                deleted_container,
            } => {
                self.deleted_container
                    .extend((*deleted_container).into_iter());
                None
            }
        };
        self.pending_ops
            .entry(container_idx)
            .or_insert_with(Vec::new)
            .push(op);

        ans
    }

    fn insert_list(
        &mut self,
        container_idx: ContainerIdx,
        mut op: TransactionOp,
    ) -> Option<TransactionalContainer> {
        let ans = match op.as_list_mut().unwrap().1 {
            ListTxnOp::InsertContainer {
                container, type_, ..
            } => {
                // record the created container
                let next_container_idx = self
                    .store
                    .upgrade()
                    .unwrap()
                    .try_read()
                    .unwrap()
                    .next_container_idx();
                *container = Some(next_container_idx);
                Some(TransactionalContainer::new(*type_, next_container_idx))
            }
            ListTxnOp::Delete {
                deleted_container: Some(deleted_container),
                ..
            } => {
                // record the deleted container
                self.deleted_container
                    .extend(deleted_container.clone().into_iter());
                None
            }
            _ => None,
        };

        self.pending_ops
            .entry(container_idx)
            .or_insert_with(Vec::new)
            .push(op);

        ans
    }

    fn apply_ops_emit_event(&mut self) {
        let pending_ops = std::mem::take(&mut self.pending_ops);
        self.with_store_hierarchy_mut(|txn, store, hierarchy| {
            for (idx, ops) in pending_ops.into_iter() {
                let container = store.reg.get_by_idx(&idx).unwrap();
                let container = container.upgrade().unwrap();
                let mut container = container.try_lock().unwrap();
                let container_id = container.id().clone();
                let mut store_ops = Vec::with_capacity(ops.len());
                for op in ops.iter() {
                    // let id = store.next_id();
                    store_ops.push(container.apply_txn_op(store, op));
                }
                drop(container);
                let (old_version, new_version) = store.append_local_ops(&store_ops);
                let new_version = new_version.into();
                let event = if hierarchy.should_notify(&container_id) {
                    let mut delta = Delta::new();
                    for op in ops {
                        // TODO delta
                    }
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
                } else {
                    None
                };
                // Emit event
            }
        })
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
        // TODO:
        self.pending_ops.values_mut().for_each(|ops| {
            let owned_ops = std::mem::take(ops);
            *ops = owned_ops
                .into_iter()
                .map(|op| {
                    if let TransactionOp::List {
                        container,
                        op: ListTxnOp::InsertValue { pos, value },
                    } = op
                    {
                        TransactionOp::insert_list_batch_value(container, pos, vec![value])
                    } else {
                        op
                    }
                })
                .collect();
        });
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

        // remove the ops of the deleted container
        std::mem::take(&mut self.deleted_container)
            .iter()
            .for_each(|idx| {
                // the ops of deleted_container must be created in exist container
                self.pending_ops.remove(idx);
            });

        // convert InsertContainer op to Insert op with LoroValue::Unresolved
        let mut pending_ops = std::mem::take(&mut self.pending_ops);
        pending_ops.values_mut().for_each(|ops| {
            ops.iter_mut()
                .filter(|op| op.is_insert_container())
                .for_each(|op| {
                    op.register_container_and_convert(self).unwrap();
                });
        });
        self.pending_ops = pending_ops;

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

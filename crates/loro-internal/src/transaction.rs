use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, RwLock, Weak},
};

use fxhash::{FxHashMap, FxHashSet};

use crate::{
    container::{registry::ContainerIdx, ContainerID, ContainerTrait},
    event::{Diff, RawEvent},
    hierarchy::Hierarchy,
    id::ClientID,
    transaction::op::Value,
    ContainerType, LogStore, LoroCore, LoroError, LoroValue, Map,
};

use self::op::{ListTxnOps, MapTxnOps, TextTxnOps, TransactionOp};

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
    pending_ops: BTreeMap<ContainerIdx, Vec<TransactionOp>>,
    compressed_op: Vec<TransactionOp>,
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
            created_container: Default::default(),
            deleted_container: Default::default(),
            committed: false,
        }
    }

    pub(crate) fn next_container_idx(&mut self) -> ContainerIdx {
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
                let type_ = op.container_type();
                // TODO: diff remove vec!
                let diff = vec![match type_ {
                    ContainerType::List => {
                        Diff::List(op.as_list().unwrap().1.clone().into_event_format())
                    }
                    ContainerType::Map => {
                        let container = store.reg.get_by_idx(&idx).unwrap();
                        let map = Map::from_instance(container, store.this_client_id);
                        Diff::Map(op.as_map().unwrap().1.clone().into_event_format(&map))
                    }
                    ContainerType::Text => Diff::Text(op.as_text().unwrap().1.clone()),
                }];
                let container = store.reg.get_by_idx(&idx).unwrap();
                let container = container.upgrade().unwrap();
                let mut container = container.try_lock().unwrap();
                let container_id = container.id().clone();
                let store_ops = container.apply_txn_op(store, op);
                drop(container);
                let (old_version, new_version) = store.append_local_ops(&store_ops);
                let new_version = new_version.into();
                let event = if hierarchy.should_notify(&container_id) {
                    match type_ {
                        ContainerType::List => hierarchy
                            .get_abs_path(&store.reg, &container_id)
                            .map(|abs_path| RawEvent {
                                container_id,
                                old_version,
                                new_version,
                                diff,
                                local: true,
                                abs_path,
                            }),
                        ContainerType::Text => hierarchy
                            .get_abs_path(&store.reg, &container_id)
                            .map(|abs_path| RawEvent {
                                container_id,
                                old_version,
                                new_version,
                                diff,
                                local: true,
                                abs_path,
                            }),
                        ContainerType::Map => hierarchy
                            .get_abs_path(&store.reg, &container_id)
                            .map(|abs_path| RawEvent {
                                container_id,
                                old_version,
                                new_version,
                                diff,
                                local: true,
                                abs_path,
                            }),
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
            let mut container_id = ContainerID::new_normal(id, type_);

            while s.reg.contains(&container_id) {
                if let ContainerID::Normal { id, .. } = &mut container_id {
                    id.counter += 1;
                }
            }

            let parent_id = s.reg.get_id(parent_idx).unwrap();
            h.add_child(parent_id, &container_id);

            s.reg.register_txn(idx, container_id.clone());
            container_id
        })
    }

    fn compress_ops(&mut self) {
        let pending_ops = std::mem::take(&mut self.pending_ops);
        for (idx, ops) in pending_ops {
            if self.deleted_container.remove(&idx) {
                continue;
            }
            let type_ = ops.first().unwrap().container_type();
            match type_ {
                ContainerType::List => {
                    let new_op = ops.into_iter().fold(ListTxnOps::new(), |a, mut b| {
                        self.convert_op_container(&mut b);
                        let b = b.list_inner();
                        a.compose(b)
                    });
                    self.compressed_op.push(TransactionOp::List {
                        container: idx,
                        ops: new_op,
                    })
                }
                ContainerType::Text => {
                    let new_op = ops.into_iter().fold(TextTxnOps::new(), |a, b| {
                        let b = b.text_inner();
                        a.compose(b)
                    });
                    self.compressed_op.push(TransactionOp::Text {
                        container: idx,
                        ops: new_op,
                    })
                }
                ContainerType::Map => {
                    let new_op = ops.into_iter().fold(MapTxnOps::new(), |a, mut b| {
                        self.convert_op_container(&mut b);
                        let b = b.map_inner();
                        a.compose(b)
                    });
                    self.compressed_op.push(TransactionOp::Map {
                        container: idx,
                        ops: new_op,
                    })
                }
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
                // TODO: cache the containers?
                for item in ops.iter_mut() {
                    if let Some((value, _)) = item.as_insert_mut() {
                        for v in value {
                            if let Some((type_, idx)) = v.as_container() {
                                self.created_container
                                    .get_mut(container)
                                    .unwrap()
                                    .remove(idx);
                                let id = self.register_container(*idx, *type_, *container);
                                *v = Value::Value(LoroValue::Unresolved(id.into()));
                            }
                        }
                    }
                }
            }
            TransactionOp::Map { container, ops } => {
                for (_k, v) in ops.added.iter_mut() {
                    if let Some((type_, idx)) = v.as_container() {
                        self.created_container
                            .get_mut(container)
                            .unwrap()
                            .remove(idx);
                        let id = self.register_container(*idx, *type_, *container);
                        *v = Value::Value(LoroValue::Unresolved(id.into()));
                    }
                }
            }
            _ => unreachable!(),
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
        self.compress_ops();
        self.apply_ops_emit_event();
    }
}

impl Drop for Transaction {
    fn drop(&mut self) {
        self.commit()
    }
}

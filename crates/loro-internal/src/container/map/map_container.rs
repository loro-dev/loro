use std::sync::{Mutex, Weak};

use super::{super::pool::Pool, InnerMapSet};
use crate::{
    container::{
        pool_mapping::{MapPoolMapping, StateContent},
        registry::{ContainerIdx, ContainerInner, ContainerRegistry, ContainerTemp},
    },
    delta::MapDiff,
    op::OwnedRichOp,
    transaction::{
        op::{MapTxnOps, TransactionOp},
    },
    LogStore, LoroError, Transact,
};
use fxhash::FxHashMap;
use smallvec::{smallvec, SmallVec};

use crate::{
    container::{
        registry::{ContainerInstance, ContainerWrapper},
        Container, ContainerID, ContainerType,
    },
    event::{Diff, Index},
    hierarchy::Hierarchy,
    id::ClientID,
    log_store::ImportContext,
    op::{InnerContent, Op, RemoteContent, RichOp},
    prelim::Prelim,
    span::HasLamport,
    value::LoroValue,
    version::TotalOrderStamp,
    InternalString,
};

use super::MapSet;

/// We can only insert to Map
/// delete = set null
///
#[derive(Debug)]
pub struct MapContainer {
    id: ContainerID,
    idx: ContainerIdx,
    pub(crate) state: FxHashMap<InternalString, ValueSlot>,
    pub(crate) pool: Pool,
    pending_ops: Vec<OwnedRichOp>,
    pool_mapping: Option<MapPoolMapping>,
}

#[derive(Debug, Clone, Copy)]
pub struct ValueSlot {
    pub(crate) value: u32,
    pub(crate) order: TotalOrderStamp,
}

// FIXME: make map container support checkout to certain version
impl MapContainer {
    #[inline]
    pub(crate) fn new(id: ContainerID, idx: ContainerIdx) -> Self {
        MapContainer {
            id,
            idx,
            state: FxHashMap::default(),
            pool: Pool::default(),
            pending_ops: Vec::new(),
            pool_mapping: None,
        }
    }

    fn apply_insert(&mut self, store: &mut LogStore, key: InternalString, value: LoroValue) -> Op {
        let id = store.next_id();
        let value_index = self.pool.alloc(value).start;
        let order = TotalOrderStamp {
            client_id: store.this_client_id,
            lamport: store.next_lamport(),
        };
        self.state.insert(
            key.clone(),
            ValueSlot {
                value: value_index,
                order,
            },
        );
        Op {
            counter: id.counter,
            container: self.idx,
            content: InnerContent::Map(InnerMapSet {
                key,
                value: value_index,
            }),
        }
    }

    fn apply_txn_op_impl(&mut self, store: &mut LogStore, ops: &MapTxnOps) -> Vec<Op> {
        let ops = ops.clone();
        let mut store_ops = Vec::with_capacity(ops.added.len() + ops.deleted.len());
        for (k, v) in ops.added.into_iter() {
            store_ops.push(self.apply_insert(store, k, v.into_value().unwrap()));
        }
        for k in ops.deleted {
            store_ops.push(self.apply_insert(store, k, LoroValue::Null));
        }
        store_ops
    }

    fn update_hierarchy_if_container_is_overwritten(
        &mut self,
        key: &InternalString,
        h: &mut Hierarchy,
    ) {
        if let Some(old_value) = self.state.get(key) {
            let v = &self.pool[old_value.value];
            if let Some(container) = v.as_unresolved() {
                h.remove_child(&self.id, container);
            }
        }
    }

    pub fn index_of_child(&self, child: &ContainerID) -> Option<Index> {
        for (key, value) in self.state.iter() {
            if self.pool[value.value]
                .as_unresolved()
                .map(|x| &**x == child)
                .unwrap_or(false)
            {
                return Some(Index::Key(key.clone()));
            }
        }

        None
    }

    #[inline]
    pub fn get(&self, key: &InternalString) -> Option<&LoroValue> {
        self.state
            .get(key)
            .map(|v| self.pool.slice(&(v.value..v.value + 1)).first().unwrap())
    }

    pub fn to_json(&self, reg: &ContainerRegistry) -> LoroValue {
        self.get_value().resolve_deep(reg)
    }

    pub fn keys(&self) -> Vec<InternalString> {
        self.state.keys().cloned().collect()
    }

    pub fn values(&self) -> Vec<LoroValue> {
        self.state
            .values()
            .map(|value| {
                let index = value.value;
                let value = self.pool.slice(&(index..index + 1))[0].clone();
                value
            })
            .collect()
    }
}

impl Container for MapContainer {
    #[inline(always)]
    fn id(&self) -> &ContainerID {
        &self.id
    }

    #[inline(always)]
    fn idx(&self) -> ContainerIdx {
        self.idx
    }

    #[inline(always)]
    fn type_(&self) -> ContainerType {
        ContainerType::Map
    }

    fn get_value(&self) -> LoroValue {
        let mut map = FxHashMap::default();
        for (key, value) in self.state.iter() {
            let index = value.value;
            let value = self.pool.slice(&(index..index + 1))[0].clone();
            if let Some(container_id) = value.as_unresolved() {
                map.insert(
                    key.to_string(),
                    // TODO: make a from
                    LoroValue::Unresolved(container_id.clone()),
                );
            } else {
                map.insert(key.to_string(), value);
            }
        }

        map.into()
    }

    fn tracker_init(&mut self, _vv: &crate::version::PatchedVersionVector) {}

    fn tracker_checkout(&mut self, _vv: &crate::version::PatchedVersionVector) {}

    fn to_export(&mut self, content: InnerContent, _gc: bool) -> SmallVec<[RemoteContent; 1]> {
        if let Ok(set) = content.into_map() {
            let index = set.value;
            let value = self.pool.slice(&(index..index + 1))[0].clone();
            return smallvec![RemoteContent::Map(MapSet {
                key: set.key,
                value,
            })];
        }

        unreachable!()
    }

    fn to_import(&mut self, mut content: RemoteContent) -> InnerContent {
        if let Some(set) = content.as_map_mut() {
            let index = self.pool.alloc(std::mem::take(&mut set.value));
            return InnerContent::Map(InnerMapSet {
                key: set.key.clone(),
                value: index.start,
            });
        }
        unreachable!()
    }

    // TODO: refactor
    fn update_state_directly(
        &mut self,
        hierarchy: &mut Hierarchy,
        op: &RichOp,
        ctx: &mut ImportContext,
    ) {
        let content = op.get_sliced().content;
        let new_val: &InnerMapSet = content.as_map().unwrap();
        let order = TotalOrderStamp {
            lamport: op.lamport(),
            client_id: op.client_id(),
        };
        let should_notify = hierarchy.should_notify(&self.id);
        if let Some(slot) = self.state.get_mut(&new_val.key) {
            if slot.order < order {
                let new_value = &self.pool[new_val.value];
                if should_notify {
                    let mut map_diff = MapDiff::default();
                    map_diff.updated.insert(
                        new_val.key.clone(),
                        (self.pool[slot.value].clone(), new_value.clone()).into(),
                    );
                    ctx.push_diff(&self.id, Diff::Map(map_diff));
                }

                let old_val = &self.pool[slot.value];
                if let Some(container) = old_val.as_unresolved() {
                    hierarchy.remove_child(&self.id, container);
                }
                if let Some(container) = new_value.as_unresolved() {
                    hierarchy.add_child(&self.id, container);
                }

                slot.value = new_val.value;
                slot.order = order;
            }
        } else {
            let new_value = &self.pool[new_val.value];
            if should_notify {
                let mut map_diff = MapDiff::default();
                map_diff
                    .added
                    .insert(new_val.key.clone(), self.pool[new_val.value].clone());
                ctx.push_diff(&self.id, Diff::Map(map_diff));
            }

            if let Some(container) = new_value.as_unresolved() {
                hierarchy.add_child(&self.id, container);
            }

            self.state.insert(
                new_val.key.to_owned(),
                ValueSlot {
                    value: new_val.value,
                    order,
                },
            );
        }
    }

    fn track_apply(&mut self, _: &mut Hierarchy, op: &RichOp, _: &mut ImportContext) {
        self.pending_ops.push(op.as_owned());
    }

    fn apply_tracked_effects_from(
        &mut self,
        hierarchy: &mut Hierarchy,
        import_context: &mut ImportContext,
    ) {
        for op in std::mem::take(&mut self.pending_ops) {
            self.update_state_directly(hierarchy, &op.rich_op(), import_context)
        }
    }

    fn initialize_pool_mapping(&mut self) {
        let mut pool_mapping = MapPoolMapping::default();
        for value in self.state.values() {
            let index = value.value;
            pool_mapping.push_state_slice(index, &self.pool.slice(&(index..index + 1))[0]);
        }
        self.pool_mapping = Some(pool_mapping);
    }

    fn encode_and_release_pool_mapping(&mut self) -> StateContent {
        let pool_mapping = self.pool_mapping.take().unwrap();
        let (keys, values) = self
            .state
            .iter()
            .map(|(k, v)| {
                (
                    k.clone(),
                    ValueSlot {
                        value: pool_mapping.get_new_index(v.value),
                        order: v.order,
                    },
                )
            })
            .unzip();
        StateContent::Map {
            pool: pool_mapping.inner(),
            keys,
            values,
        }
    }

    fn to_export_snapshot(
        &mut self,
        content: &InnerContent,
        _gc: bool,
    ) -> SmallVec<[InnerContent; 1]> {
        match content {
            InnerContent::Map(set) => {
                let index = set.value;
                let value = self
                    .pool_mapping
                    .as_mut()
                    .unwrap()
                    .convert_ops_value(index, &self.pool[index]);
                smallvec![InnerContent::Map(InnerMapSet {
                    key: set.key.clone(),
                    value,
                })]
            }
            _ => unreachable!(),
        }
    }

    fn to_import_snapshot(
        &mut self,
        state_content: StateContent,
        hierarchy: &mut Hierarchy,
        ctx: &mut ImportContext,
    ) {
        if let StateContent::Map { pool, keys, values } = state_content {
            for v in pool.iter() {
                if let LoroValue::Unresolved(child_container_id) = v {
                    hierarchy.add_child(self.id(), child_container_id.as_ref());
                }
            }
            self.pool = pool.into();
            self.state = keys.into_iter().zip(values).collect();
            // notify
            let should_notify = hierarchy.should_notify(&self.id);
            if should_notify {
                let mut map_diff = MapDiff::default();
                for (k, v) in self.state.iter() {
                    map_diff.added.insert(k.clone(), self.pool[v.value].clone());
                }
                ctx.push_diff(&self.id, Diff::Map(map_diff));
            }
        } else {
            unreachable!()
        }
    }

    fn apply_txn_op(&mut self, store: &mut LogStore, op: &TransactionOp) -> Vec<Op> {
        let op = op.as_map().unwrap().1;
        self.apply_txn_op_impl(store, op)
    }
}

#[derive(Debug, Clone)]
pub struct Map {
    container: ContainerInner,
    client_id: ClientID,
    container_idx: ContainerIdx,
}

impl Map {
    pub fn from_instance(instance: Weak<Mutex<ContainerInstance>>, client_id: ClientID) -> Self {
        let container_idx = instance.upgrade().unwrap().try_lock().unwrap().idx();
        Self {
            container: ContainerInner::from(instance),
            client_id,
            container_idx,
        }
    }

    pub fn from_idx(idx: ContainerIdx, client_id: ClientID) -> Self {
        Self {
            container: ContainerInner::from(idx),
            client_id,
            container_idx: idx,
        }
    }

    #[inline(always)]
    pub fn idx(&self) -> ContainerIdx {
        self.container_idx
    }

    pub fn insert<T: Transact, V: Prelim>(
        &mut self,
        txn: &T,
        key: &str,
        value: V,
    ) -> Result<Option<ContainerTemp>, LoroError> {
        self.with_transaction_checked(txn, |txn, _| {
            let (value, maybe_container) = value.convert_value()?;
            if let Some(prelim) = maybe_container {
                let idx = txn.next_container_idx();
                let type_ = value.into_container().unwrap();
                txn.push(
                    TransactionOp::insert_map_container(self.idx(), key.into(), type_, idx),
                    Some(idx),
                )?;
                prelim.integrate(txn, idx)?;
                Ok(Some(ContainerTemp::new(idx, type_, self.client_id)))
            } else {
                let value = value.into_value().unwrap();
                txn.push(
                    TransactionOp::insert_map_value(self.idx(), key.into(), value),
                    None,
                )?;
                Ok(None)
            }
        })?
    }

    pub fn delete<T: Transact>(&mut self, txn: &T, key: &str) -> Result<(), LoroError> {
        self.with_transaction_checked(txn, |txn, _| {
            txn.push(
                TransactionOp::delete_map(self.idx(), &key.to_string().into()),
                None,
            )
        })?
    }

    pub fn get(&self, key: &str) -> Option<LoroValue> {
        self.with_container(|map| map.get(&key.into()).cloned())
            .unwrap()
    }

    pub fn try_get(&self, key: &str) -> Result<Option<LoroValue>, LoroError> {
        self.with_container(|map| map.get(&key.into()).cloned())
    }

    pub fn keys(&self) -> Vec<String> {
        todo!()
    }

    pub fn values(&self) -> Result<Vec<LoroValue>, LoroError> {
        self.with_container(|map| map.values())
    }

    // pub fn for_each<F>(&self, f: F)
    // where
    //     F: Fn(&InternalString, &LoroValue),
    // {
    //     self.with_container(|map| {
    //         for (k, v) in map.state.iter() {
    //             let value = &map.pool.slice(&(v.value..v.value + 1))[0];
    //             f(k, value);
    //         }
    //     })
    // }

    pub fn id(&self) -> Result<ContainerID, LoroError> {
        self.with_container(|x| x.id.clone())
    }

    pub fn try_get_value(&self) -> Result<LoroValue, LoroError> {
        self.with_container(|x| x.get_value())
    }

    pub fn get_value(&self) -> LoroValue {
        self.try_get_value().unwrap()
    }

    pub fn len(&self) -> usize {
        todo!()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl ContainerWrapper for Map {
    type Container = MapContainer;

    fn client_id(&self) -> ClientID {
        self.client_id
    }

    fn container_inner(&self) -> &ContainerInner {
        &self.container
    }

    fn with_container<F, R>(&self, f: F) -> Result<R, LoroError>
    where
        F: FnOnce(&mut Self::Container) -> R,
    {
        let ContainerInner::Instance(ref instance) = self.container else {return Err(LoroError::TempContainerError)};

        let w = instance.upgrade().unwrap();
        let mut container_instance = w.try_lock().unwrap();
        let map = container_instance.as_map_mut().unwrap();
        Ok(f(map))
    }
}

use std::{
    ops::{Deref, DerefMut},
    sync::{
        atomic::{AtomicU32, Ordering},
        Arc, Mutex, RwLockWriteGuard, Weak,
    },
};

use enum_as_inner::EnumAsInner;

use fxhash::{FxHashMap, FxHashSet};
use owning_ref::OwningRefMut;
use smallvec::SmallVec;
use tracing::instrument;

use crate::{
    context::Context,
    event::{Index, ObserverHandler, SubscriptionID},
    hierarchy::Hierarchy,
    id::ClientID,
    log_store::ImportContext,
    op::{RemoteContent, RichOp},
    transaction::Transaction,
    version::PatchedVersionVector,
    LoroError, LoroValue, Transact,
};

use super::{
    list::ListContainer, map::MapContainer, pool_mapping::StateContent, text::TextContainer,
    ContainerID, ContainerTrait, ContainerType,
};

#[derive(PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Hash, Debug)]
pub struct ContainerIdx(u32);

impl ContainerIdx {
    pub(crate) fn to_u32(self) -> u32 {
        self.0
    }

    pub(crate) fn from_u32(idx: u32) -> Self {
        Self(idx)
    }
}

// TODO: replace this with a fat pointer?
#[derive(Debug, EnumAsInner)]
pub enum ContainerInstance {
    List(Box<ListContainer>),
    Text(Box<TextContainer>),
    Map(Box<MapContainer>),
    Dyn(Box<dyn ContainerTrait>),
}

impl ContainerTrait for ContainerInstance {
    fn id(&self) -> &ContainerID {
        match self {
            ContainerInstance::Map(x) => x.id(),
            ContainerInstance::Text(x) => x.id(),
            ContainerInstance::List(x) => x.id(),
            ContainerInstance::Dyn(x) => x.id(),
        }
    }

    fn idx(&self) -> ContainerIdx {
        match self {
            ContainerInstance::Map(x) => x.idx(),
            ContainerInstance::Text(x) => x.idx(),
            ContainerInstance::List(x) => x.idx(),
            ContainerInstance::Dyn(x) => x.idx(),
        }
    }

    fn type_(&self) -> ContainerType {
        match self {
            ContainerInstance::Map(_) => ContainerType::Map,
            ContainerInstance::Text(_) => ContainerType::Text,
            ContainerInstance::List(_) => ContainerType::List,
            ContainerInstance::Dyn(x) => x.type_(),
        }
    }

    #[instrument(skip_all)]
    fn tracker_init(&mut self, vv: &PatchedVersionVector) {
        match self {
            ContainerInstance::Map(x) => x.tracker_init(vv),
            ContainerInstance::Text(x) => x.tracker_init(vv),
            ContainerInstance::List(x) => x.tracker_init(vv),
            ContainerInstance::Dyn(x) => x.tracker_init(vv),
        }
    }

    #[instrument(skip_all)]
    fn tracker_checkout(&mut self, vv: &PatchedVersionVector) {
        match self {
            ContainerInstance::Map(x) => x.tracker_checkout(vv),
            ContainerInstance::Text(x) => x.tracker_checkout(vv),
            ContainerInstance::List(x) => x.tracker_checkout(vv),
            ContainerInstance::Dyn(x) => x.tracker_checkout(vv),
        }
    }

    #[instrument(skip_all)]
    fn get_value(&self) -> crate::LoroValue {
        match self {
            ContainerInstance::Map(x) => x.get_value(),
            ContainerInstance::Text(x) => x.get_value(),
            ContainerInstance::List(x) => x.get_value(),
            ContainerInstance::Dyn(x) => x.get_value(),
        }
    }

    #[instrument(skip_all)]
    fn update_state_directly(
        &mut self,
        hierarchy: &mut Hierarchy,
        op: &RichOp,
        context: &mut ImportContext,
    ) {
        match self {
            ContainerInstance::Map(x) => x.update_state_directly(hierarchy, op, context),
            ContainerInstance::Text(x) => x.update_state_directly(hierarchy, op, context),
            ContainerInstance::List(x) => x.update_state_directly(hierarchy, op, context),
            ContainerInstance::Dyn(x) => x.update_state_directly(hierarchy, op, context),
        }
    }

    #[instrument(skip_all)]
    fn track_apply(&mut self, hierarchy: &mut Hierarchy, op: &RichOp, ctx: &mut ImportContext) {
        match self {
            ContainerInstance::Map(x) => x.track_apply(hierarchy, op, ctx),
            ContainerInstance::Text(x) => x.track_apply(hierarchy, op, ctx),
            ContainerInstance::List(x) => x.track_apply(hierarchy, op, ctx),
            ContainerInstance::Dyn(x) => x.track_apply(hierarchy, op, ctx),
        }
    }

    #[instrument(skip_all)]
    fn apply_tracked_effects_from(
        &mut self,
        h: &mut Hierarchy,
        import_context: &mut ImportContext,
    ) {
        match self {
            ContainerInstance::Map(x) => x.apply_tracked_effects_from(h, import_context),
            ContainerInstance::Text(x) => x.apply_tracked_effects_from(h, import_context),
            ContainerInstance::List(x) => x.apply_tracked_effects_from(h, import_context),
            ContainerInstance::Dyn(x) => x.apply_tracked_effects_from(h, import_context),
        }
    }

    #[instrument(skip_all)]
    fn to_export(
        &mut self,
        content: crate::op::InnerContent,
        gc: bool,
    ) -> SmallVec<[RemoteContent; 1]> {
        match self {
            ContainerInstance::Map(x) => x.to_export(content, gc),
            ContainerInstance::Text(x) => x.to_export(content, gc),
            ContainerInstance::List(x) => x.to_export(content, gc),
            ContainerInstance::Dyn(x) => x.to_export(content, gc),
        }
    }

    #[instrument(skip_all)]
    fn to_import(&mut self, content: crate::op::RemoteContent) -> crate::op::InnerContent {
        match self {
            ContainerInstance::Map(x) => x.to_import(content),
            ContainerInstance::Text(x) => x.to_import(content),
            ContainerInstance::List(x) => x.to_import(content),
            ContainerInstance::Dyn(x) => x.to_import(content),
        }
    }

    fn to_export_snapshot(
        &mut self,
        content: &crate::op::InnerContent,
        gc: bool,
    ) -> SmallVec<[crate::op::InnerContent; 1]> {
        match self {
            ContainerInstance::Map(x) => x.to_export_snapshot(content, gc),
            ContainerInstance::Text(x) => x.to_export_snapshot(content, gc),
            ContainerInstance::List(x) => x.to_export_snapshot(content, gc),
            ContainerInstance::Dyn(x) => x.to_export_snapshot(content, gc),
        }
    }

    fn initialize_pool_mapping(&mut self) {
        match self {
            ContainerInstance::Map(x) => x.initialize_pool_mapping(),
            ContainerInstance::Text(x) => x.initialize_pool_mapping(),
            ContainerInstance::List(x) => x.initialize_pool_mapping(),
            ContainerInstance::Dyn(x) => x.initialize_pool_mapping(),
        }
    }

    fn encode_and_release_pool_mapping(&mut self) -> StateContent {
        match self {
            ContainerInstance::Map(x) => x.encode_and_release_pool_mapping(),
            ContainerInstance::Text(x) => x.encode_and_release_pool_mapping(),
            ContainerInstance::List(x) => x.encode_and_release_pool_mapping(),
            ContainerInstance::Dyn(x) => x.encode_and_release_pool_mapping(),
        }
    }

    fn to_import_snapshot(
        &mut self,
        state_content: StateContent,
        hierarchy: &mut Hierarchy,
        ctx: &mut ImportContext,
    ) {
        match self {
            ContainerInstance::Map(x) => x.to_import_snapshot(state_content, hierarchy, ctx),
            ContainerInstance::Text(x) => x.to_import_snapshot(state_content, hierarchy, ctx),
            ContainerInstance::List(x) => x.to_import_snapshot(state_content, hierarchy, ctx),
            ContainerInstance::Dyn(x) => x.to_import_snapshot(state_content, hierarchy, ctx),
        }
    }
}

impl ContainerInstance {
    pub fn index_of_child(&self, child: &ContainerID) -> Option<Index> {
        match self {
            ContainerInstance::Map(x) => x.index_of_child(child),
            ContainerInstance::List(x) => x.index_of_child(child),
            _ => unreachable!(),
        }
    }
}

// TODO: containers snapshot: we need to resolve each container's parent even
// if its creation op is not in the logStore
#[derive(Debug)]
pub struct ContainerRegistry {
    container_to_idx: FxHashMap<ContainerID, ContainerIdx>,
    containers: FxHashMap<ContainerIdx, ContainerAndId>,
    next_container_idx: Arc<AtomicU32>,
}

#[derive(Debug)]
struct ContainerAndId {
    pub container: Arc<Mutex<ContainerInstance>>,
    pub id: ContainerID,
}

impl ContainerRegistry {
    pub fn new() -> Self {
        ContainerRegistry {
            container_to_idx: FxHashMap::default(),
            containers: FxHashMap::default(),
            next_container_idx: Arc::new(AtomicU32::new(0)),
        }
    }

    #[inline]
    fn create(&mut self, id: ContainerID, idx: ContainerIdx) -> ContainerInstance {
        match id.container_type() {
            ContainerType::Map => ContainerInstance::Map(Box::new(MapContainer::new(id, idx))),
            ContainerType::Text => ContainerInstance::Text(Box::new(TextContainer::new(id, idx))),
            ContainerType::List => ContainerInstance::List(Box::new(ListContainer::new(id, idx))),
        }
    }

    #[inline(always)]
    pub fn get(&self, id: &ContainerID) -> Option<Weak<Mutex<ContainerInstance>>> {
        self.container_to_idx
            .get(id)
            .and_then(|x| self.containers.get(x))
            .map(|x| Arc::downgrade(&x.container))
    }

    #[inline(always)]
    pub fn contains(&self, id: &ContainerID) -> bool {
        self.container_to_idx.contains_key(id)
    }

    #[inline(always)]
    pub fn contains_idx(&self, idx: &ContainerIdx) -> bool {
        self.containers.contains_key(idx)
    }

    #[inline(always)]
    pub fn all_container_idx(&self) -> FxHashSet<ContainerIdx> {
        self.containers.keys().copied().collect()
    }

    #[inline(always)]
    pub(crate) fn get_by_idx(&self, idx: &ContainerIdx) -> Option<Weak<Mutex<ContainerInstance>>> {
        self.containers
            .get(idx)
            .map(|x| Arc::downgrade(&x.container))
    }

    #[inline(always)]
    pub(crate) fn get_idx(&self, id: &ContainerID) -> Option<ContainerIdx> {
        self.container_to_idx.get(id).copied()
    }

    pub(crate) fn get_id(&self, idx: ContainerIdx) -> Option<&ContainerID> {
        self.containers.get(&idx).map(|x| &x.id)
    }

    #[inline(always)]
    pub(crate) fn insert(
        &mut self,
        id: ContainerID,
        idx: ContainerIdx,
        container: ContainerInstance,
    ) -> ContainerIdx {
        self.container_to_idx.insert(id.clone(), idx);
        self.containers.insert(
            idx,
            ContainerAndId {
                container: Arc::new(Mutex::new(container)),
                id,
            },
        );

        idx
    }

    #[inline(always)]
    pub(crate) fn next_idx_and_add_1(&self) -> ContainerIdx {
        let idx = self.next_container_idx.fetch_add(1, Ordering::SeqCst);
        ContainerIdx::from_u32(idx)
    }

    pub(crate) fn register(&mut self, id: &ContainerID) -> ContainerIdx {
        let idx = self.next_idx_and_add_1();
        let container = self.create(id.clone(), idx);
        self.insert(id.clone(), idx, container);
        idx
    }

    pub(crate) fn register_txn(&mut self, idx: ContainerIdx, id: ContainerID) {
        let container = self.create(id.clone(), idx);
        self.insert(id, idx, container);
    }

    pub(crate) fn get_or_create(&mut self, id: &ContainerID) -> Weak<Mutex<ContainerInstance>> {
        if !self.container_to_idx.contains_key(id) {
            self.register(id);
        }

        self.get(id).unwrap()
    }

    pub(crate) fn get_or_create_container_idx(&mut self, id: &ContainerID) -> ContainerIdx {
        if let Some(idx) = self.container_to_idx.get(id) {
            *idx
        } else {
            self.register(id)
        }
    }

    #[cfg(feature = "test_utils")]
    pub fn debug_inspect(&mut self) {
        for (_, ContainerAndId { container, id: _ }) in self.containers.iter_mut() {
            if let ContainerInstance::Text(x) = container.try_lock().unwrap().deref_mut() {
                x.debug_inspect()
            }
        }
    }

    pub fn to_json(&self) -> LoroValue {
        let mut map = FxHashMap::default();
        for (_, ContainerAndId { container, id }) in self.containers.iter() {
            if let ContainerID::Root {
                name,
                container_type,
            } = id
            {
                let container = container.try_lock().unwrap();
                let json = match container.deref() {
                    ContainerInstance::Map(x) => x.to_json(self),
                    ContainerInstance::Text(x) => x.to_json(),
                    ContainerInstance::Dyn(_) => unreachable!("registry to json dyn"),
                    ContainerInstance::List(x) => x.to_json(self),
                };
                if map.contains_key(name.as_ref()) {
                    // TODO: warning
                    map.insert(format!("{}-({:?})", name, container_type), json);
                } else {
                    map.insert(name.to_string(), json);
                }
            }
        }
        LoroValue::Map(Box::new(map))
    }

    pub(crate) fn export_by_sorted_idx(&self) -> Vec<ContainerID> {
        let mut keys: Vec<_> = self.containers.keys().collect();
        keys.sort();
        keys.into_iter()
            .map(|idx| self.containers.get(idx).unwrap().id.clone())
            .collect()
    }
}

impl Default for ContainerRegistry {
    fn default() -> Self {
        Self::new()
    }
}

pub struct ContainerRefMut<'a, T> {
    value: OwningRefMut<RwLockWriteGuard<'a, ContainerRegistry>, Box<T>>,
}

impl<'a, T> From<OwningRefMut<RwLockWriteGuard<'a, ContainerRegistry>, Box<T>>>
    for ContainerRefMut<'a, T>
{
    fn from(value: OwningRefMut<RwLockWriteGuard<'a, ContainerRegistry>, Box<T>>) -> Self {
        ContainerRefMut { value }
    }
}

impl<'a, T> Deref for ContainerRefMut<'a, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.value.deref()
    }
}

impl<'a, T> DerefMut for ContainerRefMut<'a, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.value.deref_mut()
    }
}

pub trait LockContainer {
    type MapTarget<'a>
    where
        Self: 'a;
    type TextTarget<'a>
    where
        Self: 'a;

    fn lock_map(&self) -> Self::MapTarget<'_>;
    fn lock_text(&self) -> Self::TextTarget<'_>;
}

pub trait ContainerWrapper {
    type Container: ContainerTrait;

    fn with_container<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut Self::Container) -> R;

    fn with_transaction<T: Transact, F, R>(&self, txn: &T, f: F) -> Result<R, LoroError>
    where
        F: FnOnce(&mut Transaction, &mut Self::Container) -> Result<R, LoroError>,
    {
        let txn = txn.transact();
        let mut txn = txn.0.borrow_mut();
        let txn = txn.as_mut();
        if txn.client_id != self.client_id() {
            return Err(LoroError::UnmatchedContext {
                expected: self.client_id(),
                found: txn.client_id,
            });
        }
        let ans = self.with_container(|x| f(txn, x));
        if ans.is_err() {
            // TODO: Transaction rollback
        }
        ans
    }

    fn client_id(&self) -> ClientID;

    fn id(&self) -> ContainerID {
        self.with_container(|x| x.id().clone())
    }

    fn idx(&self) -> ContainerIdx;

    fn get_value(&self) -> LoroValue {
        self.with_container(|x| x.get_value())
    }

    fn get_value_deep<C: Context>(&self, ctx: &C) -> LoroValue {
        let m = ctx.log_store();
        let reg = &m.try_read().unwrap().reg;
        let mut value = self.get_value();
        match &mut value {
            LoroValue::List(list) => {
                list.iter_mut().for_each(|x| {
                    if x.as_unresolved().is_some() {
                        *x = x.clone().resolve_deep(reg)
                    }
                });
            }
            LoroValue::Map(map) => {
                map.iter_mut().for_each(|(_, x)| {
                    if x.as_unresolved().is_some() {
                        *x = x.clone().resolve_deep(reg)
                    }
                });
            }
            LoroValue::Unresolved(_) => unreachable!(),
            _ => {}
        }

        value
    }

    fn subscribe<T: Transact>(
        &self,
        txn: &T,
        handler: ObserverHandler,
    ) -> Result<SubscriptionID, LoroError> {
        self.with_transaction(txn, |txn, x| {
            let h = txn.hierarchy.upgrade().unwrap();
            let mut h = h.try_lock().unwrap();
            Ok(x.subscribe(&mut h, handler, false, false))
        })
    }

    fn subscribe_deep<T: Transact>(
        &self,
        txn: &T,
        handler: ObserverHandler,
    ) -> Result<SubscriptionID, LoroError> {
        self.with_transaction(txn, |txn, x| {
            let h = txn.hierarchy.upgrade().unwrap();
            let mut h = h.try_lock().unwrap();
            Ok(x.subscribe(&mut h, handler, true, false))
        })
    }

    fn subscribe_once<T: Transact>(
        &self,
        txn: &T,
        handler: ObserverHandler,
    ) -> Result<SubscriptionID, LoroError> {
        self.with_transaction(txn, |txn, x| {
            let h = txn.hierarchy.upgrade().unwrap();
            let mut h = h.try_lock().unwrap();
            Ok(x.subscribe(&mut h, handler, false, true))
        })
    }

    fn subscribe_deep_once<T: Transact>(
        &self,
        txn: &T,
        handler: ObserverHandler,
    ) -> Result<SubscriptionID, LoroError> {
        self.with_transaction(txn, |txn, x| {
            let h = txn.hierarchy.upgrade().unwrap();
            let mut h = h.try_lock().unwrap();
            Ok(x.subscribe(&mut h, handler, true, true))
        })
    }

    fn unsubscribe<T: Transact>(
        &self,
        txn: &T,
        subscription: SubscriptionID,
    ) -> Result<(), LoroError> {
        self.with_transaction(txn, |txn, x| {
            let h = txn.hierarchy.upgrade().unwrap();
            let mut h = h.try_lock().unwrap();
            Ok(x.unsubscribe(&mut h, subscription))
        })
    }
}

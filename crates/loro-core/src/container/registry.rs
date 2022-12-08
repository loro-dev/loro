use std::{
    ops::{Deref, DerefMut},
    sync::{Arc, Mutex, RwLockWriteGuard},
};

use enum_as_inner::EnumAsInner;

use fxhash::FxHashMap;
use owning_ref::OwningRefMut;
use smallvec::SmallVec;
use tracing::instrument;

use crate::{
    context::Context,
    event::{Index, Observer, SubscriptionID},
    hierarchy::Hierarchy,
    id::ClientID,
    log_store::ImportContext,
    op::{RemoteContent, RichOp},
    version::IdSpanVector,
    LoroError, LoroValue,
};

use super::{
    list::ListContainer, map::MapContainer, text::TextContainer, Container, ContainerID,
    ContainerType,
};

#[derive(PartialEq, Eq, Clone, Copy, Hash, Debug)]
pub(crate) struct ContainerIdx(u32);

impl ContainerIdx {
    pub(crate) fn to_u32(self) -> u32 {
        self.0
    }
}

// TODO: replace this with a fat pointer?
#[derive(Debug, EnumAsInner)]
pub enum ContainerInstance {
    Map(Box<MapContainer>),
    Text(Box<TextContainer>),
    List(Box<ListContainer>),
    Dyn(Box<dyn Container>),
}

impl Container for ContainerInstance {
    fn id(&self) -> &ContainerID {
        match self {
            ContainerInstance::Map(x) => x.id(),
            ContainerInstance::Text(x) => x.id(),
            ContainerInstance::Dyn(x) => x.id(),
            ContainerInstance::List(x) => x.id(),
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
    fn tracker_checkout(&mut self, vv: &crate::VersionVector) {
        match self {
            ContainerInstance::Map(x) => x.tracker_checkout(vv),
            ContainerInstance::Text(x) => x.tracker_checkout(vv),
            ContainerInstance::Dyn(x) => x.tracker_checkout(vv),
            ContainerInstance::List(x) => x.tracker_checkout(vv),
        }
    }

    #[instrument(skip_all)]
    fn get_value(&self) -> crate::LoroValue {
        match self {
            ContainerInstance::Map(x) => x.get_value(),
            ContainerInstance::Text(x) => x.get_value(),
            ContainerInstance::Dyn(x) => x.get_value(),
            ContainerInstance::List(x) => x.get_value(),
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
            ContainerInstance::Dyn(x) => x.update_state_directly(hierarchy, op, context),
            ContainerInstance::List(x) => x.update_state_directly(hierarchy, op, context),
        }
    }

    #[instrument(skip_all)]
    fn track_retreat(&mut self, op: &IdSpanVector) {
        match self {
            ContainerInstance::Map(x) => x.track_retreat(op),
            ContainerInstance::Text(x) => x.track_retreat(op),
            ContainerInstance::Dyn(x) => x.track_retreat(op),
            ContainerInstance::List(x) => x.track_retreat(op),
        }
    }

    #[instrument(skip_all)]
    fn track_forward(&mut self, op: &IdSpanVector) {
        match self {
            ContainerInstance::Map(x) => x.track_forward(op),
            ContainerInstance::Text(x) => x.track_forward(op),
            ContainerInstance::Dyn(x) => x.track_forward(op),
            ContainerInstance::List(x) => x.track_forward(op),
        }
    }

    #[instrument(skip_all)]
    fn track_apply(&mut self, hierarchy: &mut Hierarchy, op: &RichOp, ctx: &mut ImportContext) {
        match self {
            ContainerInstance::Map(x) => x.track_apply(hierarchy, op, ctx),
            ContainerInstance::Text(x) => x.track_apply(hierarchy, op, ctx),
            ContainerInstance::Dyn(x) => x.track_apply(hierarchy, op, ctx),
            ContainerInstance::List(x) => x.track_apply(hierarchy, op, ctx),
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
            ContainerInstance::Dyn(x) => x.apply_tracked_effects_from(h, import_context),
            ContainerInstance::List(x) => x.apply_tracked_effects_from(h, import_context),
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
            ContainerInstance::Dyn(x) => x.to_export(content, gc),
            ContainerInstance::List(x) => x.to_export(content, gc),
        }
    }

    #[instrument(skip_all)]
    fn to_import(&mut self, content: crate::op::RemoteContent) -> crate::op::InnerContent {
        match self {
            ContainerInstance::Map(x) => x.to_import(content),
            ContainerInstance::Text(x) => x.to_import(content),
            ContainerInstance::Dyn(x) => x.to_import(content),
            ContainerInstance::List(x) => x.to_import(content),
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
    containers: Vec<ContainerAndId>,
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
            containers: Vec::new(),
        }
    }

    #[inline]
    fn create(&mut self, id: ContainerID) -> ContainerInstance {
        match id.container_type() {
            ContainerType::Map => ContainerInstance::Map(Box::new(MapContainer::new(id))),
            ContainerType::Text => ContainerInstance::Text(Box::new(TextContainer::new(id))),
            ContainerType::List => ContainerInstance::List(Box::new(ListContainer::new(id))),
        }
    }

    #[inline(always)]
    pub fn get(&self, id: &ContainerID) -> Option<&Arc<Mutex<ContainerInstance>>> {
        self.container_to_idx
            .get(id)
            .map(|x| &self.containers[x.0 as usize].container)
    }

    #[inline(always)]
    pub(crate) fn get_by_idx(&self, idx: ContainerIdx) -> Option<&Arc<Mutex<ContainerInstance>>> {
        self.containers.get(idx.0 as usize).map(|x| &x.container)
    }

    #[inline(always)]
    pub(crate) fn get_idx(&self, id: &ContainerID) -> Option<ContainerIdx> {
        self.container_to_idx.get(id).copied()
    }

    pub(crate) fn get_id(&self, idx: ContainerIdx) -> Option<&ContainerID> {
        self.containers.get(idx.0 as usize).map(|x| &x.id)
    }

    #[inline(always)]
    fn insert(&mut self, id: ContainerID, container: ContainerInstance) -> ContainerIdx {
        let idx = self.next_idx();
        self.container_to_idx.insert(id.clone(), idx);
        self.containers.push(ContainerAndId {
            container: Arc::new(Mutex::new(container)),
            id,
        });

        idx
    }

    #[inline(always)]
    fn next_idx(&self) -> ContainerIdx {
        ContainerIdx(self.containers.len() as u32)
    }

    pub(crate) fn register(&mut self, id: &ContainerID) -> ContainerIdx {
        let container = self.create(id.clone());
        self.insert(id.clone(), container)
    }

    pub(crate) fn get_or_create(&mut self, id: &ContainerID) -> &Arc<Mutex<ContainerInstance>> {
        if !self.container_to_idx.contains_key(id) {
            let container = self.create(id.clone());
            self.insert(id.clone(), container);
        }

        let container = self.get(id).unwrap();
        container
    }

    pub(crate) fn get_or_create_container_idx(&mut self, id: &ContainerID) -> ContainerIdx {
        if let Some(idx) = self.container_to_idx.get(id) {
            *idx
        } else {
            let container = self.create(id.clone());
            self.insert(id.clone(), container)
        }
    }

    #[cfg(feature = "test_utils")]
    pub fn debug_inspect(&mut self) {
        for ContainerAndId { container, id: _ } in self.containers.iter_mut() {
            if let ContainerInstance::Text(x) = container.lock().unwrap().deref_mut() {
                x.debug_inspect()
            }
        }
    }

    pub fn to_json(&self) -> LoroValue {
        let mut map = FxHashMap::default();
        for ContainerAndId { container, id } in self.containers.iter() {
            if let ContainerID::Root {
                name,
                container_type,
            } = id
            {
                let container = container.lock().unwrap();
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

    pub(crate) fn export(&self) -> (&FxHashMap<ContainerID, ContainerIdx>, Vec<ContainerID>) {
        (
            &self.container_to_idx,
            self.containers.iter().map(|x| x.id.clone()).collect(),
        )
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
    type Container: Container;

    fn with_container<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut Self::Container) -> R;

    fn with_container_checked<C: Context, F, R>(&self, ctx: &C, f: F) -> Result<R, LoroError>
    where
        F: FnOnce(&mut Self::Container) -> R,
    {
        let store_client_id = ctx.log_store().read().unwrap().this_client_id();
        if store_client_id != self.client_id() {
            return Err(LoroError::UnmatchedContext {
                expected: self.client_id(),
                found: store_client_id,
            });
        }
        Ok(self.with_container(f))
    }

    fn client_id(&self) -> ClientID;

    fn id(&self) -> ContainerID {
        self.with_container(|x| x.id().clone())
    }

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

    fn subscribe<C: Context>(
        &self,
        ctx: &C,
        observer: Observer,
    ) -> Result<SubscriptionID, LoroError> {
        self.with_container_checked(ctx, |x| {
            x.subscribe(
                &mut ctx
                    .log_store()
                    .write()
                    .unwrap()
                    .hierarchy
                    .try_lock()
                    .unwrap(),
                observer,
                false,
            )
        })
    }

    fn subscribe_deep<C: Context>(
        &self,
        ctx: &C,
        observer: Observer,
    ) -> Result<SubscriptionID, LoroError> {
        self.with_container_checked(ctx, |x| {
            x.subscribe(
                &mut ctx
                    .log_store()
                    .write()
                    .unwrap()
                    .hierarchy
                    .try_lock()
                    .unwrap(),
                observer,
                true,
            )
        })
    }

    fn unsubscribe<C: Context>(
        &self,
        ctx: &C,
        subscription: SubscriptionID,
    ) -> Result<(), LoroError> {
        self.with_container_checked(ctx, |x| {
            x.unsubscribe(
                &mut ctx
                    .log_store()
                    .write()
                    .unwrap()
                    .hierarchy
                    .try_lock()
                    .unwrap(),
                subscription,
            )
        })
    }
}

use std::{
    ops::{Deref, DerefMut},
    sync::{Arc, Mutex, RwLockWriteGuard},
};

use enum_as_inner::EnumAsInner;

use fxhash::FxHashMap;
use owning_ref::OwningRefMut;
use smallvec::SmallVec;

use crate::{
    context::Context,
    id::{ClientID, ContainerIdx},
    op::{RemoteContent, RichOp},
    version::IdSpanVector,
    LoroError, LoroValue, VersionVector,
};

use super::{
    list::ListContainer, map::MapContainer, text::TextContainer, Container, ContainerID,
    ContainerType,
};

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

    fn tracker_checkout(&mut self, vv: &crate::VersionVector) {
        match self {
            ContainerInstance::Map(x) => x.tracker_checkout(vv),
            ContainerInstance::Text(x) => x.tracker_checkout(vv),
            ContainerInstance::Dyn(x) => x.tracker_checkout(vv),
            ContainerInstance::List(x) => x.tracker_checkout(vv),
        }
    }

    fn get_value(&self) -> crate::LoroValue {
        match self {
            ContainerInstance::Map(x) => x.get_value(),
            ContainerInstance::Text(x) => x.get_value(),
            ContainerInstance::Dyn(x) => x.get_value(),
            ContainerInstance::List(x) => x.get_value(),
        }
    }

    fn update_state_directly(&mut self, op: &RichOp) {
        match self {
            ContainerInstance::Map(x) => x.update_state_directly(op),
            ContainerInstance::Text(x) => x.update_state_directly(op),
            ContainerInstance::Dyn(x) => x.update_state_directly(op),
            ContainerInstance::List(x) => x.update_state_directly(op),
        }
    }

    fn track_retreat(&mut self, op: &IdSpanVector) {
        match self {
            ContainerInstance::Map(x) => x.track_retreat(op),
            ContainerInstance::Text(x) => x.track_retreat(op),
            ContainerInstance::Dyn(x) => x.track_retreat(op),
            ContainerInstance::List(x) => x.track_retreat(op),
        }
    }

    fn track_forward(&mut self, op: &IdSpanVector) {
        match self {
            ContainerInstance::Map(x) => x.track_forward(op),
            ContainerInstance::Text(x) => x.track_forward(op),
            ContainerInstance::Dyn(x) => x.track_forward(op),
            ContainerInstance::List(x) => x.track_forward(op),
        }
    }

    fn track_apply(&mut self, op: &RichOp) {
        match self {
            ContainerInstance::Map(x) => x.track_apply(op),
            ContainerInstance::Text(x) => x.track_apply(op),
            ContainerInstance::Dyn(x) => x.track_apply(op),
            ContainerInstance::List(x) => x.track_apply(op),
        }
    }

    fn apply_tracked_effects_from(&mut self, from: &VersionVector, effect_spans: &IdSpanVector) {
        match self {
            ContainerInstance::Map(x) => x.apply_tracked_effects_from(from, effect_spans),
            ContainerInstance::Text(x) => x.apply_tracked_effects_from(from, effect_spans),
            ContainerInstance::Dyn(x) => x.apply_tracked_effects_from(from, effect_spans),
            ContainerInstance::List(x) => x.apply_tracked_effects_from(from, effect_spans),
        }
    }

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

    fn to_import(&mut self, content: crate::op::RemoteContent) -> crate::op::InnerContent {
        match self {
            ContainerInstance::Map(x) => x.to_import(content),
            ContainerInstance::Text(x) => x.to_import(content),
            ContainerInstance::Dyn(x) => x.to_import(content),
            ContainerInstance::List(x) => x.to_import(content),
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
            .map(|x| &self.containers[*x as usize].container)
    }

    #[inline(always)]
    pub fn get_by_idx(&self, idx: ContainerIdx) -> Option<&Arc<Mutex<ContainerInstance>>> {
        self.containers.get(idx as usize).map(|x| &x.container)
    }

    #[inline(always)]
    pub fn get_idx(&self, id: &ContainerID) -> Option<ContainerIdx> {
        self.container_to_idx.get(id).copied()
    }

    pub fn get_id(&self, idx: ContainerIdx) -> Option<&ContainerID> {
        self.containers.get(idx as usize).map(|x| &x.id)
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
        self.containers.len() as ContainerIdx
    }

    pub(crate) fn register(&mut self, id: &ContainerID) {
        let container = self.create(id.clone());
        self.insert(id.clone(), container);
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
        let mut value = self.get_value();
        match &mut value {
            LoroValue::List(list) => {
                list.iter_mut().for_each(|x| {
                    if x.as_unresolved().is_some() {
                        *x = x.resolve_deep(ctx).unwrap();
                    }
                });
            }
            LoroValue::Map(map) => {
                map.iter_mut().for_each(|(_, x)| {
                    if x.as_unresolved().is_some() {
                        *x = x.resolve_deep(ctx).unwrap();
                    }
                });
            }
            LoroValue::Unresolved(_) => unreachable!(),
            _ => {}
        }

        value
    }
}

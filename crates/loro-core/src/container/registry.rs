use std::{
    ops::{Deref, DerefMut},
    sync::{Arc, Mutex, RwLockReadGuard, RwLockWriteGuard},
};

use enum_as_inner::EnumAsInner;

use fxhash::FxHashMap;
use owning_ref::{OwningRef, OwningRefMut};

use crate::{context::Context, id::ContainerIdx, op::RemoteOp, span::IdSpan, LogStore, LoroValue};

use super::{map::MapContainer, text::TextContainer, Container, ContainerID, ContainerType};

#[derive(Debug, EnumAsInner)]
pub enum ContainerInstance {
    Map(Box<MapContainer>),
    Text(Box<TextContainer>),
    Dyn(Box<dyn Container>),
}

impl Container for ContainerInstance {
    fn id(&self) -> &ContainerID {
        match self {
            ContainerInstance::Map(x) => x.id(),
            ContainerInstance::Text(x) => x.id(),
            ContainerInstance::Dyn(x) => x.id(),
        }
    }

    fn type_(&self) -> ContainerType {
        match self {
            ContainerInstance::Map(_) => ContainerType::Map,
            ContainerInstance::Text(_) => ContainerType::Text,
            ContainerInstance::Dyn(x) => x.type_(),
        }
    }

    fn apply(&mut self, id_span: IdSpan, log: &LogStore) {
        match self {
            ContainerInstance::Map(x) => x.apply(id_span, log),
            ContainerInstance::Text(x) => x.apply(id_span, log),
            ContainerInstance::Dyn(x) => x.apply(id_span, log),
        }
    }

    fn checkout_version(&mut self, vv: &crate::VersionVector) {
        match self {
            ContainerInstance::Map(x) => x.checkout_version(vv),
            ContainerInstance::Text(x) => x.checkout_version(vv),
            ContainerInstance::Dyn(x) => x.checkout_version(vv),
        }
    }

    fn get_value(&self) -> crate::LoroValue {
        match self {
            ContainerInstance::Map(x) => x.get_value(),
            ContainerInstance::Text(x) => x.get_value(),
            ContainerInstance::Dyn(x) => x.get_value(),
        }
    }

    fn to_export(&self, op: &mut crate::op::Op) {
        match self {
            ContainerInstance::Map(x) => x.to_export(op),
            ContainerInstance::Text(x) => x.to_export(op),
            ContainerInstance::Dyn(x) => x.to_export(op),
        }
    }

    fn to_import(&mut self, op: &mut RemoteOp) {
        match self {
            ContainerInstance::Map(x) => x.to_import(op),
            ContainerInstance::Text(x) => x.to_import(op),
            ContainerInstance::Dyn(x) => x.to_import(op),
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
            _ => unimplemented!(),
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
    fn insert(&mut self, id: ContainerID, container: ContainerInstance) {
        let idx = self.next_idx();
        self.container_to_idx.insert(id.clone(), idx);
        self.containers.push(ContainerAndId {
            container: Arc::new(Mutex::new(container)),
            id,
        });
    }

    #[inline(always)]
    fn next_idx(&self) -> ContainerIdx {
        self.containers.len() as ContainerIdx
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
        if !self.container_to_idx.contains_key(id) {
            let container = self.create(id.clone());
            self.insert(id.clone(), container);
        }

        self.get_idx(id).unwrap()
    }

    #[cfg(feature = "fuzzing")]
    pub fn debug_inspect(&mut self) {
        for ContainerAndId { container, id: _ } in self.containers.iter_mut() {
            if let ContainerInstance::Text(x) = container.lock().unwrap().deref_mut() {
                x.debug_inspect()
            }
        }
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

pub struct ContainerRef<'a, T> {
    value: OwningRef<RwLockReadGuard<'a, ContainerRegistry>, Box<T>>,
}

impl<'a, T> From<OwningRefMut<RwLockWriteGuard<'a, ContainerRegistry>, Box<T>>>
    for ContainerRefMut<'a, T>
{
    fn from(value: OwningRefMut<RwLockWriteGuard<'a, ContainerRegistry>, Box<T>>) -> Self {
        ContainerRefMut { value }
    }
}

impl<'a, T> From<OwningRef<RwLockReadGuard<'a, ContainerRegistry>, Box<T>>>
    for ContainerRef<'a, T>
{
    fn from(value: OwningRef<RwLockReadGuard<'a, ContainerRegistry>, Box<T>>) -> Self {
        ContainerRef { value }
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
                        *x = x.resolve(ctx).unwrap();
                    }
                });
            }
            LoroValue::Map(map) => {
                map.iter_mut().for_each(|(_, x)| {
                    if x.as_unresolved().is_some() {
                        *x = x.resolve(ctx).unwrap();
                    }
                });
            }
            LoroValue::Unresolved(_) => unreachable!(),
            _ => {}
        }

        value
    }
}

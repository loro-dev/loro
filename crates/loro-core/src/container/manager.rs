use std::{
    ops::{Deref, DerefMut},
    sync::{Arc, Mutex, MutexGuard, RwLockReadGuard, RwLockWriteGuard, Weak},
};

use dashmap::DashMap;
use enum_as_inner::EnumAsInner;

use owning_ref::{OwningRef, OwningRefMut};

use crate::{log_store::LogStoreWeakRef, op::RemoteOp, span::IdSpan, LogStore};

use super::{
    map::MapContainer, text::text_container::TextContainer, Container, ContainerID, ContainerType,
};

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
pub struct ContainerManager {
    containers: DashMap<ContainerID, Arc<Mutex<ContainerInstance>>>,
    to_self: Weak<ContainerManager>,
}

impl ContainerManager {
    pub(crate) fn new() -> Arc<ContainerManager> {
        Arc::new_cyclic(|x| ContainerManager {
            containers: Default::default(),
            to_self: x.clone(),
        })
    }

    #[inline]
    fn create(&self, id: ContainerID, log_store: LogStoreWeakRef) -> ContainerInstance {
        match id.container_type() {
            ContainerType::Map => ContainerInstance::Map(Box::new(MapContainer::new(
                id,
                log_store,
                self.to_self.clone(),
            ))),
            ContainerType::Text => {
                ContainerInstance::Text(Box::new(TextContainer::new(id, log_store)))
            }
            _ => unimplemented!(),
        }
    }

    #[inline]
    pub fn get(
        &self,
        id: &ContainerID,
    ) -> Option<dashmap::mapref::one::Ref<ContainerID, Arc<Mutex<ContainerInstance>>>> {
        self.containers.get(id)
    }

    #[inline]
    fn insert(&self, id: ContainerID, container: ContainerInstance) {
        self.containers.insert(id, Arc::new(Mutex::new(container)));
    }

    pub(crate) fn get_or_create(
        &self,
        id: &ContainerID,
        log_store: LogStoreWeakRef,
    ) -> dashmap::mapref::one::Ref<ContainerID, Arc<Mutex<ContainerInstance>>> {
        if !self.containers.contains_key(id) {
            let container = self.create(id.clone(), log_store);
            self.insert(id.clone(), container);
        }

        let container = self.get(id).unwrap();
        container
    }

    #[cfg(feature = "fuzzing")]
    pub fn debug_inspect(&self) {
        for container in self.containers.iter_mut() {
            if let ContainerInstance::Text(x) = container.lock().unwrap().deref_mut() {
                x.debug_inspect()
            }
        }
    }
}

pub struct ContainerRefMut<'a, T> {
    value: OwningRefMut<RwLockWriteGuard<'a, ContainerManager>, Box<T>>,
}

pub struct ContainerRef<'a, T> {
    value: OwningRef<RwLockReadGuard<'a, ContainerManager>, Box<T>>,
}

impl<'a, T> From<OwningRefMut<RwLockWriteGuard<'a, ContainerManager>, Box<T>>>
    for ContainerRefMut<'a, T>
{
    fn from(value: OwningRefMut<RwLockWriteGuard<'a, ContainerManager>, Box<T>>) -> Self {
        ContainerRefMut { value }
    }
}

impl<'a, T> From<OwningRef<RwLockReadGuard<'a, ContainerManager>, Box<T>>> for ContainerRef<'a, T> {
    fn from(value: OwningRef<RwLockReadGuard<'a, ContainerManager>, Box<T>>) -> Self {
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

impl LockContainer for Arc<Mutex<ContainerInstance>> {
    type MapTarget<'a> = OwningRefMut<MutexGuard<'a, ContainerInstance>, Box<MapContainer>> where Self:'a;
    type TextTarget<'a> = OwningRefMut<MutexGuard<'a, ContainerInstance>, Box<TextContainer>> where Self:'a;

    fn lock_map(&self) -> Self::MapTarget<'_> {
        let a = OwningRefMut::new(self.lock().unwrap());
        a.map_mut(|x| x.as_map_mut().unwrap())
    }

    fn lock_text(&self) -> Self::TextTarget<'_> {
        let a = OwningRefMut::new(self.lock().unwrap());
        a.map_mut(|x| x.as_text_mut().unwrap())
    }
}

impl<'x> LockContainer for &'x Arc<Mutex<ContainerInstance>> {
    type MapTarget<'a> = OwningRefMut<MutexGuard<'a, ContainerInstance>, Box<MapContainer>> where Self:'a;
    type TextTarget<'a> = OwningRefMut<MutexGuard<'a, ContainerInstance>, Box<TextContainer>> where Self:'a;

    fn lock_map(&self) -> Self::MapTarget<'_> {
        let a = OwningRefMut::new(self.lock().unwrap());
        a.map_mut(|x| x.as_map_mut().unwrap())
    }

    fn lock_text(&self) -> Self::TextTarget<'_> {
        let a = OwningRefMut::new(self.lock().unwrap());
        a.map_mut(|x| x.as_text_mut().unwrap())
    }
}

use std::{
    ptr::NonNull,
    sync::{Arc, RwLock, RwLockWriteGuard},
};

use owning_ref::OwningRefMut;

use crate::{
    configure::Configure,
    container::{
        manager::{ContainerInstance, ContainerManager},
        map::MapContainer,
        ContainerID, ContainerType,
    },
    id::ClientID,
    InternalString, LogStore,
};

pub struct LoroCore {
    pub store: Arc<RwLock<LogStore>>,
    pub container: Arc<RwLock<ContainerManager>>,
}

impl Default for LoroCore {
    fn default() -> Self {
        LoroCore::new(Configure::default(), None)
    }
}

impl LoroCore {
    pub fn new(cfg: Configure, client_id: Option<ClientID>) -> Self {
        let container = Arc::new(RwLock::new(ContainerManager {
            containers: Default::default(),
            store: NonNull::dangling(),
        }));
        Self {
            store: LogStore::new(cfg, client_id, container.clone()),
            container,
        }
    }

    pub fn get_container(
        &mut self,
        name: InternalString,
        container: ContainerType,
    ) -> OwningRefMut<RwLockWriteGuard<ContainerManager>, ContainerInstance> {
        let a = OwningRefMut::new(self.container.write().unwrap());
        a.map_mut(|x| x.get_or_create(&ContainerID::new_root(name, container)))
    }

    pub fn get_map_container(
        &mut self,
        name: InternalString,
    ) -> OwningRefMut<RwLockWriteGuard<ContainerManager>, Box<MapContainer>> {
        let a = OwningRefMut::new(self.container.write().unwrap());
        a.map_mut(|x| {
            x.get_or_create(&ContainerID::new_root(name, ContainerType::Map))
                .as_map_mut()
                .unwrap()
        })
    }
}

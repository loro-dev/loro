use std::{
    ptr::NonNull,
    sync::{Arc, RwLock, RwLockWriteGuard},
};

use owning_ref::OwningRefMut;

use crate::{
    change::Change,
    configure::Configure,
    container::{
        manager::{ContainerInstance, ContainerManager},
        map::MapContainer,
        text::text_container::TextContainer,
        ContainerID, ContainerType,
    },
    id::ClientID,
    InternalString, LogStore, VersionVector,
};

pub struct LoroCore {
    pub log_store: Arc<RwLock<LogStore>>,
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
            log_store: LogStore::new(cfg, client_id, container.clone()),
            container,
        }
    }

    pub fn vv(&self) -> VersionVector {
        self.log_store.read().unwrap().get_vv().clone()
    }

    pub fn get_container(
        &mut self,
        name: InternalString,
        container: ContainerType,
    ) -> OwningRefMut<RwLockWriteGuard<ContainerManager>, ContainerInstance> {
        let a = OwningRefMut::new(self.container.write().unwrap());
        a.map_mut(|x| {
            x.get_or_create(
                &ContainerID::new_root(name, container),
                Arc::downgrade(&self.log_store),
            )
        })
    }

    pub fn get_map_container(
        &mut self,
        name: InternalString,
    ) -> OwningRefMut<RwLockWriteGuard<ContainerManager>, Box<MapContainer>> {
        let a = OwningRefMut::new(self.container.write().unwrap());
        a.map_mut(|x| {
            x.get_or_create(
                &ContainerID::new_root(name, ContainerType::Map),
                Arc::downgrade(&self.log_store),
            )
            .as_map_mut()
            .unwrap()
        })
    }

    pub fn get_text_container(
        &mut self,
        name: InternalString,
    ) -> OwningRefMut<RwLockWriteGuard<ContainerManager>, Box<TextContainer>> {
        let a = OwningRefMut::new(self.container.write().unwrap());
        a.map_mut(|x| {
            x.get_or_create(
                &ContainerID::new_root(name, ContainerType::Text),
                Arc::downgrade(&self.log_store),
            )
            .as_text_mut()
            .unwrap()
        })
    }

    pub fn export(&self, remote_vv: VersionVector) -> Vec<Change> {
        let store = self.log_store.read().unwrap();
        store.export(&remote_vv)
    }

    pub fn import(&mut self, changes: Vec<Change>) {
        let mut store = self.log_store.write().unwrap();
        store.import(changes)
    }
}

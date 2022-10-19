use std::sync::{Arc, RwLock, RwLockWriteGuard};

use owning_ref::{OwningRef, OwningRefMut};

use crate::{
    configure::Configure,
    container::{map::MapContainer, Cast, Container, ContainerID, ContainerType},
    id::ClientID,
    InternalString, LogStore,
};

pub struct LoroCore {
    pub store: Arc<RwLock<LogStore>>,
}

impl Default for LoroCore {
    fn default() -> Self {
        LoroCore::new(Configure::default(), None)
    }
}

impl LoroCore {
    pub fn new(cfg: Configure, client_id: Option<ClientID>) -> Self {
        Self {
            store: LogStore::new(cfg, client_id),
        }
    }

    pub fn get_container<'a>(
        &'a mut self,
        name: InternalString,
        container: ContainerType,
    ) -> OwningRefMut<RwLockWriteGuard<LogStore>, dyn Container + 'a> {
        if let Ok(store) = self.store.write() {
            OwningRefMut::new(store).map_mut(|store| {
                let r = store
                    .container
                    .get_or_create(&ContainerID::new_root(name, container));
                r
            })
        } else {
            todo!()
        }
    }

    pub fn get_map_container(
        &mut self,
        name: InternalString,
    ) -> OwningRefMut<RwLockWriteGuard<LogStore>, MapContainer> {
        if let Ok(store) = self.store.write() {
            OwningRefMut::new(store).map_mut(|store| {
                let r = store
                    .container
                    .get_or_create(&ContainerID::new_root(name, ContainerType::Map));
                r.cast_mut().unwrap()
            })
        } else {
            todo!()
        }
    }
}

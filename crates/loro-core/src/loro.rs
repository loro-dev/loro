use std::sync::{Arc, Mutex, MutexGuard, RwLock};

use owning_ref::{OwningRef, OwningRefMut};

use crate::{
    change::Change,
    configure::Configure,
    container::{
        manager::{ContainerInstance, ContainerManager, ContainerRef, ContainerRefMut},
        map::MapContainer,
        text::text_container::TextContainer,
        ContainerID, ContainerType,
    },
    id::ClientID,
    op::RemoteOp,
    LogStore, LoroError, VersionVector,
};

pub struct LoroCore {
    pub(crate) log_store: Arc<RwLock<LogStore>>,
    pub(crate) container: Arc<RwLock<ContainerManager>>,
}

impl Default for LoroCore {
    fn default() -> Self {
        LoroCore::new(Configure::default(), None)
    }
}

impl LoroCore {
    pub fn new(cfg: Configure, client_id: Option<ClientID>) -> Self {
        let container = ContainerManager::new();
        let weak = Arc::downgrade(&container);
        Self {
            log_store: LogStore::new(cfg, client_id, weak),
            container,
        }
    }

    pub fn vv(&self) -> VersionVector {
        self.log_store.read().unwrap().get_vv().clone()
    }

    #[inline(always)]
    pub fn get_or_create_root_map(&mut self, name: &str) -> Arc<Mutex<ContainerInstance>> {
        let id = ContainerID::new_root(name, ContainerType::Map);
        self.log_store
            .write()
            .unwrap()
            .get_or_create_container_idx(&id);
        let ptr = Arc::downgrade(&self.log_store);
        let mut container = self.container.write().unwrap();
        let map = container.get_or_create(&id, ptr);
        map.clone()
    }

    #[inline(always)]
    pub fn get_or_create_root_text(&mut self, name: &str) -> Arc<Mutex<ContainerInstance>> {
        let mut container = self.container.write().unwrap();
        let id = ContainerID::new_root(name, ContainerType::Text);
        self.log_store
            .write()
            .unwrap()
            .get_or_create_container_idx(&id);
        let ptr = Arc::downgrade(&self.log_store);
        container.get_or_create(&id, ptr).clone()
    }

    #[inline(always)]
    pub fn get_container(&self, id: &ContainerID) -> Option<Arc<Mutex<ContainerInstance>>> {
        let container = self.container.read().unwrap();
        container.get(id).map(|x| x.clone())
    }

    pub fn export(&self, remote_vv: VersionVector) -> Vec<Change<RemoteOp>> {
        let store = self.log_store.read().unwrap();
        store.export(&remote_vv)
    }

    pub fn import(&mut self, changes: Vec<Change<RemoteOp>>) {
        let mut store = self.log_store.write().unwrap();
        store.import(changes)
    }

    #[cfg(feature = "fuzzing")]
    pub fn debug_inspect(&self) {
        self.log_store.write().unwrap().debug_inspect();
        self.container.write().unwrap().debug_inspect();
    }
}

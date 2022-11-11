use std::{
    ops::Deref,
    sync::{Arc, Mutex, RwLock},
};

use crate::{
    change::Change,
    configure::Configure,
    container::{
        registry::{ContainerInstance, ContainerRegistry},
        ContainerID, ContainerType,
    },
    id::ClientID,
    op::RemoteOp,
    LogStore, VersionVector,
};

pub struct LoroCore {
    pub(crate) log_store: Arc<RwLock<LogStore>>,
    pub(crate) container: Arc<ContainerRegistry>,
}

impl Default for LoroCore {
    fn default() -> Self {
        LoroCore::new(Configure::default(), None)
    }
}

impl LoroCore {
    pub fn new(cfg: Configure, client_id: Option<ClientID>) -> Self {
        let container = ContainerRegistry::new();
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
        let map = self.container.get_or_create(&id);
        map.clone()
    }

    #[inline(always)]
    pub fn get_or_create_root_text(&mut self, name: &str) -> Arc<Mutex<ContainerInstance>> {
        let id = ContainerID::new_root(name, ContainerType::Text);
        self.log_store
            .write()
            .unwrap()
            .get_or_create_container_idx(&id);
        self.container.get_or_create(&id).clone()
    }

    #[inline(always)]
    pub fn get_container(&self, id: &ContainerID) -> Option<Arc<Mutex<ContainerInstance>>> {
        self.container.get(id).map(|x| x.deref().clone())
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
        self.container.debug_inspect();
    }
}

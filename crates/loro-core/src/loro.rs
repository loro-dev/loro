use std::{
    sync::{Arc, Mutex, RwLock},
};

use crate::{
    change::Change,
    configure::Configure,
    container::{
        map::Map,
        registry::{ContainerInstance},
        text::Text,
        ContainerID, ContainerType,
    },
    id::ClientID,
    op::RemoteOp,
    LogStore, VersionVector,
};

pub struct LoroCore {
    pub(crate) log_store: Arc<RwLock<LogStore>>,
}

impl Default for LoroCore {
    fn default() -> Self {
        LoroCore::new(Configure::default(), None)
    }
}

impl LoroCore {
    pub fn new(cfg: Configure, client_id: Option<ClientID>) -> Self {
        Self {
            log_store: LogStore::new(cfg, client_id),
        }
    }

    pub fn vv(&self) -> VersionVector {
        self.log_store.read().unwrap().get_vv().clone()
    }

    #[inline(always)]
    pub fn get_map(&mut self, name: &str) -> Map {
        let id = ContainerID::new_root(name, ContainerType::Map);
        self.log_store
            .write()
            .unwrap()
            .get_or_create_container(&id)
            .clone()
            .into()
    }

    #[inline(always)]
    pub fn get_text(&mut self, name: &str) -> Text {
        let id = ContainerID::new_root(name, ContainerType::Text);
        self.log_store
            .write()
            .unwrap()
            .get_or_create_container(&id)
            .clone()
            .into()
    }

    #[inline(always)]
    pub fn get_container(&self, id: &ContainerID) -> Option<Arc<Mutex<ContainerInstance>>> {
        self.log_store
            .read()
            .unwrap()
            .get_container(id)
            .unwrap()
            .clone()
            .into()
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
    }
}

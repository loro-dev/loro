use std::sync::{Arc, Mutex, RwLock};

use fxhash::FxHashMap;
use rle::RleVecWithIndex;

use crate::{
    change::{Change, ChangeMergeCfg},
    configure::Configure,
    container::{
        list::List, map::Map, registry::ContainerInstance, text::Text, ContainerID, ContainerIdRaw,
        ContainerType,
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
    pub fn get_list<I: Into<ContainerIdRaw>>(&mut self, id: I) -> List {
        let id: ContainerIdRaw = id.into();
        self.log_store
            .write()
            .unwrap()
            .get_or_create_container(&id.with_type(ContainerType::List))
            .clone()
            .into()
    }

    #[inline(always)]
    pub fn get_map<I: Into<ContainerIdRaw>>(&mut self, id: I) -> Map {
        let id: ContainerIdRaw = id.into();
        self.log_store
            .write()
            .unwrap()
            .get_or_create_container(&id.with_type(ContainerType::Map))
            .clone()
            .into()
    }

    #[inline(always)]
    pub fn get_text<I: Into<ContainerIdRaw>>(&mut self, id: I) -> Text {
        let id: ContainerIdRaw = id.into();
        self.log_store
            .write()
            .unwrap()
            .get_or_create_container(&id.with_type(ContainerType::Text))
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

    pub fn export(
        &self,
        remote_vv: VersionVector,
    ) -> FxHashMap<u64, RleVecWithIndex<Change<RemoteOp>, ChangeMergeCfg>> {
        let store = self.log_store.read().unwrap();
        store.export(&remote_vv)
    }

    pub fn import(
        &mut self,
        changes: FxHashMap<u64, RleVecWithIndex<Change<RemoteOp>, ChangeMergeCfg>>,
    ) {
        let mut store = self.log_store.write().unwrap();
        store.import(changes)
    }

    pub fn encode_snapshot(&self) -> Vec<u8> {
        let store = self.log_store.read().unwrap();
        store.encode_snapshot()
    }

    pub fn decode_snapshot(input: &[u8], client_id: Option<ClientID>, cfg: Configure) -> Self {
        let log_store = LogStore::decode_snapshot(input, client_id, cfg);
        Self { log_store }
    }

    #[cfg(feature = "test_utils")]
    pub fn debug_inspect(&self) {
        self.log_store.write().unwrap().debug_inspect();
    }
}

use std::sync::{Arc, RwLock};

use crate::{context::Context, LoroValue};
use fxhash::FxHashMap;
use rle::RleVecWithIndex;

use crate::{
    change::{Change, ChangeMergeCfg},
    configure::Configure,
    container::{list::List, map::Map, text::Text, ContainerIdRaw, ContainerType},
    event::{Observer, SubscriptionID},
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

    pub fn client_id(&self) -> ClientID {
        self.log_store.read().unwrap().this_client_id()
    }

    pub fn vv(&self) -> VersionVector {
        self.log_store.read().unwrap().get_vv().clone()
    }

    #[inline(always)]
    pub fn get_list<I: Into<ContainerIdRaw>>(&mut self, id: I) -> List {
        let id: ContainerIdRaw = id.into();
        let mut store = self.log_store.write().unwrap();
        let instance = store
            .get_or_create_container(&id.with_type(ContainerType::List))
            .clone();
        let cid = store.this_client_id();
        List::from_instance(instance, cid)
    }

    #[inline(always)]
    pub fn get_map<I: Into<ContainerIdRaw>>(&mut self, id: I) -> Map {
        let id: ContainerIdRaw = id.into();
        let mut store = self.log_store.write().unwrap();
        let instance = store
            .get_or_create_container(&id.with_type(ContainerType::Map))
            .clone();
        let cid = store.this_client_id();
        Map::from_instance(instance, cid)
    }

    #[inline(always)]
    pub fn get_text<I: Into<ContainerIdRaw>>(&mut self, id: I) -> Text {
        let id: ContainerIdRaw = id.into();
        let mut store = self.log_store.write().unwrap();
        let instance = store
            .get_or_create_container(&id.with_type(ContainerType::Text))
            .clone();
        let cid = store.this_client_id();
        Text::from_instance(instance, cid)
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

    pub fn encode_snapshot(&self, vv: &VersionVector, compress: bool) -> Vec<u8> {
        let store = self.log_store.read().unwrap();
        store.encode_snapshot(vv, compress)
    }

    pub fn decode_snapshot(&mut self, input: &[u8]) {
        self.log_store().try_write().unwrap().decode_snapshot(input);
    }

    pub fn export_store(&self, compress: bool) -> Vec<u8> {
        let store = self.log_store.read().unwrap();
        store.export_store(compress)
    }

    pub fn import_store(input: &[u8], cfg: Configure, client_id: Option<ClientID>) -> Self {
        let store = LogStore::new(cfg, client_id);
        store.write().unwrap().import_store(input);
        Self { log_store: store }
    }

    #[cfg(feature = "test_utils")]
    pub fn debug_inspect(&self) {
        self.log_store.try_write().unwrap().debug_inspect();
    }

    pub fn to_json(&self) -> LoroValue {
        self.log_store.try_read().unwrap().to_json()
    }

    pub fn subscribe_deep(&mut self, observer: Observer) -> SubscriptionID {
        self.log_store
            .write()
            .unwrap()
            .hierarchy
            .subscribe_root(observer)
    }

    pub fn unsubscribe_deep(&mut self, subscription: SubscriptionID) -> bool {
        self.log_store
            .write()
            .unwrap()
            .hierarchy
            .unsubscribe_root(subscription)
    }
}

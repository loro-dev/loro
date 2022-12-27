use std::sync::{Arc, RwLock};

use crate::{
    event::RawEvent,
    log_store::{EncodeConfig, LoroEncoder},
    LoroError, LoroValue,
};
use fxhash::FxHashMap;
use tracing::instrument;

use crate::{
    change::Change,
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

    pub fn vv_cloned(&self) -> VersionVector {
        self.log_store.read().unwrap().get_vv().clone()
    }

    #[inline(always)]
    pub fn get_list<I: Into<ContainerIdRaw>>(&mut self, id: I) -> List {
        let id: ContainerIdRaw = id.into();
        let mut store = self.log_store.write().unwrap();
        let instance = store.get_or_create_container(&id.with_type(ContainerType::List));
        let cid = store.this_client_id();
        List::from_instance(instance, cid)
    }

    #[inline(always)]
    pub fn get_map<I: Into<ContainerIdRaw>>(&mut self, id: I) -> Map {
        let id: ContainerIdRaw = id.into();
        let mut store = self.log_store.write().unwrap();
        let instance = store.get_or_create_container(&id.with_type(ContainerType::Map));
        let cid = store.this_client_id();
        Map::from_instance(instance, cid)
    }

    #[inline(always)]
    pub fn get_text<I: Into<ContainerIdRaw>>(&mut self, id: I) -> Text {
        let id: ContainerIdRaw = id.into();
        let mut store = self.log_store.write().unwrap();
        let instance = store.get_or_create_container(&id.with_type(ContainerType::Text));
        let cid = store.this_client_id();
        Text::from_instance(instance, cid)
    }

    // TODO: make it private
    pub fn export(&self, remote_vv: VersionVector) -> FxHashMap<u64, Vec<Change<RemoteOp>>> {
        let store = self.log_store.read().unwrap();
        store.export(&remote_vv)
    }

    // TODO: make it private
    pub fn import(&mut self, changes: FxHashMap<u64, Vec<Change<RemoteOp>>>) {
        debug_log::group!("Import at {}", self.client_id());
        let mut store = self.log_store.write().unwrap();
        let events = store.import(changes);
        // FIXME: move hierarchy to loro_core
        drop(store);
        self.notify(events);
        debug_log::group_end!();
    }

    pub fn encode(&self, config: EncodeConfig) -> Result<Vec<u8>, LoroError> {
        LoroEncoder::encode(self, config)
    }

    pub fn decode(&mut self, input: &[u8]) -> Result<(), LoroError> {
        let events = LoroEncoder::decode(self, input)?;
        self.notify(events);
        Ok(())
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
            .try_lock()
            .unwrap()
            .subscribe_root(observer)
    }

    pub fn unsubscribe_deep(&mut self, subscription: SubscriptionID) -> bool {
        self.log_store
            .write()
            .unwrap()
            .hierarchy
            .try_lock()
            .unwrap()
            .unsubscribe_root(subscription)
    }

    #[instrument(skip_all)]
    pub fn notify(&self, events: Vec<RawEvent>) {
        let store = self.log_store.read().unwrap();
        let hierarchy = store.hierarchy.clone();
        drop(store);
        let mut h = hierarchy.try_lock().unwrap();
        h.send_notifications(events);
    }
}

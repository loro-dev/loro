use std::sync::{Arc, Mutex, RwLock};

use crate::{
    container::{registry::ContainerIdx, ContainerID},
    context::Context,
    event::{ObserverHandler, RawEvent},
    hierarchy::Hierarchy,
    log_store::LoroEncoder,
    EncodeMode, LoroError, LoroValue,
};
use fxhash::{FxHashMap, FxHashSet};

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
    pub(crate) hierarchy: Arc<Mutex<Hierarchy>>,
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
            hierarchy: Default::default(),
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
        let mut store = self.log_store.try_write().unwrap();
        let instance = store.get_or_create_container(&id.with_type(ContainerType::List));
        let cid = store.this_client_id();
        List::from_instance(instance, cid)
    }

    #[inline(always)]
    pub fn get_map<I: Into<ContainerIdRaw>>(&mut self, id: I) -> Map {
        let id: ContainerIdRaw = id.into();
        let mut store = self.log_store.try_write().unwrap();
        let instance = store.get_or_create_container(&id.with_type(ContainerType::Map));
        let cid = store.this_client_id();
        Map::from_instance(instance, cid)
    }

    #[inline(always)]
    pub fn get_text<I: Into<ContainerIdRaw>>(&mut self, id: I) -> Text {
        let id: ContainerIdRaw = id.into();
        let mut store = self.log_store.try_write().unwrap();
        let instance = store.get_or_create_container(&id.with_type(ContainerType::Text));
        let cid = store.this_client_id();
        Text::from_instance(instance, cid)
    }

    pub fn get_list_by_idx(&self, idx: &ContainerIdx) -> Option<List> {
        let cid = self.client_id();
        self.get_container_by_idx(idx)
            .map(|i| List::from_instance(i, cid))
    }

    pub fn get_map_by_idx(&self, idx: &ContainerIdx) -> Option<Map> {
        let cid = self.client_id();
        self.get_container_by_idx(idx)
            .map(|i| Map::from_instance(i, cid))
    }

    pub fn get_text_by_idx(&self, idx: &ContainerIdx) -> Option<Text> {
        let cid = self.client_id();
        self.get_container_by_idx(idx)
            .map(|i| Text::from_instance(i, cid))
    }

    pub fn contains(&self, id: &ContainerID) -> bool {
        let store = self.log_store.try_read().unwrap();
        store.contains_container(id)
    }

    pub fn children(&self, id: &ContainerID) -> Result<FxHashSet<ContainerID>, LoroError> {
        let hierarchy = self.hierarchy.try_lock().unwrap();
        hierarchy.children(id)
    }

    pub fn parent(&self, id: &ContainerID) -> Result<Option<ContainerID>, LoroError> {
        let hierarchy = self.hierarchy.try_lock().unwrap();
        hierarchy.parent(id)
    }

    // TODO: make it private
    pub fn export(&self, remote_vv: VersionVector) -> FxHashMap<u64, Vec<Change<RemoteOp>>> {
        let store = self.log_store.read().unwrap();
        store.export(&remote_vv)
    }

    // TODO: make it private
    pub fn import(&mut self, changes: FxHashMap<u64, Vec<Change<RemoteOp>>>) {
        debug_log::group!("Import at {}", self.client_id());
        let events = {
            let mut store = self.log_store.write().unwrap();
            let mut hierarchy = self.hierarchy.try_lock().unwrap();
            store.import(&mut hierarchy, changes)
        };
        self.notify(events);
        debug_log::group_end!();
    }

    /// this method will always compress
    pub fn encode_all(&self) -> Vec<u8> {
        LoroEncoder::encode_context(self, EncodeMode::Snapshot)
    }

    /// encode without compress
    pub fn encode_from(&self, from: VersionVector) -> Vec<u8> {
        LoroEncoder::encode_context(self, EncodeMode::Auto(from))
    }

    pub fn encode_with_cfg(&self, mode: EncodeMode) -> Vec<u8> {
        LoroEncoder::encode_context(self, mode)
    }

    pub fn decode(&mut self, input: &[u8]) -> Result<(), LoroError> {
        let events = LoroEncoder::decode_context(self, input)?;
        self.notify(events);
        Ok(())
    }

    pub fn decode_batch(&mut self, input: &[Vec<u8>]) -> Result<(), LoroError> {
        let events = LoroEncoder::decode_batch_context(self, input)?;
        self.notify(events);
        Ok(())
    }

    #[cfg(feature = "test_utils")]
    pub fn diagnose(&self) {
        self.log_store.try_write().unwrap().debug_inspect();
    }

    pub fn to_json(&self) -> LoroValue {
        self.log_store.try_read().unwrap().to_json()
    }

    pub fn subscribe_deep(&mut self, handler: ObserverHandler) -> SubscriptionID {
        let observer = Observer::new_root(handler);
        self.hierarchy.try_lock().unwrap().subscribe(observer)
    }

    pub fn unsubscribe_deep(&mut self, subscription: SubscriptionID) {
        self.hierarchy.try_lock().unwrap().unsubscribe(subscription)
    }

    pub fn subscribe_once(&mut self, handler: ObserverHandler) -> SubscriptionID {
        let observer = Observer::new_root(handler).with_once(true);
        self.hierarchy.try_lock().unwrap().subscribe(observer)
    }

    #[instrument(skip_all)]
    pub(crate) fn notify(&self, events: Vec<RawEvent>) {
        Hierarchy::send_notifications_without_lock(&self.hierarchy, events)
    }

    // config

    pub fn max_change_length(&mut self, max_change_length: usize) {
        self.log_store
            .write()
            .unwrap()
            .max_change_length(max_change_length);
    }

    pub fn max_change_interval(&mut self, max_change_interval: usize) {
        self.log_store
            .write()
            .unwrap()
            .max_change_interval(max_change_interval);
    }

    pub fn gc(&mut self, gc: bool) {
        self.log_store.write().unwrap().gc(gc)
    }

    pub fn snapshot_interval(&mut self, snapshot_interval: u64) {
        self.log_store
            .write()
            .unwrap()
            .snapshot_interval(snapshot_interval);
    }
}

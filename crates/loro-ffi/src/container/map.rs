use std::sync::Arc;

use loro::{ContainerTrait, LoroResult, PeerID};

use crate::{ContainerID, LoroValue, LoroValueLike, ValueOrContainer};

use super::{LoroCounter, LoroList, LoroMovableList, LoroText, LoroTree};

#[derive(Debug, Clone)]
pub struct LoroMap {
    pub(crate) map: loro::LoroMap,
}

impl LoroMap {
    pub fn new() -> Self {
        Self {
            map: loro::LoroMap::new(),
        }
    }

    pub fn is_attached(&self) -> bool {
        self.map.is_attached()
    }

    /// If a detached container is attached, this method will return its corresponding attached handler.
    pub fn get_attached(&self) -> Option<Arc<LoroMap>> {
        self.map
            .get_attached()
            .map(|x| Arc::new(LoroMap { map: x }))
    }

    /// Delete a key-value pair from the map.
    pub fn delete(&self, key: &str) -> LoroResult<()> {
        self.map.delete(key)
    }

    /// Iterate over the key-value pairs of the map.
    // pub fn for_each<I>(&self, f: I)
    // where
    //     I: FnMut(&str, loro::ValueOrContainer),
    // {
    //     self.map.for_each(f)
    // }
    /// Insert a key-value pair into the map.
    pub fn insert(&self, key: &str, value: Arc<dyn LoroValueLike>) -> LoroResult<()> {
        self.map.insert(key, value.as_loro_value())
    }

    /// Get the length of the map.
    pub fn len(&self) -> u32 {
        self.map.len() as u32
    }

    /// Get the ID of the map.
    pub fn id(&self) -> ContainerID {
        self.map.id().into()
    }

    /// Whether the map is empty.
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// Get the value of the map with the given key.
    pub fn get(&self, key: &str) -> Option<Arc<dyn ValueOrContainer>> {
        self.map
            .get(key)
            .map(|v| Arc::new(v) as Arc<dyn ValueOrContainer>)
    }

    #[inline]
    pub fn insert_list_container(
        &self,
        key: &str,
        child: Arc<LoroList>,
    ) -> LoroResult<Arc<LoroList>> {
        let c = self
            .map
            .insert_container(key, child.as_ref().clone().list)?;
        Ok(Arc::new(LoroList { list: c }))
    }

    #[inline]
    pub fn insert_map_container(&self, key: &str, child: Arc<LoroMap>) -> LoroResult<Arc<LoroMap>> {
        let c = self.map.insert_container(key, child.as_ref().clone().map)?;
        Ok(Arc::new(LoroMap { map: c }))
    }

    #[inline]
    pub fn insert_text_container(
        &self,
        key: &str,
        child: Arc<LoroText>,
    ) -> LoroResult<Arc<LoroText>> {
        let c = self
            .map
            .insert_container(key, child.as_ref().clone().text)?;
        Ok(Arc::new(LoroText { text: c }))
    }

    #[inline]
    pub fn insert_tree_container(
        &self,
        key: &str,
        child: Arc<LoroTree>,
    ) -> LoroResult<Arc<LoroTree>> {
        let c = self
            .map
            .insert_container(key, child.as_ref().clone().tree)?;
        Ok(Arc::new(LoroTree { tree: c }))
    }

    #[inline]
    pub fn insert_movable_list_container(
        &self,
        key: &str,
        child: Arc<LoroMovableList>,
    ) -> LoroResult<Arc<LoroMovableList>> {
        let c = self
            .map
            .insert_container(key, child.as_ref().clone().list)?;
        Ok(Arc::new(LoroMovableList { list: c }))
    }

    #[inline]
    pub fn insert_counter_container(
        &self,
        key: &str,
        child: Arc<LoroCounter>,
    ) -> LoroResult<Arc<LoroCounter>> {
        let c = self
            .map
            .insert_container(key, child.as_ref().clone().counter)?;
        Ok(Arc::new(LoroCounter { counter: c }))
    }

    /// Get the shallow value of the map.
    ///
    /// It will not convert the state of sub-containers, but represent them as [LoroValue::Container].
    pub fn get_value(&self) -> LoroValue {
        self.map.get_value().into()
    }

    /// Get the deep value of the map.
    ///
    /// It will convert the state of sub-containers into a nested JSON value.
    pub fn get_deep_value(&self) -> LoroValue {
        self.map.get_deep_value().into()
    }

    pub fn is_deleted(&self) -> bool {
        self.map.is_deleted()
    }

    pub fn get_last_editor(&self, key: &str) -> Option<PeerID> {
        self.map.get_last_editor(key)
    }

    pub fn clear(&self) -> LoroResult<()> {
        self.map.clear()
    }

    pub fn keys(&self) -> Vec<String> {
        self.map.keys().map(|k| k.to_string()).collect()
    }

    pub fn values(&self) -> Vec<Arc<dyn ValueOrContainer>> {
        self.map
            .values()
            .map(|v| Arc::new(v) as Arc<dyn ValueOrContainer>)
            .collect()
    }
}

impl Default for LoroMap {
    fn default() -> Self {
        Self::new()
    }
}

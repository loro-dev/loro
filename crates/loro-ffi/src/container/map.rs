use std::sync::Arc;

use loro::{ContainerTrait, LoroResult, PeerID};

use crate::{ContainerID, LoroDoc, LoroValue, LoroValueLike, ValueOrContainer};

use super::{LoroCounter, LoroList, LoroMovableList, LoroText, LoroTree};

#[derive(Debug, Clone)]
pub struct LoroMap {
    pub(crate) inner: loro::LoroMap,
}

impl LoroMap {
    pub fn new() -> Self {
        Self {
            inner: loro::LoroMap::new(),
        }
    }

    pub fn is_attached(&self) -> bool {
        self.inner.is_attached()
    }

    /// If a detached container is attached, this method will return its corresponding attached handler.
    pub fn get_attached(&self) -> Option<Arc<LoroMap>> {
        self.inner
            .get_attached()
            .map(|x| Arc::new(LoroMap { inner: x }))
    }

    /// Delete a key-value pair from the map.
    pub fn delete(&self, key: &str) -> LoroResult<()> {
        self.inner.delete(key)
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
        self.inner.insert(key, value.as_loro_value())
    }

    /// Get the length of the map.
    pub fn len(&self) -> u32 {
        self.inner.len() as u32
    }

    /// Get the ID of the map.
    pub fn id(&self) -> ContainerID {
        self.inner.id().into()
    }

    /// Whether the map is empty.
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Get the value of the map with the given key.
    pub fn get(&self, key: &str) -> Option<ValueOrContainer> {
        self.inner.get(key).map(|v| v.into())
    }

    #[inline]
    pub fn insert_list_container(
        &self,
        key: &str,
        child: Arc<LoroList>,
    ) -> LoroResult<Arc<LoroList>> {
        let c = self
            .inner
            .insert_container(key, child.as_ref().clone().inner)?;
        Ok(Arc::new(LoroList { inner: c }))
    }

    #[inline]
    pub fn insert_map_container(&self, key: &str, child: Arc<LoroMap>) -> LoroResult<Arc<LoroMap>> {
        let c = self
            .inner
            .insert_container(key, child.as_ref().clone().inner)?;
        Ok(Arc::new(LoroMap { inner: c }))
    }

    #[inline]
    pub fn insert_text_container(
        &self,
        key: &str,
        child: Arc<LoroText>,
    ) -> LoroResult<Arc<LoroText>> {
        let c = self
            .inner
            .insert_container(key, child.as_ref().clone().inner)?;
        Ok(Arc::new(LoroText { inner: c }))
    }

    #[inline]
    pub fn insert_tree_container(
        &self,
        key: &str,
        child: Arc<LoroTree>,
    ) -> LoroResult<Arc<LoroTree>> {
        let c = self
            .inner
            .insert_container(key, child.as_ref().clone().inner)?;
        Ok(Arc::new(LoroTree { inner: c }))
    }

    #[inline]
    pub fn insert_movable_list_container(
        &self,
        key: &str,
        child: Arc<LoroMovableList>,
    ) -> LoroResult<Arc<LoroMovableList>> {
        let c = self
            .inner
            .insert_container(key, child.as_ref().clone().inner)?;
        Ok(Arc::new(LoroMovableList { inner: c }))
    }

    #[inline]
    pub fn insert_counter_container(
        &self,
        key: &str,
        child: Arc<LoroCounter>,
    ) -> LoroResult<Arc<LoroCounter>> {
        let c = self
            .inner
            .insert_container(key, child.as_ref().clone().inner)?;
        Ok(Arc::new(LoroCounter { inner: c }))
    }

    /// Get the shallow value of the map.
    ///
    /// It will not convert the state of sub-containers, but represent them as [LoroValue::Container].
    pub fn get_value(&self) -> LoroValue {
        self.inner.get_value().into()
    }

    /// Get the deep value of the map.
    ///
    /// It will convert the state of sub-containers into a nested JSON value.
    pub fn get_deep_value(&self) -> LoroValue {
        self.inner.get_deep_value().into()
    }

    pub fn is_deleted(&self) -> bool {
        self.inner.is_deleted()
    }

    pub fn get_last_editor(&self, key: &str) -> Option<PeerID> {
        self.inner.get_last_editor(key)
    }

    pub fn clear(&self) -> LoroResult<()> {
        self.inner.clear()
    }

    pub fn keys(&self) -> Vec<String> {
        self.inner.keys().map(|k| k.to_string()).collect()
    }

    pub fn values(&self) -> Vec<ValueOrContainer> {
        self.inner.values().map(|v| v.into()).collect()
    }

    pub fn doc(&self) -> Option<Arc<LoroDoc>> {
        self.inner.doc().map(|x| Arc::new(LoroDoc { doc: x }))
    }
}

impl Default for LoroMap {
    fn default() -> Self {
        Self::new()
    }
}

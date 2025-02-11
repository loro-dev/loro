use std::sync::Arc;

use loro::{cursor::Side, ContainerTrait, LoroResult, PeerID};

use crate::{ContainerID, LoroDoc, LoroValue, LoroValueLike, ValueOrContainer};

use super::{Cursor, LoroCounter, LoroList, LoroMap, LoroText, LoroTree};

#[derive(Debug, Clone)]
pub struct LoroMovableList {
    pub(crate) inner: loro::LoroMovableList,
}

impl LoroMovableList {
    pub fn new() -> Self {
        Self {
            inner: loro::LoroMovableList::new(),
        }
    }

    /// Get the container id.
    pub fn id(&self) -> ContainerID {
        self.inner.id().into()
    }

    /// Whether the container is attached to a document
    ///
    /// The edits on a detached container will not be persisted.
    /// To attach the container to the document, please insert it into an attached container.
    pub fn is_attached(&self) -> bool {
        self.inner.is_attached()
    }

    /// If a detached container is attached, this method will return its corresponding attached handler.
    pub fn get_attached(&self) -> Option<Arc<LoroMovableList>> {
        self.inner
            .get_attached()
            .map(|x| Arc::new(LoroMovableList { inner: x }))
    }

    /// Insert a value at the given position.
    pub fn insert(&self, pos: u32, v: Arc<dyn LoroValueLike>) -> LoroResult<()> {
        self.inner.insert(pos as usize, v.as_loro_value())
    }

    /// Delete values at the given position.
    #[inline]
    pub fn delete(&self, pos: u32, len: u32) -> LoroResult<()> {
        self.inner.delete(pos as usize, len as usize)
    }

    /// Get the value at the given position.
    #[inline]
    pub fn get(&self, index: u32) -> Option<ValueOrContainer> {
        self.inner.get(index as usize).map(|v| v.into())
    }

    /// Get the length of the list.
    pub fn len(&self) -> u32 {
        self.inner.len() as u32
    }

    /// Whether the list is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Get the shallow value of the list.
    ///
    /// It will not convert the state of sub-containers, but represent them as [LoroValue::Container].
    pub fn get_value(&self) -> LoroValue {
        self.inner.get_value().into()
    }

    /// Get the deep value of the list.
    ///
    /// It will convert the state of sub-containers into a nested JSON value.
    pub fn get_deep_value(&self) -> LoroValue {
        self.inner.get_deep_value().into()
    }

    /// Pop the last element of the list.
    #[inline]
    pub fn pop(&self) -> LoroResult<Option<ValueOrContainer>> {
        self.inner.pop().map(|v| v.map(|v| v.into()))
    }

    #[inline]
    pub fn push(&self, v: Arc<dyn LoroValueLike>) -> LoroResult<()> {
        self.inner.push(v.as_loro_value())
    }

    #[inline]
    pub fn insert_list_container(
        &self,
        pos: u32,
        child: Arc<LoroList>,
    ) -> LoroResult<Arc<LoroList>> {
        let c = self
            .inner
            .insert_container(pos as usize, child.as_ref().clone().inner)?;
        Ok(Arc::new(LoroList { inner: c }))
    }

    #[inline]
    pub fn insert_map_container(&self, pos: u32, child: Arc<LoroMap>) -> LoroResult<Arc<LoroMap>> {
        let c = self
            .inner
            .insert_container(pos as usize, child.as_ref().clone().inner)?;
        Ok(Arc::new(LoroMap { inner: c }))
    }

    #[inline]
    pub fn insert_text_container(
        &self,
        pos: u32,
        child: Arc<LoroText>,
    ) -> LoroResult<Arc<LoroText>> {
        let c = self
            .inner
            .insert_container(pos as usize, child.as_ref().clone().inner)?;
        Ok(Arc::new(LoroText { inner: c }))
    }

    #[inline]
    pub fn insert_tree_container(
        &self,
        pos: u32,
        child: Arc<LoroTree>,
    ) -> LoroResult<Arc<LoroTree>> {
        let c = self
            .inner
            .insert_container(pos as usize, child.as_ref().clone().inner)?;
        Ok(Arc::new(LoroTree { inner: c }))
    }

    #[inline]
    pub fn insert_movable_list_container(
        &self,
        pos: u32,
        child: Arc<LoroMovableList>,
    ) -> LoroResult<Arc<LoroMovableList>> {
        let c = self
            .inner
            .insert_container(pos as usize, child.as_ref().clone().inner)?;
        Ok(Arc::new(LoroMovableList { inner: c }))
    }

    #[inline]
    pub fn insert_counter_container(
        &self,
        pos: u32,
        child: Arc<LoroCounter>,
    ) -> LoroResult<Arc<LoroCounter>> {
        let c = self
            .inner
            .insert_container(pos as usize, child.as_ref().clone().inner)?;
        Ok(Arc::new(LoroCounter { inner: c }))
    }

    #[inline]
    pub fn set_list_container(&self, pos: u32, child: Arc<LoroList>) -> LoroResult<Arc<LoroList>> {
        let c = self
            .inner
            .set_container(pos as usize, child.as_ref().clone().inner)?;
        Ok(Arc::new(LoroList { inner: c }))
    }

    #[inline]
    pub fn set_map_container(&self, pos: u32, child: Arc<LoroMap>) -> LoroResult<Arc<LoroMap>> {
        let c = self
            .inner
            .set_container(pos as usize, child.as_ref().clone().inner)?;
        Ok(Arc::new(LoroMap { inner: c }))
    }

    #[inline]
    pub fn set_text_container(&self, pos: u32, child: Arc<LoroText>) -> LoroResult<Arc<LoroText>> {
        let c = self
            .inner
            .set_container(pos as usize, child.as_ref().clone().inner)?;
        Ok(Arc::new(LoroText { inner: c }))
    }

    #[inline]
    pub fn set_tree_container(&self, pos: u32, child: Arc<LoroTree>) -> LoroResult<Arc<LoroTree>> {
        let c = self
            .inner
            .set_container(pos as usize, child.as_ref().clone().inner)?;
        Ok(Arc::new(LoroTree { inner: c }))
    }

    #[inline]
    pub fn set_movable_list_container(
        &self,
        pos: u32,
        child: Arc<LoroMovableList>,
    ) -> LoroResult<Arc<LoroMovableList>> {
        let c = self
            .inner
            .set_container(pos as usize, child.as_ref().clone().inner)?;
        Ok(Arc::new(LoroMovableList { inner: c }))
    }

    #[inline]
    pub fn set_counter_container(
        &self,
        pos: u32,
        child: Arc<LoroCounter>,
    ) -> LoroResult<Arc<LoroCounter>> {
        let c = self
            .inner
            .set_container(pos as usize, child.as_ref().clone().inner)?;
        Ok(Arc::new(LoroCounter { inner: c }))
    }

    /// Set the value at the given position.
    pub fn set(&self, pos: u32, value: Arc<dyn LoroValueLike>) -> LoroResult<()> {
        self.inner.set(pos as usize, value.as_loro_value())
    }

    /// Move the value at the given position to the given position.
    pub fn mov(&self, from: u32, to: u32) -> LoroResult<()> {
        self.inner.mov(from as usize, to as usize)
    }

    /// Get the cursor at the given position.
    ///
    /// Using "index" to denote cursor positions can be unstable, as positions may
    /// shift with document edits. To reliably represent a position or range within
    /// a document, it is more effective to leverage the unique ID of each item/character
    /// in a List CRDT or Text CRDT.
    ///
    /// Loro optimizes State metadata by not storing the IDs of deleted elements. This
    /// approach complicates tracking cursors since they rely on these IDs. The solution
    /// recalculates position by replaying relevant history to update stable positions
    /// accurately. To minimize the performance impact of history replay, the system
    /// updates cursor info to reference only the IDs of currently present elements,
    /// thereby reducing the need for replay.
    pub fn get_cursor(&self, pos: u32, side: Side) -> Option<Arc<Cursor>> {
        self.inner
            .get_cursor(pos as usize, side)
            .map(|v| Arc::new(v.into()))
    }

    pub fn to_vec(&self) -> Vec<LoroValue> {
        self.inner.to_vec().into_iter().map(|v| v.into()).collect()
    }

    pub fn clear(&self) -> LoroResult<()> {
        self.inner.clear()
    }

    pub fn is_deleted(&self) -> bool {
        self.inner.is_deleted()
    }

    pub fn get_creator_at(&self, index: u32) -> Option<PeerID> {
        self.inner.get_creator_at(index as usize)
    }

    pub fn get_last_mover_at(&self, index: u32) -> Option<PeerID> {
        self.inner.get_last_mover_at(index as usize)
    }

    pub fn get_last_editor_at(&self, index: u32) -> Option<PeerID> {
        self.inner.get_last_editor_at(index as usize)
    }

    pub fn doc(&self) -> Option<Arc<LoroDoc>> {
        self.inner.doc().map(|x| Arc::new(LoroDoc { doc: x }))
    }
}

impl Default for LoroMovableList {
    fn default() -> Self {
        Self::new()
    }
}

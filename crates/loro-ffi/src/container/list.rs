use std::{ops::Deref, sync::Arc};

use loro::{cursor::Side, ContainerTrait, LoroList as InnerLoroList, LoroResult, ID};

use crate::{ContainerID, LoroDoc, LoroValue, LoroValueLike, ValueOrContainer};

use super::{LoroCounter, LoroMap, LoroMovableList, LoroText, LoroTree};

#[derive(Debug, Clone)]
pub struct LoroList {
    pub(crate) inner: InnerLoroList,
}

impl LoroList {
    pub fn new() -> Self {
        Self {
            inner: InnerLoroList::new(),
        }
    }

    /// Whether the container is attached to a document
    ///
    /// The edits on a detached container will not be persisted.
    /// To attach the container to the document, please insert it into an attached container.
    pub fn is_attached(&self) -> bool {
        self.inner.is_attached()
    }

    /// If a detached container is attached, this method will return its corresponding attached handler.
    pub fn get_attached(&self) -> Option<Arc<LoroList>> {
        self.inner
            .get_attached()
            .map(|x| Arc::new(LoroList { inner: x }))
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
    pub fn get(&self, index: u32) -> Option<Arc<dyn ValueOrContainer>> {
        self.inner
            .get(index as usize)
            .map(|v| Arc::new(v) as Arc<dyn ValueOrContainer>)
    }

    /// Get the deep value of the container.
    #[inline]
    pub fn get_deep_value(&self) -> LoroValue {
        self.inner.get_deep_value().into()
    }

    /// Get the shallow value of the container.
    ///
    /// This does not convert the state of sub-containers; instead, it represents them as [LoroValue::Container].
    #[inline]
    pub fn get_value(&self) -> LoroValue {
        self.inner.get_value().into()
    }

    /// Get the ID of the container.
    #[inline]
    pub fn id(&self) -> ContainerID {
        self.inner.id().into()
    }

    #[inline]
    pub fn len(&self) -> u32 {
        self.inner.len() as u32
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Pop the last element of the list.
    #[inline]
    pub fn pop(&self) -> LoroResult<Option<LoroValue>> {
        self.inner.pop().map(|v| v.map(|v| v.into()))
    }

    #[inline]
    pub fn push(&self, v: Arc<dyn LoroValueLike>) -> LoroResult<()> {
        self.inner.push(v.as_loro_value())
    }

    /// Iterate over the elements of the list.
    // TODO: wrap it in ffi side
    pub fn for_each<I>(&self, f: I)
    where
        I: FnMut(loro::ValueOrContainer),
    {
        self.inner.for_each(f)
    }

    /// Push a container to the list.
    // #[inline]
    // pub fn push_container(&self, child: Arc<dyn ContainerLike>) -> LoroResult<()> {
    //     let c = child.to_container();
    //     self.list.push_container(c)?;
    //     Ok(())
    // }
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

    pub fn get_cursor(&self, pos: u32, side: Side) -> Option<Arc<Cursor>> {
        self.inner
            .get_cursor(pos as usize, side)
            .map(|v| Arc::new(v.into()))
    }

    pub fn is_deleted(&self) -> bool {
        self.inner.is_deleted()
    }

    pub fn to_vec(&self) -> Vec<LoroValue> {
        self.inner.to_vec().into_iter().map(|v| v.into()).collect()
    }

    pub fn clear(&self) -> LoroResult<()> {
        self.inner.clear()
    }

    pub fn get_id_at(&self, index: u32) -> Option<ID> {
        self.inner.get_id_at(index as usize)
    }

    pub fn doc(&self) -> Option<Arc<LoroDoc>> {
        self.inner.doc().map(|x| Arc::new(LoroDoc { doc: x }))
    }
}

impl Default for LoroList {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
pub struct Cursor(loro::cursor::Cursor);

impl Cursor {
    pub fn new(id: Option<ID>, container: ContainerID, side: Side, origin_pos: u32) -> Self {
        Self(loro::cursor::Cursor::new(
            id,
            container.into(),
            side,
            origin_pos as usize,
        ))
    }
}

impl Deref for Cursor {
    type Target = loro::cursor::Cursor;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl From<loro::cursor::Cursor> for Cursor {
    fn from(c: loro::cursor::Cursor) -> Self {
        Self(c)
    }
}

impl From<Cursor> for loro::cursor::Cursor {
    fn from(c: Cursor) -> Self {
        c.0
    }
}

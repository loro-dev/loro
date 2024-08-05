use std::{ops::Deref, sync::Arc};

use loro::{cursor::Side, LoroList as InnerLoroList, LoroResult, ID};

use crate::{ContainerID, LoroValue, LoroValueLike, ValueOrContainer};

use super::{LoroCounter, LoroMap, LoroMovableList, LoroText, LoroTree};

#[derive(Debug, Clone)]
pub struct LoroList {
    pub(crate) list: InnerLoroList,
}

impl LoroList {
    pub fn new() -> Self {
        Self {
            list: InnerLoroList::new(),
        }
    }

    /// Insert a value at the given position.
    pub fn insert(&self, pos: u32, v: Arc<dyn LoroValueLike>) -> LoroResult<()> {
        self.list.insert(pos as usize, v.as_loro_value())
    }

    /// Delete values at the given position.
    #[inline]
    pub fn delete(&self, pos: u32, len: u32) -> LoroResult<()> {
        self.list.delete(pos as usize, len as usize)
    }

    /// Get the value at the given position.
    #[inline]
    pub fn get(&self, index: u32) -> Option<Arc<dyn ValueOrContainer>> {
        self.list
            .get(index as usize)
            .map(|v| Arc::new(v) as Arc<dyn ValueOrContainer>)
    }

    /// Get the deep value of the container.
    #[inline]
    pub fn get_deep_value(&self) -> LoroValue {
        self.list.get_deep_value().into()
    }

    /// Get the shallow value of the container.
    ///
    /// This does not convert the state of sub-containers; instead, it represents them as [LoroValue::Container].
    #[inline]
    pub fn get_value(&self) -> LoroValue {
        self.list.get_value().into()
    }

    /// Get the ID of the container.
    #[inline]
    pub fn id(&self) -> ContainerID {
        self.list.id().into()
    }

    /// Pop the last element of the list.
    #[inline]
    pub fn pop(&self) -> LoroResult<Option<LoroValue>> {
        self.list.pop().map(|v| v.map(|v| v.into()))
    }

    #[inline]
    pub fn push(&self, v: Arc<dyn LoroValueLike>) -> LoroResult<()> {
        self.list.push(v.as_loro_value())
    }

    /// Iterate over the elements of the list.
    // TODO: wrap it in ffi side
    pub fn for_each<I>(&self, f: I)
    where
        I: FnMut((usize, loro::ValueOrContainer)),
    {
        self.list.for_each(f)
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
            .list
            .insert_container(pos as usize, child.as_ref().clone().list)?;
        Ok(Arc::new(LoroList { list: c }))
    }

    #[inline]
    pub fn insert_map_container(&self, pos: u32, child: Arc<LoroMap>) -> LoroResult<Arc<LoroMap>> {
        let c = self
            .list
            .insert_container(pos as usize, child.as_ref().clone().map)?;
        Ok(Arc::new(LoroMap { map: c }))
    }

    #[inline]
    pub fn insert_text_container(
        &self,
        pos: u32,
        child: Arc<LoroText>,
    ) -> LoroResult<Arc<LoroText>> {
        let c = self
            .list
            .insert_container(pos as usize, child.as_ref().clone().text)?;
        Ok(Arc::new(LoroText { text: c }))
    }

    #[inline]
    pub fn insert_tree_container(
        &self,
        pos: u32,
        child: Arc<LoroTree>,
    ) -> LoroResult<Arc<LoroTree>> {
        let c = self
            .list
            .insert_container(pos as usize, child.as_ref().clone().tree)?;
        Ok(Arc::new(LoroTree { tree: c }))
    }

    #[inline]
    pub fn insert_movable_list_container(
        &self,
        pos: u32,
        child: Arc<LoroMovableList>,
    ) -> LoroResult<Arc<LoroMovableList>> {
        let c = self
            .list
            .insert_container(pos as usize, child.as_ref().clone().list)?;
        Ok(Arc::new(LoroMovableList { list: c }))
    }

    #[inline]
    pub fn insert_counter_container(
        &self,
        pos: u32,
        child: Arc<LoroCounter>,
    ) -> LoroResult<Arc<LoroCounter>> {
        let c = self
            .list
            .insert_container(pos as usize, child.as_ref().clone().counter)?;
        Ok(Arc::new(LoroCounter { counter: c }))
    }

    pub fn get_cursor(&self, pos: u32, side: Side) -> Option<Arc<Cursor>> {
        self.list
            .get_cursor(pos as usize, side)
            .map(|v| Arc::new(v.into()))
    }
}

impl Default for LoroList {
    fn default() -> Self {
        Self::new()
    }
}

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

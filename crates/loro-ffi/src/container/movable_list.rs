use std::sync::Arc;

use loro::{cursor::Side, LoroResult};

use crate::{ContainerID, LoroValue, LoroValueLike, ValueOrContainer};

use super::{Cursor, LoroCounter, LoroList, LoroMap, LoroText, LoroTree};

#[derive(Debug, Clone)]
pub struct LoroMovableList {
    pub(crate) list: loro::LoroMovableList,
}

impl LoroMovableList {
    pub fn new() -> Self {
        Self {
            list: loro::LoroMovableList::new(),
        }
    }

    /// Get the container id.
    pub fn id(&self) -> ContainerID {
        self.list.id().into()
    }

    /// Whether the container is attached to a document
    ///
    /// The edits on a detached container will not be persisted.
    /// To attach the container to the document, please insert it into an attached container.
    pub fn is_attached(&self) -> bool {
        self.list.is_attached()
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

    /// Get the length of the list.
    pub fn len(&self) -> u32 {
        self.list.len() as u32
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
        self.list.get_value().into()
    }

    /// Get the deep value of the list.
    ///
    /// It will convert the state of sub-containers into a nested JSON value.
    pub fn get_deep_value(&self) -> LoroValue {
        self.list.get_deep_value().into()
    }

    /// Pop the last element of the list.
    #[inline]
    pub fn pop(&self) -> LoroResult<Option<Arc<dyn ValueOrContainer>>> {
        self.list
            .pop()
            .map(|v| v.map(|v| Arc::new(v) as Arc<dyn ValueOrContainer>))
    }

    #[inline]
    pub fn push(&self, v: Arc<dyn LoroValueLike>) -> LoroResult<()> {
        self.list.push(v.as_loro_value())
    }

    /// Push a container to the end of the list.
    // pub fn push_container<C: ContainerTrait>(&self, child: C) -> LoroResult<C> {
    //     let pos = self.list.len();
    //     Ok(C::from_list(
    //         self.list.insert_container(pos, child.to_list())?,
    //     ))
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

    #[inline]
    pub fn set_list_container(&self, pos: u32, child: Arc<LoroList>) -> LoroResult<Arc<LoroList>> {
        let c = self
            .list
            .set_container(pos as usize, child.as_ref().clone().list)?;
        Ok(Arc::new(LoroList { list: c }))
    }

    #[inline]
    pub fn set_map_container(&self, pos: u32, child: Arc<LoroMap>) -> LoroResult<Arc<LoroMap>> {
        let c = self
            .list
            .set_container(pos as usize, child.as_ref().clone().map)?;
        Ok(Arc::new(LoroMap { map: c }))
    }

    #[inline]
    pub fn set_text_container(&self, pos: u32, child: Arc<LoroText>) -> LoroResult<Arc<LoroText>> {
        let c = self
            .list
            .set_container(pos as usize, child.as_ref().clone().text)?;
        Ok(Arc::new(LoroText { text: c }))
    }

    #[inline]
    pub fn set_tree_container(&self, pos: u32, child: Arc<LoroTree>) -> LoroResult<Arc<LoroTree>> {
        let c = self
            .list
            .set_container(pos as usize, child.as_ref().clone().tree)?;
        Ok(Arc::new(LoroTree { tree: c }))
    }

    #[inline]
    pub fn set_movable_list_container(
        &self,
        pos: u32,
        child: Arc<LoroMovableList>,
    ) -> LoroResult<Arc<LoroMovableList>> {
        let c = self
            .list
            .set_container(pos as usize, child.as_ref().clone().list)?;
        Ok(Arc::new(LoroMovableList { list: c }))
    }

    #[inline]
    pub fn set_counter_container(
        &self,
        pos: u32,
        child: Arc<LoroCounter>,
    ) -> LoroResult<Arc<LoroCounter>> {
        let c = self
            .list
            .set_container(pos as usize, child.as_ref().clone().counter)?;
        Ok(Arc::new(LoroCounter { counter: c }))
    }

    /// Set the value at the given position.
    pub fn set(&self, pos: u32, value: Arc<dyn LoroValueLike>) -> LoroResult<()> {
        self.list.set(pos as usize, value.as_loro_value())
    }

    /// Move the value at the given position to the given position.
    pub fn mov(&self, from: u32, to: u32) -> LoroResult<()> {
        self.list.mov(from as usize, to as usize)
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
    ///
    /// # Example
    ///
    /// ```
    /// use loro::LoroDoc;
    /// use loro_internal::cursor::Side;
    ///
    /// let doc = LoroDoc::new();
    /// let list = doc.get_movable_list("list");
    /// list.insert(0, 0).unwrap();
    /// let cursor = list.get_cursor(0, Side::Middle).unwrap();
    /// assert_eq!(doc.get_cursor_pos(&cursor).unwrap().current.pos, 0);
    /// list.insert(0, 0).unwrap();
    /// assert_eq!(doc.get_cursor_pos(&cursor).unwrap().current.pos, 1);
    /// list.insert(0, 0).unwrap();
    /// list.insert(0, 0).unwrap();
    /// assert_eq!(doc.get_cursor_pos(&cursor).unwrap().current.pos, 3);
    /// list.insert(4, 0).unwrap();
    /// assert_eq!(doc.get_cursor_pos(&cursor).unwrap().current.pos, 3);
    /// ```
    pub fn get_cursor(&self, pos: u32, side: Side) -> Option<Arc<Cursor>> {
        self.list
            .get_cursor(pos as usize, side)
            .map(|v| Arc::new(v.into()))
    }
}

impl Default for LoroMovableList {
    fn default() -> Self {
        Self::new()
    }
}

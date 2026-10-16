//! Field-level reads under one state lock. See `context/state-reader.md`.
//!
//! `get_deep_value` builds a `LoroValue` tree: one `HashMap<String, LoroValue>` per map, freed
//! again by the caller. A caller that only needs some fields, or converts the tree into its own
//! types right away, can read through a [`StateReader`] instead. Each read returns a scalar or a
//! child container index; nothing below it is materialized until asked for.

use loro_common::{ContainerID, ContainerType, InternalString, LoroValue};

use super::DocState;
use crate::{arena::SharedArena, container::idx::ContainerIdx, handler::HandlerTrait};

/// One slot of a map or list: a value, or a child container to read further.
#[derive(Debug, Clone)]
pub enum ReadEntry {
    Value(LoroValue),
    Container(ContainerIdx),
}

/// Read access to the document state, borrowed for the duration of one lock.
pub struct StateReader<'a> {
    state: &'a mut DocState,
}

impl std::fmt::Debug for StateReader<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("StateReader")
    }
}

impl<'a> StateReader<'a> {
    pub fn new(state: &'a mut DocState) -> Self {
        Self { state }
    }

    fn arena(&self) -> &SharedArena {
        &self.state.arena
    }

    /// The index of an attached handler of this document; `None` for a detached handler or a
    /// handler of another document.
    pub fn container_of(&self, handler: &impl HandlerTrait) -> Option<ContainerIdx> {
        let attached = handler.attached_handler()?;
        attached
            .is_in_arena(self.arena())
            .then(|| attached.container_idx())
    }

    /// The index of `id`, registering it if needed. Like a handler, it does not check that the
    /// container exists or is alive.
    pub fn container_by_id(&self, id: &ContainerID) -> ContainerIdx {
        self.arena().register_container(id)
    }

    pub fn container_id(&self, idx: ContainerIdx) -> ContainerID {
        self.arena().idx_to_id(idx).unwrap()
    }

    pub fn is_deleted(&mut self, idx: ContainerIdx) -> bool {
        self.state.is_deleted(idx)
    }

    fn entry(&self, value: LoroValue) -> ReadEntry {
        match value {
            LoroValue::Container(id) => ReadEntry::Container(self.arena().register_container(&id)),
            value => ReadEntry::Value(value),
        }
    }

    /// `MapHandler::get_` without the handler: mergeable markers resolve to their child.
    pub fn map_get(&mut self, map: ContainerIdx, key: &str) -> Option<ReadEntry> {
        debug_assert_eq!(map.get_type(), ContainerType::Map);
        let value = self.state.store.map_get(map, key)?;
        if let Some(child) = self.arena().mergeable_child_idx(map, key, &value) {
            return Some(ReadEntry::Container(child));
        }
        Some(self.entry(value))
    }

    pub fn map_len(&mut self, map: ContainerIdx) -> usize {
        self.state.store.map_len(map)
    }

    pub fn map_keys(&mut self, map: ContainerIdx) -> Vec<InternalString> {
        self.state.store.map_keys(map)
    }

    pub fn list_len(&mut self, list: ContainerIdx) -> usize {
        self.state.store.list_len(list)
    }

    pub fn list_get(&mut self, list: ContainerIdx, index: usize) -> Option<ReadEntry> {
        let value = self.state.store.list_get(list, index)?;
        Some(self.entry(value))
    }

    /// All elements of a List or MovableList.
    pub fn list_entries(&mut self, list: ContainerIdx) -> Vec<ReadEntry> {
        let values = self.state.store.list_values(list);
        values.into_iter().map(|v| self.entry(v)).collect()
    }

    /// The shallow value of a container: text as a string, a counter as a number, and child
    /// containers of maps and lists as `LoroValue::Container`.
    pub fn value(&mut self, idx: ContainerIdx) -> LoroValue {
        self.state
            .store
            .get_value_ephemeral(idx)
            .unwrap_or_else(|| idx.get_type().default_value())
    }

    /// The same value as the container handler's `get_deep_value`.
    pub fn deep_value(&mut self, idx: ContainerIdx) -> LoroValue {
        self.state.get_container_deep_value(idx)
    }
}

impl DocState {
    /// Run `f` with a [`StateReader`] over this state.
    pub fn with_reader<R>(&mut self, f: impl FnOnce(&mut StateReader<'_>) -> R) -> R {
        f(&mut StateReader::new(self))
    }
}

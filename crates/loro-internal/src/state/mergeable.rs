//! Mergeable-container edge resolution on `DocState`.
//!
//! Mergeable child containers (created via `MapHandler::ensure_mergeable_*`) use deterministic
//! `ContainerID::Root` ids in a reserved namespace. The cid encodes `(parent, key, kind)`, while
//! the parent map's compact binary child ref is the single source of truth for whether that child
//! is currently active.
//!
//! The ref is intentionally stored as a specially constructed binary marker value. This keeps
//! user-editable strings from being reinterpreted as internal container topology, and lets the
//! resolver fail closed unless the ref's digest matches the exact `(parent, key, kind)` being
//! resolved.

use loro_common::{ContainerID, ContainerType, LoroMapValue, LoroValue};

use crate::{arena::MergeableEdge, container::idx::ContainerIdx, event::Index};

use super::{ContainerState, DocState};

impl DocState {
    /// Resolve a child edge using the document-level logical edge model.
    ///
    /// Ordinary child containers use the materialized child index stored in their parent state.
    /// Mergeable children are resolved lazily from their deterministic cid plus the parent map's
    /// current binary child ref, so snapshot/update import does not need any eager mergeable-edge
    /// rebuild.
    pub(super) fn get_logical_child_index(
        &mut self,
        parent_idx: ContainerIdx,
        child_idx: ContainerIdx,
        child_id: &ContainerID,
    ) -> Option<Index> {
        // The arena caches the parsed edge of every mergeable cid, so this does not re-parse
        // the cid string or re-hash the marker.
        if let Some(Some(edge)) = self.arena.mergeable_edge(child_idx) {
            return self.resolve_mergeable_child_index(parent_idx, &edge);
        }

        self.store
            .get_container_mut(parent_idx)
            .and_then(|parent_state| parent_state.get_child_index(child_id))
    }

    pub(super) fn contains_logical_child(
        &mut self,
        parent_idx: ContainerIdx,
        child_idx: ContainerIdx,
        child_id: &ContainerID,
    ) -> bool {
        self.get_logical_child_index(parent_idx, child_idx, child_id)
            .is_some()
    }

    /// The child is active when it hangs under the parent its cid encodes and that parent map
    /// holds its marker at its key.
    fn resolve_mergeable_child_index(
        &mut self,
        parent_idx: ContainerIdx,
        edge: &MergeableEdge,
    ) -> Option<Index> {
        if parent_idx.get_type() != ContainerType::Map || parent_idx != edge.parent {
            return None;
        }

        let value = self.store.map_get(parent_idx, &edge.key)?;
        edge.is_activated_by(&value)
            .then(|| Index::Key(edge.key.clone()))
    }

    /// Derive `(key, cid, idx)` for the active mergeable children of a map directly from its
    /// current value table.
    ///
    /// This is used by deep-value and alive-container walks. It mirrors regular map semantics:
    /// the current value at a key determines which child is visible. For mergeable children, that
    /// value is a compact binary child ref rather than `LoroValue::Container`. Children already
    /// registered are found through the arena without rebuilding their cids.
    pub(super) fn mergeable_children_from_value<'a>(
        &self,
        parent_idx: ContainerIdx,
        map_value: &'a LoroMapValue,
    ) -> Vec<(&'a str, ContainerID, ContainerIdx)> {
        map_value
            .iter()
            .filter_map(|(key, value)| {
                let idx = self.mergeable_child_of_value(parent_idx, key, value)?;
                Some((key.as_str(), self.arena.idx_to_id(idx).unwrap(), idx))
            })
            .collect()
    }

    /// The mergeable child that `value`, stored at `key` in the map `parent_idx`, activates.
    pub(super) fn mergeable_child_of_value(
        &self,
        parent_idx: ContainerIdx,
        key: &str,
        value: &LoroValue,
    ) -> Option<ContainerIdx> {
        self.arena.mergeable_child_idx(parent_idx, key, value)
    }
}

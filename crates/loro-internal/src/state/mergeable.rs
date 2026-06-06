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

use loro_common::{ContainerID, ContainerType, InternalString, LoroMapValue};

use crate::{container::idx::ContainerIdx, event::Index};

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
        child_id: &ContainerID,
    ) -> Option<Index> {
        if let Some((parent_id, key, kind)) = child_id.parse_mergeable() {
            return self.resolve_mergeable_child_index(parent_idx, &parent_id, &key, kind);
        }

        self.store
            .get_container_mut(parent_idx)
            .and_then(|parent_state| parent_state.get_child_index(child_id))
    }

    pub(super) fn contains_logical_child(
        &mut self,
        parent_idx: ContainerIdx,
        child_id: &ContainerID,
    ) -> bool {
        self.get_logical_child_index(parent_idx, child_id).is_some()
    }

    fn resolve_mergeable_child_index(
        &mut self,
        parent_idx: ContainerIdx,
        encoded_parent_id: &ContainerID,
        key: &str,
        kind: ContainerType,
    ) -> Option<Index> {
        if parent_idx.get_type() != ContainerType::Map {
            return None;
        }

        let actual_parent_id = self.arena.idx_to_id(parent_idx)?;
        if &actual_parent_id != encoded_parent_id {
            return None;
        }

        let value = self.store.map_get(parent_idx, key)?;
        if loro_common::parse_mergeable_marker(&actual_parent_id, key, &value) == Some(kind) {
            Some(Index::Key(key.into()))
        } else {
            None
        }
    }

    /// Derive `(key, cid)` pairs for the active mergeable children of a map directly from its
    /// current value table.
    ///
    /// This is used by deep-value and alive-container walks. It mirrors regular map semantics:
    /// the current value at a key determines which child is visible. For mergeable children, that
    /// value is a compact binary child ref rather than `LoroValue::Container`.
    pub(super) fn mergeable_children_from_value(
        &self,
        parent_id: &ContainerID,
        map_value: &LoroMapValue,
    ) -> Vec<(InternalString, ContainerID)> {
        let mut ans = Vec::new();
        for (key, value) in map_value.iter() {
            if let Some(kind) = loro_common::parse_mergeable_marker(parent_id, key, value) {
                let key_istr: InternalString = key.as_str().into();
                let cid = ContainerID::new_mergeable(parent_id, key, kind);
                ans.push((key_istr, cid));
            }
        }
        ans
    }
}

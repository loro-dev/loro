//! Mergeable-container bookkeeping on `DocState`.
//!
//! Mergeable child containers (created via `MapHandler::get_mergeable_*`) live as deterministic
//! `ContainerID::Root` ids in a reserved namespace. Their *visibility* is driven entirely by the
//! `"🤝:<kind>"` discriminator string the parent map stores at the key (see loro-dev/loro#759 and
//! [`DocState::mergeable_children_from_value`]): whichever discriminator the parent map's regular
//! LWW resolves to picks the active kind, exactly as a regular child container's value-table entry
//! picks the active child.
//!
//! The only out-of-band state is the parent-edge index (`child_containers` on [`MapState`]), which
//! lets path resolution and reachability map a mergeable cid back to its key. This module keeps
//! that index in sync with the discriminators: it is seeded on import (snapshot + update) and
//! updated per-op as discriminators appear or are cleared.
//!
//! [`MapState`]: super::map_state::MapState
//! [`DocState::mergeable_children_from_value`]: super::DocState::mergeable_children_from_value

use loro_common::{ContainerID, ContainerType, InternalString, LoroMapValue, LoroValue};
use rustc_hash::FxHashSet;

use crate::{
    container::{idx::ContainerIdx, map::MapSet},
    event::InternalContainerDiff,
    op::{Op, RawOp, RawOpContent},
    OpLog,
};

use super::DocState;

impl DocState {
    /// Walk all map containers and rebuild the mergeable parent-edge index from the discriminators
    /// stored in their value tables.
    ///
    /// Discriminators ride through snapshots like any other map value, so no special recovery is
    /// needed for the mergeable state itself; this walk only repopulates the in-memory
    /// `child_containers` parent edges (which are not serialized) so that path resolution,
    /// reachability, and child enumeration resolve imported mergeable cids without requiring the
    /// caller to re-invoke `get_mergeable_*`.
    ///
    /// Called from [`Self::init_with_states_and_version`] right after a snapshot decode.
    pub(super) fn repopulate_mergeable_child_side_tables(&mut self, _oplog: &OpLog) {
        let map_cids: Vec<ContainerID> = self
            .store
            .iter_all_container_ids()
            .filter(|id| matches!(id.container_type(), ContainerType::Map))
            .collect();

        for parent_id in map_cids {
            self.register_mergeable_edges_of_map(&parent_id);
        }
    }

    /// Reconcile the mergeable parent edges for a single map container against its discriminators.
    ///
    /// Registers an edge for every key whose value is an active discriminator and evicts any
    /// stale mergeable edge whose key no longer carries a matching discriminator (e.g. a key that
    /// was deleted or whose kind changed). Idempotent.
    fn register_mergeable_edges_of_map(&mut self, parent_id: &ContainerID) {
        let Some(parent_idx) = self.arena.id_to_idx(parent_id) else {
            return;
        };
        // Derive the active children from the map's value table through the lazy `get_value`
        // path. Reading the decoded `MapState` here would force every map to decode its full
        // state on import (including plain maps with no mergeable children), defeating the
        // snapshot lazy-load and leaving roots with materialized state. Discriminators ride
        // through the value table, so the value alone is sufficient (loro-dev/loro#759).
        let active: Vec<(InternalString, ContainerID)> =
            self.active_mergeable_children_lazy(parent_idx, parent_id);
        let active_set: FxHashSet<ContainerID> =
            active.iter().map(|(_, cid)| cid.clone()).collect();

        // Evict edges that are no longer backed by a matching discriminator. The parent-edge
        // index (`child_containers`) only exists on an already-decoded `MapState`; a map that is
        // still lazy has no registered edges, so there is nothing to evict and we must not force a
        // decode just to discover that. Only inspect the side table when the state is already
        // decoded.
        let stale: Vec<ContainerID> = if self.store.has_decoded_state(parent_idx) {
            self.store
                .get_container(parent_idx)
                .and_then(|state| state.as_map_state())
                .map(|map_state| {
                    map_state
                        .iter_mergeable_children()
                        .filter(|(_, cid)| !active_set.contains(*cid))
                        .map(|(_, cid)| cid.clone())
                        .collect()
                })
                .unwrap_or_default()
        } else {
            Vec::new()
        };

        if !stale.is_empty() {
            if let Some(map) = self
                .store
                .get_container_mut(parent_idx)
                .and_then(|state| state.as_map_state_mut())
            {
                for cid in &stale {
                    map.evict_mergeable_child_cid(cid);
                }
            }
            self.dead_containers_cache.clear_alive();
        }

        for (key, cid) in active {
            // Wire the arena parent edge (idempotent) so the cid resolves in path / reachability
            // walks, then record the parent-edge index entry on the MapState.
            self.store.ensure_container(parent_id);
            self.arena.register_container(parent_id);
            self.arena.register_container(&cid);
            if let Some(state) = self.store.get_container_mut(parent_idx) {
                if let Some(map) = state.as_map_state_mut() {
                    map.register_mergeable_child(key, cid);
                }
            }
        }
    }

    /// Register mergeable parent edges for the maps touched by an update-import diff batch.
    ///
    /// The discriminator ops in the batch already updated each parent map's value table; this
    /// walk reads those discriminators and registers the corresponding parent edges. Shared body
    /// with the snapshot recovery walk, scoped to the batch's parent maps so update-import cost
    /// stays proportional to the diff size.
    pub(super) fn register_mergeable_children(
        &mut self,
        parent_map_ids: impl IntoIterator<Item = ContainerID>,
    ) {
        for parent_id in parent_map_ids {
            self.register_mergeable_edges_of_map(&parent_id);
        }
    }

    /// Derive `(key, cid)` pairs for the active mergeable children of a map directly from its
    /// already-computed value table, without forcing the map's container state to decode.
    ///
    /// This is the read-time source of truth for mergeable resolution (loro-dev/loro#759),
    /// exactly mirroring how a regular child container's reachability is the parent's value slot:
    /// for each key whose value is a recognized `"🤝:<kind>"` discriminator, the active child is
    /// the deterministic cid `ContainerID::new_mergeable(parent_id, key, kind)`. Whichever
    /// discriminator the parent map's regular LWW resolved to picks the kind, so:
    ///
    /// - concurrent same-kind creation writes the identical discriminator (LWW no-op) and both
    ///   peers' contributions land in the one deterministic cid;
    /// - concurrent different-kind creation lets Map LWW deterministically pick one kind, while
    ///   the loser's cid stays reachable only by an explicit `get_mergeable_<kind>` lookup (which
    ///   rewrites the discriminator);
    /// - `delete(key)` overwrites the discriminator with `None`, so the child becomes unreachable
    ///   like any deleted container; a later `get_mergeable_<kind>(key)` rewrites the discriminator
    ///   and brings it back, with Map LWW resolving any concurrent delete-vs-recreate race.
    ///
    /// Used by the deep-value walk (`get_container_deep_value` /
    /// `get_container_deep_value_with_id`) to nest mergeable child values under their logical
    /// parent key (overwriting the raw discriminator string the walk would otherwise emit), by
    /// `get_alive_children_of`, and — via [`Self::active_mergeable_children_lazy`] — by the
    /// import-time side-table walk.
    ///
    /// Deriving from the value rather than the decoded `MapState` is what keeps snapshot-backed
    /// roots lazy: the value is obtained through the lazy `ContainerStore::get_value` path, so a
    /// plain map with no discriminators is never forced to materialize its full state.
    ///
    /// `parent_id` is the cid of the map whose value is `map_value`; the caller supplies it
    /// because `MapState` does not store its own id.
    pub(super) fn mergeable_children_from_value(
        &self,
        parent_id: &ContainerID,
        map_value: &LoroMapValue,
    ) -> Vec<(InternalString, ContainerID)> {
        let mut ans = Vec::new();
        for (key, value) in map_value.iter() {
            if let Some(kind) = loro_common::parse_mergeable_discriminator(value) {
                let key_istr: InternalString = key.as_str().into();
                let cid = ContainerID::new_mergeable(parent_id, key, kind);
                ans.push((key_istr, cid));
            }
        }
        ans
    }

    /// Like [`Self::mergeable_children_from_value`], but fetches the map's value by container idx
    /// through the lazy `ContainerStore::get_value` path so a snapshot-backed map is not forced to
    /// decode its full state. Returns an empty Vec if the idx is not a map or its value is absent.
    pub(super) fn active_mergeable_children_lazy(
        &mut self,
        parent_idx: ContainerIdx,
        parent_id: &ContainerID,
    ) -> Vec<(InternalString, ContainerID)> {
        match self.store.get_value(parent_idx) {
            Some(LoroValue::Map(map_value)) => {
                self.mergeable_children_from_value(parent_id, &map_value)
            }
            _ => Vec::new(),
        }
    }

    /// Capture the container idxs touched by a diff batch so the post-loop hook can register
    /// mergeable parent edges for whichever of them turn out to be maps.
    ///
    /// We capture ALL touched idxs rather than filtering to maps here: a fresh receiver importing
    /// the first discriminator op for a map that does not yet exist in the store would otherwise
    /// be filtered out (the map state is only created when the diff applies). The post-loop hook
    /// re-checks each idx against the now-populated store.
    pub(super) fn capture_mergeable_diff_batch(
        &mut self,
        diffs: &[InternalContainerDiff],
    ) -> FxHashSet<ContainerIdx> {
        diffs.iter().map(|d| d.idx).collect()
    }

    /// Keep the mergeable parent-edge index in sync with a just-applied `MapSet` op on a parent
    /// map.
    ///
    /// A `MapSet` writing a `"🤝:<kind>"` discriminator realizes (or re-realizes) a mergeable
    /// child: register its deterministic cid's parent edge. Any other value at that key — `None`
    /// (a delete) or a plain value that overwrote a discriminator — clears the child: evict the
    /// stale mergeable edge so reachability and path walks no longer surface it.
    ///
    /// This is the per-op counterpart of [`Self::register_mergeable_children`]; together they keep
    /// the in-memory index consistent with the discriminators that are the source of truth.
    pub(super) fn sync_mergeable_side_table_for_op(&mut self, raw_op: &RawOp, op: &Op) {
        let RawOpContent::Map(MapSet { key, value }) = &raw_op.content else {
            return;
        };
        let Some(parent_id) = self.arena.idx_to_id(op.container) else {
            return;
        };
        let key_istr: InternalString = key.clone();

        let new_kind = value
            .as_ref()
            .and_then(loro_common::parse_mergeable_discriminator);

        // Evict any stale mergeable edge under this key whose kind no longer matches what the
        // discriminator (if any) now selects. This covers deletes (new_kind == None) and
        // kind-change overwrites (different discriminator at the same key).
        let stale: Vec<ContainerID> = self
            .store
            .get_container(op.container)
            .and_then(|state| state.as_map_state())
            .map(|map_state| {
                map_state
                    .iter_mergeable_children()
                    .filter(|(k, cid)| **k == key_istr && Some(cid.container_type()) != new_kind)
                    .map(|(_, cid)| cid.clone())
                    .collect()
            })
            .unwrap_or_default();
        if !stale.is_empty() {
            if let Some(map) = self
                .store
                .get_container_mut(op.container)
                .and_then(|state| state.as_map_state_mut())
            {
                for cid in &stale {
                    map.evict_mergeable_child_cid(cid);
                }
            }
            self.dead_containers_cache.clear_alive();
        }

        // Register the parent edge for the kind the discriminator now selects.
        if let Some(kind) = new_kind {
            let cid = ContainerID::new_mergeable(&parent_id, key, kind);
            self.arena.register_container(&cid);
            if let Some(map) = self
                .store
                .get_container_mut(op.container)
                .and_then(|state| state.as_map_state_mut())
            {
                map.register_mergeable_child(key_istr, cid);
            }
        }
    }
}

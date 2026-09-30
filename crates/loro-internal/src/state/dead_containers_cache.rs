use super::DocState;
use crate::container::idx::ContainerIdx;
use loro_common::ContainerType;
use rustc_hash::FxHashSet;
use smallvec::SmallVec;

/// Containers known to be deleted. See `context/dead-container-cache.md`.
#[derive(Default, Debug, Clone)]
pub(super) struct DeadContainersCache {
    /// Removed from a Map or List: no later version gives them back.
    final_deletions: FxHashSet<ContainerIdx>,
    /// Cut off at a Tree or MovableList edge: a later move can revive them.
    revivable: FxHashSet<ContainerIdx>,
}

impl DeadContainersCache {
    /// Must be called whenever the state may move to a version that is not a
    /// descendant of the current one (checkout, reset), because that can
    /// revive any container.
    pub fn clear(&mut self) {
        self.final_deletions.clear();
        self.clear_revivable();
    }

    /// Must be called whenever a tree or movable-list change is applied going
    /// forward, because it may move a deleted node or element back.
    pub fn clear_revivable(&mut self) {
        // Reusing a large table makes interleaved deletion queries and moves
        // repeatedly clear its peak capacity, even with only one live entry.
        self.revivable = FxHashSet::default();
    }

    fn contains(&self, idx: &ContainerIdx) -> bool {
        self.final_deletions.contains(idx) || self.revivable.contains(idx)
    }
}

impl DocState {
    /// Called when a failed import is rolled back. The import may have registered parent links
    /// that the arena rollback drops, and a deletion found through one of them while it ran
    /// (e.g. by a query on another thread) is not true of the kept history.
    /// See `context/failed-import-arena-indices.md`.
    pub(crate) fn forget_parent_link_caches_after_failed_import(&mut self) {
        self.dead_containers_cache.clear();
    }

    pub(crate) fn is_deleted(&mut self, idx: ContainerIdx) -> bool {
        #[cfg(not(debug_assertions))]
        {
            if self.dead_containers_cache.contains(&idx) {
                return true;
            }
        }

        // Parent chains are shallow (depth 1 for a root container), so inline
        // storage avoids a heap allocation on this per-op check.
        let mut visited: SmallVec<[ContainerIdx; 4]> = SmallVec::new();
        visited.push(idx);
        let mut idx = idx;
        let mut depends_on_mergeable_edge = false;
        // The parent that no longer holds the chain, if any.
        let (is_deleted, cut_at) = loop {
            let id = self.arena.idx_to_id(idx).unwrap();
            if id.is_mergeable() {
                depends_on_mergeable_edge = true;
            }
            if let Some(parent_idx) = self.arena.get_parent(idx) {
                if !self.contains_logical_child(parent_idx, &id) {
                    break (true, Some(parent_idx));
                }

                idx = parent_idx;
                visited.push(idx);
            } else {
                // No parent in the arena: top-level Roots are always alive; anything else
                // (including a mergeable Root whose parent edge was never wired) is treated
                // as deleted. A later op can still attach it, so this is never cached.
                break (!id.is_root() || id.is_mergeable(), None);
            }
        };

        // Every container on the walked chain shares the answer: below the
        // missing edge all of them are deleted, otherwise all are alive.
        #[cfg(debug_assertions)]
        for idx in visited.iter() {
            assert!(
                is_deleted || !self.dead_containers_cache.contains(idx),
                "stale dead-container cache entry for {:?}",
                self.arena.idx_to_id(*idx)
            );
        }

        // A mergeable ancestor can be deleted and later reactivated by changing the parent map's
        // marker. Do not cache deletion for any descendant whose liveness depends on that
        // logical edge, including ordinary children nested inside a mergeable map.
        if is_deleted && !depends_on_mergeable_edge {
            match cut_at.map(|parent| parent.get_type()) {
                Some(ContainerType::Map | ContainerType::List) => {
                    self.dead_containers_cache.final_deletions.extend(visited)
                }
                Some(ContainerType::Tree | ContainerType::MovableList) => {
                    self.dead_containers_cache.revivable.extend(visited)
                }
                _ => {}
            }
        }

        is_deleted
    }

    #[cfg(test)]
    pub(crate) fn dead_cache_entry(&self, idx: ContainerIdx) -> Option<bool> {
        self.dead_containers_cache.contains(&idx).then_some(true)
    }
}

#[cfg(test)]
#[cfg(feature = "counter")]
mod tests {
    use loro_common::ContainerID;

    use crate::{cursor::PosType, HandlerTrait, LoroDoc, TextHandler};

    /// A mergeable child can be deleted and then reactivated: `delete(key)` clears its
    /// marker (child unreachable), and a later `ensure_mergeable_<kind>(key)` writes the
    /// marker back (child reachable again). While the child is unreachable, querying its
    /// liveness must not cache a `deleted` entry because that answer depends on the mutable
    /// mergeable marker edge.
    ///
    /// The scenario:
    /// 1. Create the mergeable counter and capture its container idx.
    /// 2. Delete the key, then query `is_deleted()`.
    /// 3. Re-get the counter to rewrite the marker and reactivate the child.
    /// 4. Assert the cache never held a `deleted` entry for that idx.
    ///
    /// It asserts the cache contents directly because `is_deleted()` only trusts the cache via a
    /// release-only early return; inspecting the cache makes stale-cache regressions fail in both
    /// debug and release builds.
    #[test]
    fn reactivated_mergeable_child_has_no_stale_dead_cache_entry() {
        let doc = LoroDoc::new_auto_commit();
        doc.set_peer_id(1).unwrap();
        let root = doc.get_map("state");
        let counter = root.ensure_mergeable_counter("revision").unwrap();
        counter.increment(1.0).unwrap();
        doc.commit_then_renew();

        let cid: ContainerID = counter.id();
        let idx = doc.state.lock().arena.id_to_idx(&cid).unwrap();

        root.delete("revision").unwrap();
        doc.commit_then_renew();
        assert!(counter.is_deleted());
        assert_eq!(
            doc.state.lock().dead_cache_entry(idx),
            None,
            "mergeable-dependent deletion must not be cached"
        );

        root.ensure_mergeable_counter("revision").unwrap();
        doc.commit_then_renew();
        assert_eq!(
            doc.state.lock().dead_cache_entry(idx),
            None,
            "reactivation must drop the stale deleted-cache entry"
        );
    }

    /// The no-cache rule also applies to ordinary descendants under a mergeable ancestor. A
    /// regular child container can look cache-safe by cid shape, but its liveness still depends on
    /// the ancestor's marker edge.
    #[test]
    fn ordinary_child_under_reactivated_mergeable_map_has_no_stale_dead_cache_entry() {
        let doc = LoroDoc::new_auto_commit();
        doc.set_peer_id(1).unwrap();
        let root = doc.get_map("state");
        let profile = root.ensure_mergeable_map("profile").unwrap();
        let text = profile
            .insert_container("bio", TextHandler::new_detached())
            .unwrap();
        text.insert(0, "Ada", PosType::Unicode).unwrap();
        doc.commit_then_renew();

        let text_id: ContainerID = text.id();
        let text_idx = doc.state.lock().arena.id_to_idx(&text_id).unwrap();

        root.delete("profile").unwrap();
        doc.commit_then_renew();
        assert!(text.is_deleted());
        assert_eq!(
            doc.state.lock().dead_cache_entry(text_idx),
            None,
            "ordinary descendants behind a mergeable edge must not be cached as deleted"
        );

        root.ensure_mergeable_map("profile").unwrap();
        doc.commit_then_renew();
        assert!(!text.is_deleted());
        assert_eq!(
            doc.state.lock().dead_cache_entry(text_idx),
            None,
            "reactivated ordinary descendant must not leave a stale cache entry"
        );
    }

    /// The same stale-cache hazard exists when reactivation arrives from a *peer* via import,
    /// not just from a local `ensure_mergeable_*` call. The importing peer must not cache the
    /// mergeable-dependent deleted result before the reactivation update arrives.
    ///
    /// The scenario, with two peers A (author) and B (importer):
    /// 1. A creates the mergeable counter and deletes the key, then exports.
    /// 2. B imports A's updates so the child exists but is unreachable, and queries
    ///    `is_deleted()`.
    /// 3. A re-gets the counter (rewriting the marker, reactivating the child) and
    ///    exports just that new update.
    /// 4. B imports the reactivation update.
    /// 5. Assert B's cache never held a `deleted` entry for that idx.
    ///
    /// Like the local-reactivation test, this asserts cache contents directly rather than
    /// through `is_deleted()`, because `is_deleted()` only trusts the cache via a release-only
    /// early return; a public-API assertion would pass in debug even with the bug present.
    #[test]
    fn imported_mergeable_child_reactivation_clears_dead_cache() {
        use crate::loro::ExportMode;

        let doc_a = LoroDoc::new_auto_commit();
        doc_a.set_peer_id(1).unwrap();
        let root_a = doc_a.get_map("state");
        let counter_a = root_a.ensure_mergeable_counter("revision").unwrap();
        counter_a.increment(1.0).unwrap();
        doc_a.commit_then_renew();
        root_a.delete("revision").unwrap();
        doc_a.commit_then_renew();

        let cid: ContainerID = counter_a.id();

        // B imports A's history: the child exists but is unreachable (marker cleared).
        let doc_b = LoroDoc::new_auto_commit();
        doc_b.set_peer_id(2).unwrap();
        doc_b
            .import(&doc_a.export(ExportMode::all_updates()).unwrap())
            .unwrap();

        let idx = doc_b.state.lock().arena.id_to_idx(&cid).unwrap();
        assert!(doc_b.state.lock().is_deleted(idx));
        assert_eq!(
            doc_b.state.lock().dead_cache_entry(idx),
            None,
            "imported mergeable-dependent deletion must not be cached"
        );

        // A reactivates the child locally and exports just the new update.
        let vv_before = doc_a.oplog_vv();
        root_a.ensure_mergeable_counter("revision").unwrap();
        doc_a.commit_then_renew();
        let reactivation = doc_a.export(ExportMode::updates(&vv_before)).unwrap();

        // B imports the reactivation. There must be no stale deleted-cache entry left to mask it.
        doc_b.import(&reactivation).unwrap();
        assert_eq!(
            doc_b.state.lock().dead_cache_entry(idx),
            None,
            "imported reactivation must drop the stale deleted-cache entry"
        );
    }

    /// A tree node under a deleted ancestor is revived by moving it out, so a
    /// local move must drop the cached deletion of its metadata.
    #[test]
    fn local_tree_move_drops_revivable_deletions() {
        use crate::state::TreeParentId;

        let doc = LoroDoc::new_auto_commit();
        doc.set_peer_id(1).unwrap();
        let tree = doc.get_tree("tree");
        let parent = tree.create(TreeParentId::Root).unwrap();
        let child = tree.create(TreeParentId::Node(parent)).unwrap();
        let meta = tree.get_meta(child).unwrap();
        doc.commit_then_renew();
        tree.delete(parent).unwrap();
        doc.commit_then_renew();

        let idx = doc.state.lock().arena.id_to_idx(&meta.id()).unwrap();
        assert!(meta.is_deleted());
        assert_eq!(doc.state.lock().dead_cache_entry(idx), Some(true));

        tree.mov(child, TreeParentId::Root).unwrap();
        assert_eq!(doc.state.lock().dead_cache_entry(idx), None);
        doc.commit_then_renew();
        assert!(!meta.is_deleted());
    }

    /// A movable-list element deleted locally is revived by a concurrent move
    /// with a greater lamport, so importing movable-list changes must drop the
    /// cached deletion.
    #[test]
    fn movable_list_import_drops_revivable_deletions() {
        use crate::{loro::ExportMode, MapHandler};

        let doc = LoroDoc::new_auto_commit();
        doc.set_peer_id(1).unwrap();
        let list = doc.get_movable_list("list");
        list.insert(0, 0).unwrap();
        let child = list
            .insert_container(1, MapHandler::new_detached())
            .unwrap();
        doc.commit_then_renew();
        let other = LoroDoc::new_auto_commit();
        other.set_peer_id(2).unwrap();
        other
            .import(&doc.export(ExportMode::all_updates()).unwrap())
            .unwrap();
        other.get_map("pad").insert("k", 1).unwrap();
        other.commit_then_renew();
        other.get_movable_list("list").mov(1, 0).unwrap();
        other.commit_then_renew();

        list.delete(1, 1).unwrap();
        doc.commit_then_renew();
        let idx = doc.state.lock().arena.id_to_idx(&child.id()).unwrap();
        assert!(child.is_deleted());
        assert_eq!(doc.state.lock().dead_cache_entry(idx), Some(true));

        doc.import(&other.export(ExportMode::all_updates()).unwrap())
            .unwrap();
        assert_eq!(doc.state.lock().dead_cache_entry(idx), None);
        assert!(!child.is_deleted());
    }

    /// Map and list removals are final for every later version, so they are
    /// cached for the child and everything below it.
    #[test]
    fn map_and_list_child_deletions_are_cached() {
        use crate::{ListHandler, MapHandler};

        let doc = LoroDoc::new_auto_commit();
        doc.set_peer_id(1).unwrap();
        let map = doc.get_map("map");
        let in_map = map
            .insert_container("k", MapHandler::new_detached())
            .unwrap();
        let nested = in_map
            .insert_container("t", TextHandler::new_detached())
            .unwrap();
        let list = doc.get_list("list");
        let in_list = list
            .insert_container(0, ListHandler::new_detached())
            .unwrap();
        doc.commit_then_renew();
        map.delete("k").unwrap();
        list.delete(0, 1).unwrap();
        doc.commit_then_renew();

        assert!(nested.is_deleted());
        assert!(in_list.is_deleted());
        let state = doc.state.lock();
        for id in [in_map.id(), nested.id(), in_list.id()] {
            let idx = state.arena.id_to_idx(&id).unwrap();
            assert_eq!(state.dead_cache_entry(idx), Some(true), "{id:?}");
        }
    }

    /// While checked out to an older version, a map child created later is
    /// cut at its map parent and cached as a final deletion. A forward
    /// checkout is not a `DiffMode::Checkout` transition, but it must still
    /// drop that entry.
    #[test]
    fn forward_checkout_drops_deletions_cached_behind_the_oplog() {
        use crate::MapHandler;

        let doc = LoroDoc::new_auto_commit();
        doc.set_peer_id(1).unwrap();
        let map = doc.get_map("m");
        map.insert("a", 1).unwrap();
        doc.commit_then_renew();
        let v1 = doc.oplog_frontiers();
        let child = map
            .insert_container("c", MapHandler::new_detached())
            .unwrap();
        doc.commit_then_renew();

        doc.checkout(&v1).unwrap();
        let idx = doc.state.lock().arena.id_to_idx(&child.id()).unwrap();
        assert!(child.is_deleted());
        assert_eq!(doc.state.lock().dead_cache_entry(idx), Some(true));

        doc.checkout_to_latest();
        assert_eq!(doc.state.lock().dead_cache_entry(idx), None);
        assert!(!child.is_deleted());
    }
}

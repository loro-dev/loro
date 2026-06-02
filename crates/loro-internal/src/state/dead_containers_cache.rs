use super::{ContainerState, DocState};
use crate::container::idx::ContainerIdx;
use rustc_hash::FxHashMap;

#[derive(Default, Debug, Clone)]
pub(super) struct DeadContainersCache {
    cache: FxHashMap<ContainerIdx, bool>,
}

impl DeadContainersCache {
    pub fn clear(&mut self) {
        self.cache.clear();
    }

    pub(crate) fn clear_alive(&mut self) {
        self.cache.retain(|_, is_deleted| *is_deleted);
    }

    pub(crate) fn remove(&mut self, idx: ContainerIdx) {
        self.cache.remove(&idx);
    }
}

impl DocState {
    pub(crate) fn is_deleted(&mut self, idx: ContainerIdx) -> bool {
        #[cfg(not(debug_assertions))]
        {
            // Cache stores only deleted containers.
            if self.dead_containers_cache.cache.contains_key(&idx) {
                return true;
            }
        }

        let mut visited = vec![idx];
        let mut idx = idx;
        let is_deleted = loop {
            let id = self.arena.idx_to_id(idx).unwrap();
            if let Some(parent_idx) = self.arena.get_parent(idx) {
                let Some(parent_state) = self.store.get_container_mut(parent_idx) else {
                    break true;
                };
                if !parent_state.contains_child(&id) {
                    // The parent has no edge to this child. For a mergeable child this means its
                    // discriminator is no longer active at the key (it was deleted or its kind
                    // changed), so the child is unreachable — exactly like a regular container
                    // whose value slot was overwritten. Re-`get_mergeable_<kind>` rewrites the
                    // discriminator and brings it back.
                    break true;
                }

                idx = parent_idx;
                visited.push(idx);
            } else {
                // No parent in the arena: top-level Roots are always alive; anything else
                // (including a mergeable Root whose parent edge was never wired) is treated
                // as deleted.
                break !id.is_root() || id.is_mergeable();
            }
        };

        #[cfg(debug_assertions)]
        {
            if let Some(cached_is_deleted) = self.dead_containers_cache.cache.get(&idx) {
                assert_eq!(is_deleted, *cached_is_deleted);
            }
        }

        if is_deleted {
            for idx in visited {
                self.dead_containers_cache.cache.insert(idx, true);
            }
        } else {
            for idx in visited {
                self.dead_containers_cache.cache.remove(&idx);
            }
        }

        is_deleted
    }

    #[cfg(test)]
    pub(crate) fn dead_cache_entry(&self, idx: ContainerIdx) -> Option<bool> {
        self.dead_containers_cache.cache.get(&idx).copied()
    }
}

#[cfg(test)]
#[cfg(feature = "counter")]
mod tests {
    use loro_common::ContainerID;

    use crate::{HandlerTrait, LoroDoc};

    /// A mergeable child can be deleted and then reactivated: `delete(key)` clears its
    /// discriminator (child unreachable), and a later `get_mergeable_<kind>(key)` writes the
    /// discriminator back (child reachable again). While the child is unreachable, querying its
    /// liveness records it in `dead_containers_cache` as deleted. This test verifies that
    /// reactivation removes that entry, so the cache cannot keep reporting the resurrected child
    /// as deleted.
    ///
    /// The scenario:
    /// 1. Create the mergeable counter and capture its container idx.
    /// 2. Delete the key, then query `is_deleted()` to poison the cache with a `deleted` entry.
    /// 3. Re-get the counter to rewrite the discriminator and reactivate the child.
    /// 4. Assert the cache no longer holds a `deleted` entry for that idx.
    ///
    /// It asserts the cache contents directly rather than going through `is_deleted()`, because
    /// `is_deleted()` only trusts the cache via a release-only early return; a public-API
    /// assertion would pass in debug even with the bug present. Inspecting the cache makes the
    /// regression fail in both debug and release builds.
    #[test]
    fn reactivated_mergeable_child_has_no_stale_dead_cache_entry() {
        let doc = LoroDoc::new_auto_commit();
        doc.set_peer_id(1).unwrap();
        let root = doc.get_map("state");
        let counter = root.get_mergeable_counter("revision").unwrap();
        counter.increment(1.0).unwrap();
        doc.commit_then_renew();

        let cid: ContainerID = counter.id();
        let idx = doc.state.lock().arena.id_to_idx(&cid).unwrap();

        root.delete("revision").unwrap();
        doc.commit_then_renew();
        // Poison the cache: querying the unreachable child records it as deleted.
        assert!(counter.is_deleted());
        assert_eq!(
            doc.state.lock().dead_cache_entry(idx),
            Some(true),
            "delete must record the child as deleted in the cache"
        );

        root.get_mergeable_counter("revision").unwrap();
        doc.commit_then_renew();
        assert_eq!(
            doc.state.lock().dead_cache_entry(idx),
            None,
            "reactivation must drop the stale deleted-cache entry"
        );
    }

    /// The same stale-cache hazard exists when reactivation arrives from a *peer* via import,
    /// not just from a local `get_mergeable_*` call. The update-import side-table rebuild
    /// (`register_mergeable_children` -> `register_mergeable_edges_of_map`) re-registers the
    /// active child, and must also clear the importing peer's poisoned deleted-cache entry.
    ///
    /// The scenario, with two peers A (author) and B (importer):
    /// 1. A creates the mergeable counter and deletes the key, then exports.
    /// 2. B imports A's updates so the child exists but is unreachable, and queries
    ///    `is_deleted()` to poison B's `dead_containers_cache` with a `deleted` entry.
    /// 3. A re-gets the counter (rewriting the discriminator, reactivating the child) and
    ///    exports just that new update.
    /// 4. B imports the reactivation update; the import path re-registers the parent edge.
    /// 5. Assert B's cache no longer holds a `deleted` entry for that idx.
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
        let counter_a = root_a.get_mergeable_counter("revision").unwrap();
        counter_a.increment(1.0).unwrap();
        doc_a.commit_then_renew();
        root_a.delete("revision").unwrap();
        doc_a.commit_then_renew();

        let cid: ContainerID = counter_a.id();

        // B imports A's history: the child exists but is unreachable (discriminator cleared).
        let doc_b = LoroDoc::new_auto_commit();
        doc_b.set_peer_id(2).unwrap();
        doc_b
            .import(&doc_a.export(ExportMode::all_updates()).unwrap())
            .unwrap();

        let idx = doc_b.state.lock().arena.id_to_idx(&cid).unwrap();
        // Poison B's cache: querying the unreachable child records it as deleted.
        assert!(doc_b.state.lock().is_deleted(idx));
        assert_eq!(
            doc_b.state.lock().dead_cache_entry(idx),
            Some(true),
            "import of a deleted child must record it as deleted in the cache"
        );

        // A reactivates the child locally and exports just the new update.
        let vv_before = doc_a.oplog_vv();
        root_a.get_mergeable_counter("revision").unwrap();
        doc_a.commit_then_renew();
        let reactivation = doc_a.export(ExportMode::updates(&vv_before)).unwrap();

        // B imports the reactivation: the import path re-registers the parent edge and must
        // drop the stale deleted-cache entry.
        doc_b.import(&reactivation).unwrap();
        assert_eq!(
            doc_b.state.lock().dead_cache_entry(idx),
            None,
            "imported reactivation must drop the stale deleted-cache entry"
        );
    }
}

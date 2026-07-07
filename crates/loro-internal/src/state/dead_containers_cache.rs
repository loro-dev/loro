use super::DocState;
use crate::container::idx::ContainerIdx;
use rustc_hash::FxHashMap;
use smallvec::SmallVec;

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

        // Parent chains are shallow (depth 1 for a root container), so inline
        // storage avoids a heap allocation on this per-op check.
        let mut visited: SmallVec<[ContainerIdx; 4]> = SmallVec::new();
        visited.push(idx);
        let mut idx = idx;
        let mut depends_on_mergeable_edge = false;
        let is_deleted = loop {
            let id = self.arena.idx_to_id(idx).unwrap();
            if id.is_mergeable() {
                depends_on_mergeable_edge = true;
            }
            if let Some(parent_idx) = self.arena.get_parent(idx) {
                if !self.contains_logical_child(parent_idx, &id) {
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
            if !depends_on_mergeable_edge {
                if let Some(cached_is_deleted) = self.dead_containers_cache.cache.get(&idx) {
                    assert_eq!(is_deleted, *cached_is_deleted);
                }
            }
        }

        // A mergeable ancestor can be deleted and later reactivated by changing the parent map's
        // marker. Do not cache deletion for any descendant whose liveness depends on that
        // logical edge, including ordinary children nested inside a mergeable map.
        if depends_on_mergeable_edge {
            if !is_deleted {
                for idx in visited {
                    self.dead_containers_cache.cache.remove(&idx);
                }
            }
            return is_deleted;
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
}

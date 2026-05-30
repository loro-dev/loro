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
}

use super::{ContainerState, DocState};
use crate::container::idx::ContainerIdx;
use fxhash::FxHashMap;

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
            if let Some(is_deleted) = self.dead_containers_cache.cache.get(&idx) {
                return *is_deleted;
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
                    break true;
                }

                idx = parent_idx;
                visited.push(idx);
            } else {
                break !id.is_root();
            }
        };

        #[cfg(debug_assertions)]
        {
            if let Some(cached_is_deleted) = self.dead_containers_cache.cache.get(&idx) {
                assert_eq!(is_deleted, *cached_is_deleted);
            }
        }

        for idx in visited {
            self.dead_containers_cache.cache.insert(idx, is_deleted);
        }

        is_deleted
    }
}

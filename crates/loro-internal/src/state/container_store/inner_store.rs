use crate::{
    arena::SharedArena,
    configure::Configure,
    container::idx::ContainerIdx,
    state::{container_store::FRONTIERS_KEY, ContainerCreationContext},
    utils::kv_wrapper::KvWrapper,
    version::Frontiers,
};
use bytes::Bytes;
use loro_common::ContainerID;
use std::ops::Bound;

use super::ContainerWrapper;

/// The invariants about this struct:
///
/// - `kv` is either the same or older than `store`.
/// - if `load_state` is `AllLoaded`, then `store` contains all the entries from `kv`
///
/// Invariants: it should be agnostic to the users of this struct whether a container is stored in `kv` or `store`
pub(crate) struct InnerStore {
    arena: SharedArena,
    store: Vec<Option<ContainerWrapper>>,
    kv: KvWrapper,
    load_state: LoadState,
    config: Configure,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LoadState {
    Lazy,
    RootsLoaded,
    AllLoaded,
}

impl std::fmt::Debug for InnerStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InnerStore").finish()
    }
}

/// This impl block contains all the mutation code that may break the invariants of this struct
impl InnerStore {
    #[inline]
    fn slot(idx: ContainerIdx) -> usize {
        idx.to_index() as usize
    }

    #[inline]
    fn get_entry_mut_in(
        store: &mut [Option<ContainerWrapper>],
        idx: ContainerIdx,
    ) -> Option<&mut ContainerWrapper> {
        let entry = store.get_mut(Self::slot(idx))?.as_mut()?;
        debug_assert_eq!(entry.kind(), idx.get_type());
        Some(entry)
    }

    #[inline]
    fn get_entry_mut(&mut self, idx: ContainerIdx) -> Option<&mut ContainerWrapper> {
        Self::get_entry_mut_in(&mut self.store, idx)
    }

    #[inline]
    fn contains_idx_in(store: &[Option<ContainerWrapper>], idx: ContainerIdx) -> bool {
        store
            .get(Self::slot(idx))
            .and_then(|entry| entry.as_ref())
            .is_some_and(|entry| entry.kind() == idx.get_type())
    }

    #[inline]
    fn contains_idx(&self, idx: ContainerIdx) -> bool {
        Self::contains_idx_in(&self.store, idx)
    }

    fn insert_entry(
        store: &mut Vec<Option<ContainerWrapper>>,
        idx: ContainerIdx,
        container: ContainerWrapper,
    ) -> Option<ContainerWrapper> {
        let slot = Self::slot(idx);
        if store.len() <= slot {
            store.resize_with(slot + 1, || None);
        }

        store[slot].replace(container)
    }

    pub(super) fn get_or_insert_with(
        &mut self,
        idx: ContainerIdx,
        f: impl FnOnce() -> ContainerWrapper,
    ) -> &mut ContainerWrapper {
        if self.get_entry_mut(idx).is_none() {
            let id = self.arena.get_container_id(idx).unwrap();
            let key = id.to_bytes();
            let container = if self.load_state != LoadState::AllLoaded {
                self.kv
                    .get(&key)
                    .map(ContainerWrapper::new_from_bytes)
                    .unwrap_or_else(f)
            } else {
                f()
            };
            Self::insert_entry(&mut self.store, idx, container);
        }

        self.get_entry_mut(idx).unwrap()
    }

    pub(super) fn ensure_container(
        &mut self,
        idx: ContainerIdx,
        f: impl FnOnce() -> ContainerWrapper,
    ) {
        if self.contains_idx(idx) {
            return;
        }

        if self.load_state != LoadState::AllLoaded {
            let id = self.arena.get_container_id(idx).unwrap();
            let key = id.to_bytes();
            if let Some(v) = self.kv.get(&key) {
                let c = ContainerWrapper::new_from_bytes(v);
                Self::insert_entry(&mut self.store, idx, c);
                return;
            }
        }

        let c = f();
        Self::insert_entry(&mut self.store, idx, c);
    }

    pub(crate) fn get_mut(&mut self, idx: ContainerIdx) -> Option<&mut ContainerWrapper> {
        if self.get_entry_mut(idx).is_none() && self.load_state != LoadState::AllLoaded {
            let id = self.arena.get_container_id(idx).unwrap();
            let key = id.to_bytes();
            if let Some(v) = self.kv.get(&key) {
                let c = ContainerWrapper::new_from_bytes(v);
                Self::insert_entry(&mut self.store, idx, c);
            }
        }

        self.get_entry_mut(idx)
    }

    pub(crate) fn with_container_for_read<R>(
        &mut self,
        idx: ContainerIdx,
        f: impl FnOnce(&mut ContainerWrapper) -> R,
    ) -> Option<R> {
        if let Some(entry) = self.get_entry_mut(idx) {
            return Some(f(entry));
        }

        if self.load_state != LoadState::AllLoaded {
            let id = self.arena.get_container_id(idx).unwrap();
            let key = id.to_bytes();
            if let Some(v) = self.kv.get(&key) {
                let mut container = ContainerWrapper::new_from_bytes(v);
                let ans = f(&mut container);
                if container.has_cached_value() {
                    Self::insert_entry(&mut self.store, idx, container);
                }
                return Some(ans);
            }
        }

        None
    }

    pub(crate) fn has_decoded_state(&mut self, idx: ContainerIdx) -> bool {
        self.get_entry_mut(idx)
            .is_some_and(|entry| entry.try_get_state().is_some())
    }

    pub(crate) fn contains_id(&mut self, id: &ContainerID) -> bool {
        if let Some(idx) = self.arena.id_to_idx(id) {
            if self.contains_idx(idx) {
                return true;
            }
        }

        if self.load_state != LoadState::AllLoaded {
            let key = id.to_bytes();
            return self.kv.contains_key(&key);
        }

        false
    }

    pub(crate) fn iter_all_containers_mut(
        &mut self,
    ) -> impl Iterator<Item = (ContainerIdx, &mut ContainerWrapper)> {
        self.load_all();
        self.store
            .iter_mut()
            .enumerate()
            .filter_map(|(slot, entry)| {
                entry.as_mut().map(|container| {
                    (
                        ContainerIdx::from_index_and_type(slot as u32, container.kind()),
                        container,
                    )
                })
            })
    }

    pub(crate) fn iter_all_container_ids(&mut self) -> impl Iterator<Item = ContainerID> + '_ {
        // PERF: we don't need to load all the containers here
        self.load_all();
        self.store.iter().enumerate().filter_map(|(slot, entry)| {
            entry.as_ref().map(|container| {
                let idx = ContainerIdx::from_index_and_type(slot as u32, container.kind());
                self.arena.get_container_id(idx).unwrap()
            })
        })
    }

    pub(crate) fn encode(&mut self) -> Bytes {
        self.flush();
        self.kv.export()
    }

    pub(crate) fn flush(&mut self) {
        let deleted = self.config.deleted_root_containers.lock();
        let mut updates = Vec::new();
        let mut deleted_roots = Vec::new();

        for (slot, entry) in self.store.iter_mut().enumerate() {
            let Some(c) = entry.as_mut() else {
                continue;
            };
            let idx = ContainerIdx::from_index_and_type(slot as u32, c.kind());
            let cid = self.arena.get_container_id(idx).unwrap();
            if cid.is_root() && deleted.contains(&cid) && c.is_deleted_root_value_cleared() {
                deleted_roots.push(cid.to_bytes());
                c.set_flushed(true);
                continue;
            }

            if c.is_flushed() {
                continue;
            }

            let cid: Bytes = cid.to_bytes().into();
            let value = c.encode();
            c.set_flushed(true);
            updates.push((cid, value));
        }

        drop(deleted);
        for cid in deleted_roots {
            self.kv.remove(&cid);
        }
        self.kv.set_all(updates.into_iter());
    }

    pub(crate) fn get_kv_clone(&self) -> KvWrapper {
        self.kv.clone()
    }

    pub(crate) fn decode(
        &mut self,
        bytes: bytes::Bytes,
        ctx: ContainerCreationContext,
    ) -> Result<Option<Frontiers>, loro_common::LoroError> {
        assert!(self.kv.is_empty());
        let mut fr = None;
        self.kv
            .import(bytes)
            .map_err(|e| loro_common::LoroError::DecodeError(e.into_boxed_str()))?;
        if let Some(f) = self.kv.remove(FRONTIERS_KEY) {
            fr = Some(Frontiers::decode(&f)?);
        }
        self.validate_lazy_entries(ctx)?;

        let kv = self.kv.arc_clone();
        self.arena
            .set_parent_resolver(Some(move |child_id: ContainerID| {
                let k = child_id.to_bytes();
                let v = kv.get(&k)?;
                let c = ContainerWrapper::new_from_bytes(v);
                c.parent().cloned()
            }));

        self.store.clear();
        self.load_state = LoadState::Lazy;
        Ok(fr)
    }

    pub(crate) fn decode_twice(
        &mut self,
        bytes_a: bytes::Bytes,
        bytes_b: bytes::Bytes,
        ctx: ContainerCreationContext,
    ) -> Result<(), loro_common::LoroError> {
        assert!(self.kv.is_empty());
        // TODO: add assert that all containers in the store should be empty right now
        self.kv
            .import(bytes_a)
            .map_err(|e| loro_common::LoroError::DecodeError(e.into_boxed_str()))?;
        self.kv
            .import(bytes_b)
            .map_err(|e| loro_common::LoroError::DecodeError(e.into_boxed_str()))?;
        self.kv.remove(FRONTIERS_KEY);
        self.validate_lazy_entries(ctx)?;
        let store = &mut self.store;
        let arena = &self.arena;
        self.kv.with_kv(|kv| {
            arena.with_guards(|guards| {
                let iter = kv.scan(Bound::Unbounded, Bound::Unbounded);
                for (k, v) in iter {
                    let cid = ContainerID::from_bytes(&k);
                    let c = ContainerWrapper::new_from_bytes(v);
                    let parent = c.parent();
                    let idx = guards.register_container(&cid);
                    let p = parent.as_ref().map(|p| guards.register_container(p));
                    guards.set_parent(idx, p);
                    if Self::insert_entry(store, idx, c).is_some() {}
                }
            });
        });

        self.load_state = LoadState::AllLoaded;
        Ok(())
    }

    fn validate_lazy_entries(
        &self,
        ctx: ContainerCreationContext,
    ) -> Result<(), loro_common::LoroError> {
        self.kv.with_kv(|kv| {
            for (k, v) in kv.scan(Bound::Unbounded, Bound::Unbounded) {
                let cid = ContainerID::from_bytes(&k);
                let idx = self.arena.register_container(&cid);
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let mut container = ContainerWrapper::new_from_bytes(v);
                    container.decode_state(idx, ctx)
                }));
                match result {
                    Ok(Ok(())) => {}
                    Ok(Err(e)) => return Err(e),
                    Err(_) => {
                        return Err(loro_common::LoroError::DecodeError(
                            "Decode container state failed".to_string().into_boxed_str(),
                        ));
                    }
                }
            }

            Ok(())
        })
    }

    pub fn load_all(&mut self) {
        if self.load_state == LoadState::AllLoaded {
            return;
        }

        let store = &mut self.store;
        let arena = &self.arena;
        self.kv.with_kv(|kv| {
            let iter = kv.scan(Bound::Unbounded, Bound::Unbounded);
            arena.with_guards(|guards| {
                for (k, v) in iter {
                    let cid = ContainerID::from_bytes(&k);
                    let idx = guards.register_container(&cid);
                    if Self::contains_idx_in(store, idx) {
                        // the container is already loaded
                        // the content in `store` is guaranteed to be newer than the content in `kv`
                        continue;
                    }

                    let container = ContainerWrapper::new_from_bytes(v);
                    Self::insert_entry(store, idx, container);
                }
            });
        });

        self.load_state = LoadState::AllLoaded;
    }

    pub fn load_roots(&mut self) {
        if self.load_state != LoadState::Lazy {
            return;
        }

        let arena = &self.arena;
        self.kv.with_kv(|kv| {
            let iter = kv.scan(Bound::Unbounded, Bound::Unbounded);
            arena.with_guards(|guards| {
                for (k, _) in iter {
                    let cid = ContainerID::from_bytes(&k);
                    if cid.is_root() {
                        guards.register_container(&cid);
                    }
                }
            });
        });
        self.load_state = LoadState::RootsLoaded;
    }

    pub(crate) fn can_import_snapshot(&self) -> bool {
        if !self.kv.is_empty() {
            return false;
        }

        self.store
            .iter()
            .filter_map(|entry| entry.as_ref())
            .all(|c| c.is_state_empty())
    }

    #[cfg(test)]
    pub(super) fn has_cached_value_for_test(&mut self, idx: ContainerIdx) -> bool {
        self.get_entry_mut(idx)
            .is_some_and(|entry| entry.has_cached_value_for_test())
    }
}

impl InnerStore {
    pub(crate) fn new(arena: SharedArena, config: Configure) -> Self {
        Self {
            arena,
            store: Vec::new(),
            kv: KvWrapper::new_mem(),
            load_state: LoadState::AllLoaded,
            config,
        }
    }

    pub(crate) fn fork(&mut self, arena: SharedArena, config: &Configure) -> InnerStore {
        // PERF: we can try to reuse
        let bytes = self.encode();
        let mut new_store = Self::new(arena, config.clone());
        new_store
            .decode(
                bytes,
                ContainerCreationContext {
                    configure: config,
                    peer: 0,
                },
            )
            .unwrap();
        new_store
    }
}

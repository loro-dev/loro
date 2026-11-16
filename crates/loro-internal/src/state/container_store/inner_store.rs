use crate::{
    arena::SharedArena,
    configure::Configure,
    container::idx::ContainerIdx,
    kv_store::{KvStore, SharedBlockCache},
    state::{container_store::FRONTIERS_KEY, ContainerCreationContext, ContainerState},
    sync::Mutex,
    utils::kv_wrapper::KvWrapper,
    version::Frontiers,
};
use bytes::Bytes;
use loro_common::{ContainerID, ContainerType, LoroResult, LoroValue};
use rustc_hash::FxHashSet;
use std::{collections::VecDeque, sync::Arc};

use super::ContainerWrapper;

/// The invariants about this struct:
///
/// - `kv` is either the same or older than `store`.
/// - `store` is a cache over `kv`: every container in `kv` is either present in
///   `store` or was evicted from it by the bounded decoded-value cache
///   (see `Configure::decoded_value_cache_limit`). Evicted entries are re-created from
///   `kv` on the next access, so lookups must always fall back to `kv` on a
///   `store` miss, regardless of `load_state`.
/// - `load_state == AllLoaded` means every `kv` entry was materialized into
///   `store` at some point; it does NOT mean `store` is still complete. Once
///   `evicted_since_full_load` is set, `load_all()` must re-scan `kv`.
///
/// Invariants: it should be agnostic to the users of this struct whether a container is stored in `kv` or `store`
pub(crate) struct InnerStore {
    arena: SharedArena,
    store: Vec<Option<ContainerWrapper>>,
    kv: KvWrapper,
    load_state: LoadState,
    config: Configure,
    /// FIFO (with a second-chance bit on each wrapper) of containers whose
    /// decoded value is currently cached in `store`. Bounded by
    /// `Configure::decoded_value_cache_limit()`. Entries may be stale (the wrapper was
    /// dropped or materialized into a full state); eviction skips those.
    /// Cached value queue bounded by `decoded_value_cache_limit()` and
    /// `decoded_value_cache_bytes_limit()`.
    value_cache_queue: VecDeque<ContainerIdx>,
    /// Approximate heap bytes currently held by wrappers tracked in `value_cache_queue`.
    value_cache_bytes: usize,
    /// True once the value cache or cold-state demotion has evicted at least one
    /// wrapper since the last full KV scan. While this is set, `load_state == AllLoaded`
    /// no longer implies `store` contains every entry in `kv`, so `load_all()`
    /// must re-scan `kv` instead of short-circuiting.
    evicted_since_full_load: bool,
    /// Clock hand for two-pass cold state demotion (`demote_cold_states`).
    demote_clock_hand: usize,
    /// Whether `self.arena` already has a KV parent resolver pointing at `self.kv`.
    parent_resolver_installed: bool,
    /// Stack of active created-container journals so nested rollback scopes
    /// can roll back independently or merge into an enclosing scope on success.
    created_journal_stack: Vec<Vec<ContainerIdx>>,
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
            let container = match self.kv.get(&key) {
                Some(bytes) => ContainerWrapper::new_from_bytes(bytes),
                None => {
                    if let Some(journal) = self.created_journal_stack.last_mut() {
                        journal.push(idx);
                    }
                    f()
                }
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

        let id = self.arena.get_container_id(idx).unwrap();
        let key = id.to_bytes();
        if let Some(v) = self.kv.get(&key) {
            let c = ContainerWrapper::new_from_bytes(v);
            Self::insert_entry(&mut self.store, idx, c);
            return;
        }

        if let Some(journal) = self.created_journal_stack.last_mut() {
            journal.push(idx);
        }
        let c = f();
        Self::insert_entry(&mut self.store, idx, c);
    }

    /// Start recording the containers that get new state.
    pub(crate) fn begin_created_journal(&mut self) {
        self.created_journal_stack.push(Vec::new());
    }

    /// Stop recording the innermost journal and return the containers that got
    /// new state since its [`Self::begin_created_journal`].
    pub(crate) fn take_created_journal(&mut self) -> Vec<ContainerIdx> {
        self.created_journal_stack.pop().unwrap_or_default()
    }

    /// Commit the innermost journal without rolling it back; when nested inside
    /// an outer rollback scope, merge its created containers into the parent scope.
    pub(crate) fn commit_top_created_journal(&mut self) {
        let popped = self.created_journal_stack.pop().unwrap_or_default();
        if let Some(parent) = self.created_journal_stack.last_mut() {
            parent.extend(popped);
        }
    }

    pub(crate) fn clear_created_journals(&mut self) {
        self.created_journal_stack.clear();
    }

    /// Drop the state of containers that got new state while a rolled back
    /// local transaction ran. They had no state and no KV entry before it (the
    /// KV store is only written by `flush`), so this restores the store.
    pub(crate) fn remove_created(&mut self, created: &[ContainerIdx]) {
        for &idx in created {
            debug_assert!(self
                .arena
                .get_container_id(idx)
                .is_none_or(|id| self.kv.get(&id.to_bytes()).is_none()));
            if let Some(slot) = self.store.get_mut(Self::slot(idx)) {
                if let Some(mut removed) = slot.take() {
                    self.value_cache_bytes = self
                        .value_cache_bytes
                        .saturating_sub(removed.take_accounted_value_cache_bytes());
                }
            }
        }
        if !created.is_empty() {
            while self.store.last().is_some_and(Option::is_none) {
                self.store.pop();
            }
            if self.store.is_empty() {
                self.demote_clock_hand = 0;
                self.store.shrink_to_fit();
            } else {
                self.demote_clock_hand %= self.store.len();
                if self.store.capacity() > self.store.len().saturating_mul(2).max(4) {
                    self.store.shrink_to_fit();
                }
            }
        }
    }

    pub(crate) fn get_mut(&mut self, idx: ContainerIdx) -> Option<&mut ContainerWrapper> {
        if self.get_entry_mut(idx).is_none() {
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
            let ans = f(entry);
            self.track_value_cache(idx);
            return Some(ans);
        }

        let id = self.arena.get_container_id(idx).unwrap();
        let key = id.to_bytes();
        if let Some(v) = self.kv.get(&key) {
            let mut container = ContainerWrapper::new_from_bytes(v);
            let ans = f(&mut container);
            if container.has_cached_value() {
                Self::insert_entry(&mut self.store, idx, container);
                self.track_value_cache(idx);
            }
            return Some(ans);
        }

        None
    }

    /// Track a wrapper whose read may have populated the decoded-value cache,
    /// and evict the oldest cached values once the cache exceeds
    /// the configured limit (2048 by default).
    ///
    /// Eviction drops the whole wrapper from `store`; the wrapper is a pure
    /// cache over the KV bytes (`ContainerWrapper::is_evictable_cached_value`
    /// requires a flushed lazy wrapper), so the next read re-creates it from
    /// the KV store.
    fn track_value_cache(&mut self, idx: ContainerIdx) {
        let Some((old_bytes, new_bytes, need_enqueue)) = ({
            let Some(entry) = self.get_entry_mut(idx) else {
                return;
            };
            if !entry.is_evictable_cached_value() {
                Some((entry.take_accounted_value_cache_bytes(), 0, false))
            } else {
                let new_bytes = entry.estimated_cached_value_bytes();
                let old_bytes = entry.replace_accounted_value_cache_bytes(new_bytes);
                let need_enqueue = !entry.is_in_value_cache_queue();
                if need_enqueue {
                    entry.set_in_value_cache_queue(true);
                }
                entry.mark_value_cache_referenced();
                Some((old_bytes, new_bytes, need_enqueue))
            }
        }) else {
            return;
        };

        self.value_cache_bytes = self
            .value_cache_bytes
            .saturating_sub(old_bytes)
            .saturating_add(new_bytes);

        if need_enqueue {
            self.value_cache_queue.push_back(idx);
        }
        if new_bytes > 0 {
            self.trim_value_cache();
        }
    }

    /// Second-chance FIFO: hot ancestors survive an eviction pass during deep
    /// walks. Only flushed lazy wrappers are evictable; dirty state stays pinned.
    pub(super) fn trim_value_cache(&mut self) {
        let count_limit = self.config.decoded_value_cache_limit();
        let bytes_limit = self.config.decoded_value_cache_bytes_limit();
        self.trim_value_cache_with_limits(count_limit, bytes_limit);
    }

    pub(crate) fn evict_value_cache_to_bytes(&mut self, target_bytes: usize) -> usize {
        let before = self.value_cache_bytes;
        let count_limit = if target_bytes == 0 {
            0
        } else {
            self.config.decoded_value_cache_limit()
        };
        self.trim_value_cache_with_limits(count_limit, target_bytes);
        before.saturating_sub(self.value_cache_bytes)
    }

    fn trim_value_cache_with_limits(&mut self, count_limit: usize, bytes_limit: usize) {
        while self.value_cache_queue.len() > count_limit
            || (self.value_cache_bytes > bytes_limit && !self.value_cache_queue.is_empty())
        {
            let Some(victim) = self.value_cache_queue.pop_front() else {
                break;
            };
            enum Action {
                Skip,
                Requeue,
                Evict,
            }
            let mut dropped_bytes = 0usize;
            let action = match self.get_entry_mut(victim) {
                Some(entry) if entry.is_evictable_cached_value() => {
                    if entry.take_value_cache_referenced() {
                        Action::Requeue
                    } else {
                        entry.set_in_value_cache_queue(false);
                        Action::Evict
                    }
                }
                // Stale entry: the wrapper was dropped or materialized into a
                // full state since it was enqueued.
                Some(entry) => {
                    entry.set_in_value_cache_queue(false);
                    dropped_bytes = entry.take_accounted_value_cache_bytes();
                    Action::Skip
                }
                None => Action::Skip,
            };
            self.value_cache_bytes = self.value_cache_bytes.saturating_sub(dropped_bytes);
            match action {
                Action::Skip => {}
                Action::Requeue => self.value_cache_queue.push_back(victim),
                Action::Evict => {
                    let slot = Self::slot(victim);
                    if let Some(entry) = self.store.get_mut(slot) {
                        if let Some(mut removed) = entry.take() {
                            self.value_cache_bytes = self
                                .value_cache_bytes
                                .saturating_sub(removed.take_accounted_value_cache_bytes());
                        }
                    }
                    // `store` may no longer mirror `kv`: a full enumeration has
                    // to re-scan `kv` even in `AllLoaded` mode.
                    self.evicted_since_full_load = true;
                }
            }
        }
    }

    /// Read a container without retaining a wrapper loaded only for this read.
    pub(crate) fn try_with_container_for_ephemeral_read<R>(
        &mut self,
        idx: ContainerIdx,
        f: impl FnOnce(&mut ContainerWrapper) -> R,
    ) -> LoroResult<Option<R>> {
        if let Some(entry) = self.get_entry_mut(idx) {
            return Ok(Some(f(entry)));
        }

        let id = self.arena.get_container_id(idx).unwrap();
        let key = id.to_bytes();
        if let Some(v) = self.kv.get(&key) {
            let mut container = ContainerWrapper::try_new_from_bytes(v)?;
            return Ok(Some(f(&mut container)));
        }

        Ok(None)
    }

    /// Unlike a handler read, this does not load KV bytes, enqueue a wrapper,
    /// mark a cache reference, or materialize a LoroValue.
    pub(super) fn resident_map_is_empty(&self, idx: ContainerIdx) -> Option<bool> {
        if idx.get_type() != ContainerType::Map {
            return None;
        }
        let entry = self.store.get(Self::slot(idx))?.as_ref()?;
        debug_assert_eq!(entry.kind(), idx.get_type());
        entry.resident_map_is_empty()
    }

    /// Read a container's current value without retaining a wrapper loaded only for this read.
    ///
    /// Mirrors [`Self::try_get_parent_and_value_ephemeral`]: routing the kv branch through
    /// `ContainerWrapper::try_get_value_ephemeral` would clone the bytes into a second temporary
    /// wrapper, so decode the already-temporary wrapper in place instead.
    pub(crate) fn try_get_value_ephemeral(
        &mut self,
        idx: ContainerIdx,
        ctx: ContainerCreationContext<'_>,
    ) -> LoroResult<Option<LoroValue>> {
        if let Some(entry) = self.get_entry_mut(idx) {
            return entry.try_get_value_ephemeral(idx, ctx).map(Some);
        }

        let id = self.arena.get_container_id(idx).unwrap();
        let key = id.to_bytes();
        if let Some(bytes) = self.kv.get(&key) {
            let mut container = ContainerWrapper::try_new_from_bytes(bytes)?;
            return container.try_get_value(idx, ctx).map(Some);
        }

        Ok(None)
    }

    /// Read a container's encoded parent and current value together without retaining a wrapper
    /// loaded only for this read.
    ///
    /// The alive-container walk needs both fields. Reading them through two independent probes can
    /// evict the first SSTable block before the value is requested, and it constructs the same lazy
    /// wrapper twice. Decode an uncached wrapper in place here and drop it after returning the value.
    pub(crate) fn try_get_parent_and_value_ephemeral(
        &mut self,
        idx: ContainerIdx,
        ctx: ContainerCreationContext<'_>,
    ) -> LoroResult<Option<(Option<ContainerID>, LoroValue)>> {
        if let Some(entry) = self.get_entry_mut(idx) {
            let parent = entry.parent().cloned();
            let value = entry.try_get_value_ephemeral(idx, ctx)?;
            return Ok(Some((parent, value)));
        }

        let id = self.arena.get_container_id(idx).unwrap();
        let key = id.to_bytes();
        if let Some(value) = self.kv.get(&key) {
            let mut container = ContainerWrapper::try_new_from_bytes(value)?;
            let parent = container.parent().cloned();
            // This wrapper is already temporary, so decoding into it does not retain state in
            // the document and avoids constructing another temporary wrapper internally.
            let value = container.try_get_value(idx, ctx)?;
            return Ok(Some((parent, value)));
        }

        Ok(None)
    }

    /// Tree counterpart of [`Self::try_get_parent_and_value_ephemeral`]: the encoded parent and
    /// the meta map ids of every node, deleted nodes included.
    pub(crate) fn try_get_parent_and_tree_meta_ids_ephemeral(
        &mut self,
        idx: ContainerIdx,
        ctx: ContainerCreationContext<'_>,
    ) -> LoroResult<Option<(Option<ContainerID>, Vec<ContainerID>)>> {
        if let Some(entry) = self.get_entry_mut(idx) {
            let parent = entry.parent().cloned();
            let ids = entry.try_get_tree_meta_ids_ephemeral(idx, ctx)?;
            return Ok(Some((parent, ids)));
        }

        let id = self.arena.get_container_id(idx).unwrap();
        let key = id.to_bytes();
        if let Some(value) = self.kv.get(&key) {
            let mut container = ContainerWrapper::try_new_from_bytes(value)?;
            let parent = container.parent().cloned();
            container.decode_state(idx, ctx)?;
            let ids = container.try_get_state().unwrap().get_child_containers();
            return Ok(Some((parent, ids)));
        }

        Ok(None)
    }

    /// Read the parent encoded in a container wrapper without retaining a wrapper loaded only
    /// for this probe.
    ///
    /// The outer `Option` distinguishes a missing wrapper from a wrapper whose encoded parent is
    /// `None`.
    pub(crate) fn get_parent_ephemeral(
        &mut self,
        idx: ContainerIdx,
    ) -> LoroResult<Option<Option<ContainerID>>> {
        self.try_with_container_for_ephemeral_read(idx, |container| container.parent().cloned())
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

        let key = id.to_bytes();
        self.kv.contains_key(&key)
    }

    /// Tree and MovableList containers, found without decoding any other
    /// container. A kv key starts with the container type byte, with the high
    /// bit set for root ids (see `ContainerID::encode`), so each type is a key range.
    pub(crate) fn tree_and_movable_list_idxs(&mut self) -> Vec<ContainerIdx> {
        let mut keys: Vec<Bytes> = Vec::new();
        for ty in [ContainerType::Tree, ContainerType::MovableList] {
            for root_bit in [0u8, 0b1000_0000] {
                let lo = ty.to_u8() | root_bit;
                keys.extend(
                    self.kv
                        .scan_range_entries(&[lo], &[lo + 1])
                        .into_iter()
                        .map(|(k, _)| k),
                );
            }
        }
        let mut out: FxHashSet<ContainerIdx> = FxHashSet::default();
        self.arena.with_guards(|guards| {
            for k in keys {
                out.insert(guards.register_container(&ContainerID::from_bytes(&k)));
            }
        });
        // Loaded entries can be newer than kv or absent from it.
        for (slot, entry) in self.store.iter().enumerate() {
            if let Some(c) = entry {
                if matches!(c.kind(), ContainerType::Tree | ContainerType::MovableList) {
                    out.insert(ContainerIdx::from_index_and_type(slot as u32, c.kind()));
                }
            }
        }
        out.into_iter().collect()
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
        self.kv.set_all(updates);
    }

    pub(crate) fn get_kv_clone(&self) -> KvWrapper {
        self.kv.clone()
    }

    pub(crate) fn decode(
        &mut self,
        bytes: bytes::Bytes,
    ) -> Result<Option<Frontiers>, loro_common::LoroError> {
        assert!(self.kv.is_empty());
        let mut fr = None;
        self.kv
            .import(bytes)
            .map_err(|e| loro_common::LoroError::DecodeError(e.into_boxed_str()))?;
        if let Some(f) = self.kv.remove(FRONTIERS_KEY) {
            fr = Some(Frontiers::decode(&f)?);
        }

        self.ensure_parent_resolver_installed();

        self.store.clear();
        self.value_cache_queue.clear();
        self.value_cache_bytes = 0;
        self.evicted_since_full_load = false;
        self.demote_clock_hand = 0;
        self.load_state = LoadState::Lazy;
        Ok(fr)
    }

    pub(crate) fn decode_twice(
        &mut self,
        bytes_a: bytes::Bytes,
        bytes_b: bytes::Bytes,
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

        self.ensure_parent_resolver_installed();

        self.store.clear();
        self.value_cache_queue.clear();
        self.value_cache_bytes = 0;
        self.evicted_since_full_load = false;
        self.demote_clock_hand = 0;
        self.load_state = LoadState::Lazy;
        Ok(())
    }

    pub fn load_all(&mut self) {
        if self.load_state == LoadState::AllLoaded && !self.evicted_since_full_load {
            return;
        }

        let entries = self.kv.scan_all_entries();
        let store = &mut self.store;
        let arena = &self.arena;
        arena.with_guards(|guards| {
            for (k, v) in entries {
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

        self.evicted_since_full_load = false;
        self.load_state = LoadState::AllLoaded;
    }

    pub fn load_roots(&mut self) {
        if self.load_state != LoadState::Lazy {
            return;
        }

        let arena = &self.arena;
        let keys = self.kv.scan_all_keys();
        arena.with_guards(|guards| {
            for k in keys {
                let cid = ContainerID::from_bytes(&k);
                if cid.is_root() {
                    guards.register_container(&cid);
                }
            }
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

    /// Number of wrappers currently holding an evictable cached decoded value.
    /// Must stay bounded by the configured limit no matter how many
    /// containers have been read (loro-dev/loro#1092).
    #[cfg(test)]
    pub(super) fn cached_value_count_for_test(&self) -> usize {
        self.store
            .iter()
            .flatten()
            .filter(|entry| entry.is_evictable_cached_value())
            .count()
    }

    #[cfg(test)]
    pub(super) fn has_materialized_map_value_for_test(&mut self, idx: ContainerIdx) -> bool {
        self.get_entry_mut(idx)
            .is_some_and(|entry| entry.has_materialized_map_value_for_test())
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
            value_cache_queue: VecDeque::new(),
            value_cache_bytes: 0,
            evicted_since_full_load: false,
            demote_clock_hand: 0,
            parent_resolver_installed: false,
            created_journal_stack: Vec::new(),
        }
    }

    pub(crate) fn value_cache_bytes(&self) -> usize {
        self.value_cache_bytes
    }

    pub(crate) fn estimated_state_bytes(&self) -> usize {
        self.store
            .iter()
            .filter_map(|entry| entry.as_ref())
            .filter(|c| c.is_materialized_state())
            .map(|c| c.estimated_state_bytes())
            .sum::<usize>()
            .saturating_add(self.store.capacity() * std::mem::size_of::<Option<ContainerWrapper>>())
    }

    pub(crate) fn kv_estimated_bytes(&self) -> usize {
        self.kv.estimated_memory_bytes()
    }

    pub(crate) fn spilled_disk_bytes(&self) -> usize {
        self.kv.spilled_disk_bytes()
    }

    pub(crate) fn set_block_cache(&mut self, cache: SharedBlockCache) {
        self.kv.set_block_cache(cache);
    }

    pub(crate) fn flush_kv(&mut self) {
        self.kv.flush_compressed();
    }

    pub(crate) fn replace_kv_backend(&mut self, new_backend: Arc<Mutex<dyn KvStore>>) {
        self.kv.replace_backend(new_backend);
        self.parent_resolver_installed = false;
        self.ensure_parent_resolver_installed();
    }

    fn ensure_parent_resolver_installed(&mut self) {
        if self.parent_resolver_installed {
            return;
        }
        let kv = self.kv.arc_clone();
        self.arena
            .set_parent_resolver(Some(move |child_id: ContainerID| {
                let k = child_id.to_bytes();
                let v = kv.get(&k)?;
                let c = ContainerWrapper::new_from_bytes(v);
                c.parent().cloned()
            }));
        self.parent_resolver_installed = true;
    }

    pub(crate) fn reclaim_auxiliary_caches(&mut self) -> usize {
        let mut freed = 0usize;
        for c in self.store.iter_mut().flatten() {
            if c.is_materialized_state() {
                let before = c.estimated_state_bytes();
                c.reclaim_auxiliary_caches();
                let after = c.estimated_state_bytes();
                freed = freed.saturating_add(before.saturating_sub(after));
            }
        }
        freed
    }

    /// Demote cold materialized container states to `self.kv` using a two-pass
    /// clock sweep until materialized state bytes drop to `target_state_bytes`.
    pub(crate) fn demote_cold_states(
        &mut self,
        target_state_bytes: usize,
        pinned: &FxHashSet<ContainerIdx>,
    ) -> usize {
        let mut cur_bytes = self.estimated_state_bytes();
        if cur_bytes <= target_state_bytes || self.store.is_empty() {
            return 0;
        }

        self.ensure_parent_resolver_installed();
        let journal_set: FxHashSet<ContainerIdx> = self
            .created_journal_stack
            .iter()
            .flat_map(|v| v.iter().copied())
            .collect();
        let store_len = self.store.len();
        let mut reclaimed = 0usize;
        let mut updates: Vec<(Bytes, Bytes)> = Vec::new();
        let deleted = self.config.deleted_root_containers.lock();

        for pass in 0..2 {
            for step in 0..store_len {
                if cur_bytes <= target_state_bytes {
                    break;
                }
                let slot = (self.demote_clock_hand + step) % store_len;
                let Some(c) = self.store[slot].as_mut() else {
                    continue;
                };
                if !c.is_materialized_state() {
                    continue;
                }
                let before_aux = c.estimated_state_bytes();
                c.reclaim_auxiliary_caches();
                let after_aux = c.estimated_state_bytes();
                let aux_freed = before_aux.saturating_sub(after_aux);
                if aux_freed > 0 {
                    reclaimed = reclaimed.saturating_add(aux_freed);
                    cur_bytes = cur_bytes.saturating_sub(aux_freed);
                    if cur_bytes <= target_state_bytes {
                        self.demote_clock_hand = (slot + 1) % store_len;
                        break;
                    }
                }

                let idx = ContainerIdx::from_index_and_type(slot as u32, c.kind());
                if pinned.contains(&idx) || journal_set.contains(&idx) || c.is_state_empty() {
                    continue;
                }
                if pass == 0 && c.take_state_referenced() {
                    continue;
                }
                let Some(cid) = self.arena.get_container_id(idx) else {
                    continue;
                };
                if cid.is_root() && deleted.contains(&cid) && c.is_deleted_root_value_cleared() {
                    continue;
                }

                let entry_bytes = c.estimated_state_bytes();
                if !c.is_flushed() {
                    let value = c.encode();
                    updates.push((cid.to_bytes().into(), value));
                }
                if let Some(mut old) = self.store[slot].take() {
                    self.value_cache_bytes = self
                        .value_cache_bytes
                        .saturating_sub(old.take_accounted_value_cache_bytes());
                }
                self.evicted_since_full_load = true;
                reclaimed = reclaimed.saturating_add(entry_bytes);
                cur_bytes = cur_bytes.saturating_sub(entry_bytes);
                self.demote_clock_hand = (slot + 1) % store_len;
            }
            if cur_bytes <= target_state_bytes {
                break;
            }
        }
        drop(deleted);

        if !updates.is_empty() {
            self.kv.set_all(updates);
        }
        reclaimed
    }

    pub(crate) fn fork(&mut self, arena: SharedArena, config: &Configure) -> InnerStore {
        // PERF: we can try to reuse
        let bytes = self.encode();
        let mut new_store = Self::new(arena, config.clone());
        new_store.decode(bytes).unwrap();
        new_store
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use loro_common::{ContainerType, ID};
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };

    fn encoded_container_header(kind: ContainerType, parent: Option<ContainerID>) -> Bytes {
        let mut output = Vec::new();
        output.push(kind.to_u8());
        leb128::write::unsigned(&mut output, 2).unwrap();
        postcard::to_io(&parent, &mut output).unwrap();
        output.into()
    }

    fn mergeable_child_entry() -> (ContainerID, Bytes) {
        let parent_id = ContainerID::new_normal(ID::new(1, 0), ContainerType::Map);
        let child_id = ContainerID::new_mergeable(&parent_id, "field", ContainerType::Text);
        let value = encoded_container_header(ContainerType::Text, Some(parent_id));
        (child_id, value)
    }

    fn install_parent_resolver_guard(
        arena: &SharedArena,
        loading: Arc<AtomicBool>,
    ) -> Arc<AtomicBool> {
        let resolver_called = Arc::new(AtomicBool::new(false));
        let called = resolver_called.clone();
        arena.set_parent_resolver(Some(move |_child_id: ContainerID| {
            assert!(
                !loading.load(Ordering::SeqCst),
                "lazy parent resolver must not run while loading from KV"
            );
            called.store(true, Ordering::SeqCst);
            None
        }));
        resolver_called
    }

    fn kv_bytes_with_entry(cid: ContainerID, value: Bytes) -> Bytes {
        let kv = KvWrapper::new_mem();
        kv.set_all(vec![(Bytes::from(cid.to_bytes()), value)]);
        kv.export()
    }

    #[test]
    fn load_all_does_not_resolve_parent_while_loading_from_kv() {
        let arena = SharedArena::new();
        let mut store = InnerStore::new(arena.clone(), Configure::default());
        let loading = Arc::new(AtomicBool::new(false));
        let resolver_called = install_parent_resolver_guard(&arena, loading.clone());
        let (child_id, value) = mergeable_child_entry();
        let child_id_for_depth = child_id.clone();
        store.load_state = LoadState::Lazy;
        store
            .kv
            .set_all(vec![(Bytes::from(child_id.to_bytes()), value)]);

        loading.store(true, Ordering::SeqCst);
        store.load_all();
        loading.store(false, Ordering::SeqCst);
        assert!(!resolver_called.load(Ordering::SeqCst));

        let child_idx = arena.id_to_idx(&child_id_for_depth).unwrap();
        let _ = arena.get_depth(child_idx);
        assert!(
            resolver_called.load(Ordering::SeqCst),
            "lazy parent resolver should still work after load_all returns"
        );
    }

    #[test]
    fn load_roots_does_not_resolve_parent_while_loading_from_kv() {
        let arena = SharedArena::new();
        let mut store = InnerStore::new(arena.clone(), Configure::default());
        let loading = Arc::new(AtomicBool::new(false));
        let resolver_called = install_parent_resolver_guard(&arena, loading.clone());
        let (child_id, value) = mergeable_child_entry();
        let child_id_for_depth = child_id.clone();
        store.load_state = LoadState::Lazy;
        store
            .kv
            .set_all(vec![(Bytes::from(child_id.to_bytes()), value)]);

        loading.store(true, Ordering::SeqCst);
        store.load_roots();
        loading.store(false, Ordering::SeqCst);
        assert!(!resolver_called.load(Ordering::SeqCst));

        let child_idx = arena.id_to_idx(&child_id_for_depth).unwrap();
        let _ = arena.get_depth(child_idx);
        assert!(
            resolver_called.load(Ordering::SeqCst),
            "lazy parent resolver should still work after load_roots returns"
        );
    }

    #[test]
    fn decode_twice_does_not_resolve_parent_while_loading_from_kv() {
        let arena = SharedArena::new();
        let mut store = InnerStore::new(arena.clone(), Configure::default());
        let loading = Arc::new(AtomicBool::new(false));
        let resolver_called = install_parent_resolver_guard(&arena, loading.clone());
        let root_id = ContainerID::new_root("root", ContainerType::Map);
        let child_id = ContainerID::new_normal(ID::new(1, 0), ContainerType::Text);
        let value = encoded_container_header(ContainerType::Text, Some(root_id.clone()));
        let bytes = kv_bytes_with_entry(child_id.clone(), value);

        loading.store(true, Ordering::SeqCst);
        store.decode_twice(bytes, Bytes::new()).unwrap();
        loading.store(false, Ordering::SeqCst);
        assert!(!resolver_called.load(Ordering::SeqCst));
        assert_eq!(store.load_state, LoadState::Lazy);
        assert!(
            arena.id_to_idx(&child_id).is_none(),
            "decode_twice should keep containers lazy until accessed"
        );

        let child_idx = arena.register_container(&child_id);
        let root_idx = arena.register_container(&root_id);
        assert_eq!(arena.get_parent(child_idx), Some(root_idx));
        assert_eq!(arena.get_depth(child_idx).map(|d| d.get()), Some(2));
    }
}

use crate::{
    arena::SharedArena,
    configure::Configure,
    container::idx::ContainerIdx,
    kv_store::{KvStore, SharedBlockCache},
    state::{
        container_store::FRONTIERS_KEY, ContainerCreationContext, ContainerState,
        FractionalIndexSetting, State,
    },
    sync::Mutex,
    utils::kv_wrapper::KvWrapper,
    version::Frontiers,
};
use bytes::Bytes;
use loro_common::{ContainerID, ContainerType, LoroResult, LoroValue};
use rustc_hash::FxHashSet;
use std::{collections::VecDeque, sync::Arc};

use super::ContainerWrapper;

const PAGE_SIZE: usize = 64;

#[cfg(test)]
thread_local! {
    static PAGE_STATS_RECOMPUTES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[derive(Debug)]
struct StorePage {
    slots: Vec<Option<ContainerWrapper>>, // length <= PAGE_SIZE
    live_count: u8,
    cached_state_bytes: std::cell::Cell<Option<usize>>,
}

impl StorePage {
    fn new() -> Self {
        Self {
            slots: Vec::new(),
            live_count: 0,
            cached_state_bytes: std::cell::Cell::new(None),
        }
    }

    fn state_bytes(&self) -> usize {
        if let Some(bytes) = self.cached_state_bytes.get() {
            return bytes;
        }
        #[cfg(test)]
        PAGE_STATS_RECOMPUTES.with(|n| n.set(n.get() + 1));
        let bytes = self
            .slots
            .iter()
            .flatten()
            .filter(|c| c.is_materialized_state())
            .map(|c| c.estimated_state_bytes())
            .sum();
        self.cached_state_bytes.set(Some(bytes));
        bytes
    }

    fn overhead_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.slots.capacity() * std::mem::size_of::<Option<ContainerWrapper>>()
    }
}

/// The invariants about this struct:
///
/// - `kv` is either the same or older than `pages`.
/// - `pages` is a cache over `kv`: every container in `kv` is either present in
///   `pages` or was evicted from it by the bounded decoded-value cache
///   (see `Configure::decoded_value_cache_limit`). Evicted entries are re-created from
///   `kv` on the next access, so lookups must always fall back to `kv` on a
///   `pages` miss, regardless of `load_state`.
/// - `load_state == AllLoaded` means every `kv` entry was materialized into
///   `pages` at some point; it does NOT mean `pages` is still complete. Once
///   `evicted_since_full_load` is set, `load_all()` must re-scan `kv`.
///
/// Invariants: it should be agnostic to the users of this struct whether a container is stored in `kv` or `pages`
pub(crate) struct InnerStore {
    arena: SharedArena,
    pages: Vec<Option<Box<StorePage>>>,
    live_pages: usize,
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
    fn page_and_offset(slot: usize) -> (usize, usize) {
        (slot / PAGE_SIZE, slot % PAGE_SIZE)
    }

    #[inline]
    fn get_entry_in(
        pages: &[Option<Box<StorePage>>],
        idx: ContainerIdx,
    ) -> Option<&ContainerWrapper> {
        let (page_idx, offset) = Self::page_and_offset(Self::slot(idx));
        let page = pages.get(page_idx)?.as_ref()?;
        let entry = page.slots.get(offset)?.as_ref()?;
        debug_assert_eq!(entry.kind(), idx.get_type());
        Some(entry)
    }

    #[inline]
    fn get_entry_mut_in(
        pages: &mut [Option<Box<StorePage>>],
        idx: ContainerIdx,
    ) -> Option<&mut ContainerWrapper> {
        let (page_idx, offset) = Self::page_and_offset(Self::slot(idx));
        let page = pages.get_mut(page_idx)?.as_mut()?;
        page.cached_state_bytes.set(None);
        let entry = page.slots.get_mut(offset)?.as_mut()?;
        debug_assert_eq!(entry.kind(), idx.get_type());
        Some(entry)
    }

    #[inline]
    fn get_entry_mut(&mut self, idx: ContainerIdx) -> Option<&mut ContainerWrapper> {
        Self::get_entry_mut_in(&mut self.pages, idx)
    }

    #[inline]
    fn get_entry_mut_preserve_state_bytes(
        &mut self,
        idx: ContainerIdx,
    ) -> Option<&mut ContainerWrapper> {
        let (page_idx, offset) = Self::page_and_offset(Self::slot(idx));
        let page = self.pages.get_mut(page_idx)?.as_mut()?;
        let entry = page.slots.get_mut(offset)?.as_mut()?;
        debug_assert_eq!(entry.kind(), idx.get_type());
        Some(entry)
    }

    #[inline]
    fn contains_idx_in(pages: &[Option<Box<StorePage>>], idx: ContainerIdx) -> bool {
        let (page_idx, offset) = Self::page_and_offset(Self::slot(idx));
        pages
            .get(page_idx)
            .and_then(|page| page.as_ref())
            .and_then(|page| page.slots.get(offset))
            .and_then(|entry| entry.as_ref())
            .is_some_and(|entry| entry.kind() == idx.get_type())
    }

    #[inline]
    fn contains_idx(&self, idx: ContainerIdx) -> bool {
        Self::contains_idx_in(&self.pages, idx)
    }

    fn insert_entry(
        pages: &mut Vec<Option<Box<StorePage>>>,
        live_pages: &mut usize,
        idx: ContainerIdx,
        container: ContainerWrapper,
    ) -> Option<ContainerWrapper> {
        let (page_idx, offset) = Self::page_and_offset(Self::slot(idx));
        if pages.len() <= page_idx {
            pages.resize_with(page_idx + 1, || None);
        }
        if pages[page_idx].is_none() {
            pages[page_idx] = Some(Box::new(StorePage::new()));
            *live_pages += 1;
        }
        let page = pages[page_idx].as_mut().unwrap();
        page.cached_state_bytes.set(None);
        if page.slots.len() <= offset {
            page.slots.resize_with(offset + 1, || None);
        }
        let prev = page.slots[offset].replace(container);
        if prev.is_none() {
            page.live_count += 1;
        }
        prev
    }

    fn take_slot_in(
        pages: &mut [Option<Box<StorePage>>],
        live_pages: &mut usize,
        slot: usize,
    ) -> Option<ContainerWrapper> {
        let (page_idx, offset) = Self::page_and_offset(slot);
        let page = pages.get_mut(page_idx)?.as_mut()?;
        page.cached_state_bytes.set(None);
        let taken = page.slots.get_mut(offset)?.take()?;
        page.live_count -= 1;
        if page.live_count == 0 {
            pages[page_idx] = None;
            *live_pages -= 1;
        } else {
            while page.slots.last().is_some_and(Option::is_none) {
                page.slots.pop();
            }
            if page.slots.capacity() > page.slots.len().saturating_mul(2).max(4) {
                page.slots.shrink_to_fit();
            }
        }
        Some(taken)
    }

    fn compact_trailing_empty_pages(&mut self) {
        while self.pages.last().is_some_and(Option::is_none) {
            self.pages.pop();
        }
        if self.pages.is_empty() {
            self.demote_clock_hand = 0;
            self.pages.shrink_to_fit();
        } else {
            self.demote_clock_hand %= self.pages.len() * PAGE_SIZE;
            if self.pages.capacity() > self.pages.len().saturating_mul(2).max(4) {
                self.pages.shrink_to_fit();
            }
        }
    }

    fn iter_entries(&self) -> impl Iterator<Item = (usize, &ContainerWrapper)> {
        self.pages
            .iter()
            .enumerate()
            .filter_map(|(page_idx, page)| page.as_ref().map(|p| (page_idx * PAGE_SIZE, p)))
            .flat_map(|(base, page)| {
                page.slots
                    .iter()
                    .enumerate()
                    .filter_map(move |(offset, entry)| entry.as_ref().map(|c| (base + offset, c)))
            })
    }

    fn iter_entries_mut(&mut self) -> impl Iterator<Item = (usize, &mut ContainerWrapper)> {
        self.pages
            .iter_mut()
            .enumerate()
            .filter_map(|(page_idx, page)| page.as_mut().map(|p| (page_idx * PAGE_SIZE, p)))
            .flat_map(|(base, page)| {
                page.cached_state_bytes.set(None);
                page.slots
                    .iter_mut()
                    .enumerate()
                    .filter_map(move |(offset, entry)| entry.as_mut().map(|c| (base + offset, c)))
            })
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
            Self::insert_entry(&mut self.pages, &mut self.live_pages, idx, container);
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
            Self::insert_entry(&mut self.pages, &mut self.live_pages, idx, c);
            return;
        }

        if let Some(journal) = self.created_journal_stack.last_mut() {
            journal.push(idx);
        }
        let c = f();
        Self::insert_entry(&mut self.pages, &mut self.live_pages, idx, c);
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
            if let Some(mut removed) =
                Self::take_slot_in(&mut self.pages, &mut self.live_pages, Self::slot(idx))
            {
                self.value_cache_bytes = self
                    .value_cache_bytes
                    .saturating_sub(removed.take_accounted_value_cache_bytes());
            }
        }
        if !created.is_empty() {
            self.compact_trailing_empty_pages();
        }
    }

    pub(crate) fn get_mut(&mut self, idx: ContainerIdx) -> Option<&mut ContainerWrapper> {
        if self.get_entry_mut(idx).is_none() {
            let id = self.arena.get_container_id(idx).unwrap();
            let key = id.to_bytes();
            if let Some(v) = self.kv.get(&key) {
                let c = ContainerWrapper::new_from_bytes(v);
                Self::insert_entry(&mut self.pages, &mut self.live_pages, idx, c);
            }
        }

        self.get_entry_mut(idx)
    }

    pub(crate) fn with_container_for_read<R>(
        &mut self,
        idx: ContainerIdx,
        f: impl FnOnce(&mut ContainerWrapper) -> R,
    ) -> Option<R> {
        let (page_idx, offset) = Self::page_and_offset(Self::slot(idx));
        if let Some(page) = self.pages.get_mut(page_idx).and_then(Option::as_mut) {
            if let Some(entry) = page.slots.get_mut(offset).and_then(Option::as_mut) {
                debug_assert_eq!(entry.kind(), idx.get_type());
                let was_fully_materialized = entry.is_fully_materialized_state();
                let ans = f(entry);
                if !was_fully_materialized && entry.is_materialized_state() {
                    page.cached_state_bytes.set(None);
                }
                if entry.needs_value_cache_tracking() && !entry.try_mark_value_cache_hit() {
                    self.track_value_cache(idx);
                }
                return Some(ans);
            }
        }

        let id = self.arena.get_container_id(idx).unwrap();
        let key = id.to_bytes();
        if let Some(v) = self.kv.get(&key) {
            let mut container = ContainerWrapper::new_from_bytes(v);
            let ans = f(&mut container);
            if container.has_cached_value() {
                Self::insert_entry(&mut self.pages, &mut self.live_pages, idx, container);
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
            let Some(entry) = self.get_entry_mut_preserve_state_bytes(idx) else {
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
        let mut evicted_any = false;
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
            let action = match self.get_entry_mut_preserve_state_bytes(victim) {
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
                    if let Some(mut removed) =
                        Self::take_slot_in(&mut self.pages, &mut self.live_pages, slot)
                    {
                        self.value_cache_bytes = self
                            .value_cache_bytes
                            .saturating_sub(removed.take_accounted_value_cache_bytes());
                        evicted_any = true;
                    }
                    // `pages` may no longer mirror `kv`: a full enumeration has
                    // to re-scan `kv` even in `AllLoaded` mode.
                    self.evicted_since_full_load = true;
                }
            }
        }
        if evicted_any {
            self.compact_trailing_empty_pages();
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
        let entry = Self::get_entry_in(&self.pages, idx)?;
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
        for (slot, c) in self.iter_entries() {
            if matches!(c.kind(), ContainerType::Tree | ContainerType::MovableList) {
                out.insert(ContainerIdx::from_index_and_type(slot as u32, c.kind()));
            }
        }
        out.into_iter().collect()
    }

    /// Fractional-index settings of decoded tree states that differ from the
    /// default. A setting can only be changed on a decoded state, so lazy
    /// entries are skipped without decoding them.
    pub(crate) fn tree_fractional_index_settings(
        &self,
    ) -> Vec<(ContainerIdx, FractionalIndexSetting)> {
        self.iter_entries()
            .filter_map(|(slot, entry)| {
                let State::TreeState(tree) = entry.try_get_state()? else {
                    return None;
                };
                let setting = tree.fractional_index_setting();
                (setting != FractionalIndexSetting::Jitter(0)).then(|| {
                    (
                        ContainerIdx::from_index_and_type(slot as u32, ContainerType::Tree),
                        setting,
                    )
                })
            })
            .collect()
    }

    pub(crate) fn iter_all_containers_mut(
        &mut self,
    ) -> impl Iterator<Item = (ContainerIdx, &mut ContainerWrapper)> {
        self.load_all();
        self.iter_entries_mut().map(|(slot, container)| {
            (
                ContainerIdx::from_index_and_type(slot as u32, container.kind()),
                container,
            )
        })
    }

    pub(crate) fn iter_all_container_ids(&mut self) -> impl Iterator<Item = ContainerID> + '_ {
        // PERF: we don't need to load all the containers here
        self.load_all();
        let arena = &self.arena;
        self.pages
            .iter()
            .enumerate()
            .filter_map(|(page_idx, page)| page.as_ref().map(|p| (page_idx * PAGE_SIZE, p)))
            .flat_map(move |(base, page)| {
                page.slots
                    .iter()
                    .enumerate()
                    .filter_map(move |(offset, entry)| {
                        entry.as_ref().map(|container| {
                            let idx = ContainerIdx::from_index_and_type(
                                (base + offset) as u32,
                                container.kind(),
                            );
                            arena.get_container_id(idx).unwrap()
                        })
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
        let arena = &self.arena;

        for (page_idx, page) in self.pages.iter_mut().enumerate() {
            let Some(page) = page.as_mut() else {
                continue;
            };
            page.cached_state_bytes.set(None);
            let base = page_idx * PAGE_SIZE;
            for (offset, entry) in page.slots.iter_mut().enumerate() {
                let Some(c) = entry.as_mut() else {
                    continue;
                };
                let idx = ContainerIdx::from_index_and_type((base + offset) as u32, c.kind());
                let cid = arena.get_container_id(idx).unwrap();
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

        self.pages.clear();
        self.live_pages = 0;
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

        self.pages.clear();
        self.live_pages = 0;
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
        let pages = &mut self.pages;
        let live_pages = &mut self.live_pages;
        let arena = &self.arena;
        arena.with_guards(|guards| {
            for (k, v) in entries {
                let cid = ContainerID::from_bytes(&k);
                let idx = guards.register_container(&cid);
                if Self::contains_idx_in(pages, idx) {
                    // the container is already loaded
                    // the content in `pages` is guaranteed to be newer than the content in `kv`
                    continue;
                }

                let container = ContainerWrapper::new_from_bytes(v);
                Self::insert_entry(pages, live_pages, idx, container);
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

        self.iter_entries().all(|(_, c)| c.is_state_empty())
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
        self.iter_entries()
            .filter(|(_, entry)| entry.is_evictable_cached_value())
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
        Self::new_with_cache(arena, config, SharedBlockCache::default_per_doc())
    }

    pub(crate) fn new_with_cache(
        arena: SharedArena,
        config: Configure,
        block_cache: SharedBlockCache,
    ) -> Self {
        Self {
            arena,
            pages: Vec::new(),
            live_pages: 0,
            kv: KvWrapper::new_mem_with_cache(block_cache),
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
        let total = self
            .pages
            .iter()
            .filter_map(Option::as_deref)
            .map(|p| p.state_bytes().saturating_add(p.overhead_bytes()))
            .sum::<usize>()
            .saturating_add(self.pages.capacity() * std::mem::size_of::<Option<Box<StorePage>>>());
        #[cfg(debug_assertions)]
        {
            let fresh = self
                .iter_entries()
                .filter(|(_, c)| c.is_materialized_state())
                .map(|(_, c)| c.estimated_state_bytes())
                .sum::<usize>()
                + self
                    .pages
                    .iter()
                    .filter_map(Option::as_deref)
                    .map(StorePage::overhead_bytes)
                    .sum::<usize>()
                + self.pages.capacity() * std::mem::size_of::<Option<Box<StorePage>>>();
            assert_eq!(total, fresh, "page statistics cache is stale");
        }
        total
    }

    pub(crate) fn kv_estimated_bytes(&self) -> usize {
        self.kv.estimated_memory_bytes()
    }

    pub(crate) fn spilled_disk_bytes(&self) -> usize {
        self.kv.spilled_disk_bytes()
    }

    pub(crate) fn take_spill_error(&self) -> Option<String> {
        self.kv.take_spill_error()
    }

    pub(crate) fn set_block_cache(&mut self, cache: SharedBlockCache) {
        self.kv.set_block_cache(cache);
    }

    pub(crate) fn flush_kv(&mut self) {
        self.kv.flush_compressed();
    }

    pub(crate) fn new_empty_same_backend(&self, arena: SharedArena, config: Configure) -> Self {
        Self {
            arena,
            pages: Vec::new(),
            live_pages: 0,
            kv: self.kv.new_empty_same_backend(),
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

    pub(crate) fn fill_kv_backend(
        &self,
        new_backend: &Arc<Mutex<dyn KvStore>>,
    ) -> Result<(), String> {
        self.kv.fill_backend(new_backend)
    }

    pub(crate) fn install_kv_backend(&mut self, new_backend: Arc<Mutex<dyn KvStore>>) {
        self.kv.install_backend(new_backend);
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
        for (_, c) in self.iter_entries_mut() {
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
        if cur_bytes <= target_state_bytes || self.live_pages == 0 {
            return 0;
        }

        self.ensure_parent_resolver_installed();
        let journal_set: FxHashSet<ContainerIdx> = self
            .created_journal_stack
            .iter()
            .flat_map(|v| v.iter().copied())
            .collect();
        let total_slots = self.pages.len() * PAGE_SIZE;
        let mut reclaimed = 0usize;
        let mut updates: Vec<(Bytes, Bytes)> = Vec::new();
        let deleted = self.config.deleted_root_containers.lock();

        for pass in 0..2 {
            let start_hand = self.demote_clock_hand % total_slots;
            let mut step = 0usize;
            while step < total_slots {
                if cur_bytes <= target_state_bytes || self.live_pages == 0 {
                    break;
                }
                let slot = (start_hand + step) % total_slots;
                let (page_idx, offset) = Self::page_and_offset(slot);
                let Some(page) = self.pages[page_idx].as_mut() else {
                    step += (PAGE_SIZE - offset).min(total_slots - step);
                    continue;
                };
                step += 1;
                page.cached_state_bytes.set(None);
                let Some(c) = page.slots.get_mut(offset).and_then(Option::as_mut) else {
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
                        self.demote_clock_hand = (slot + 1) % total_slots;
                        break;
                    }
                }

                let kind = c.kind();
                let idx = ContainerIdx::from_index_and_type(slot as u32, kind);
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
                let encoded = (!c.is_flushed()).then(|| c.encode());
                if let Some(value) = encoded {
                    updates.push((cid.to_bytes().into(), value));
                }
                let page_overhead_before = self.pages[page_idx]
                    .as_deref()
                    .map_or(0, StorePage::overhead_bytes);
                if let Some(mut old) =
                    Self::take_slot_in(&mut self.pages, &mut self.live_pages, slot)
                {
                    self.value_cache_bytes = self
                        .value_cache_bytes
                        .saturating_sub(old.take_accounted_value_cache_bytes());
                }
                let page_overhead_after = self.pages[page_idx]
                    .as_deref()
                    .map_or(0, StorePage::overhead_bytes);
                let page_freed = page_overhead_before.saturating_sub(page_overhead_after);
                let freed = entry_bytes.saturating_add(page_freed);
                self.evicted_since_full_load = true;
                reclaimed = reclaimed.saturating_add(freed);
                cur_bytes = cur_bytes.saturating_sub(freed);
                self.demote_clock_hand = (slot + 1) % total_slots;
            }
            if cur_bytes <= target_state_bytes || self.live_pages == 0 {
                break;
            }
        }
        drop(deleted);
        self.compact_trailing_empty_pages();

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

    #[test]
    fn budget_statistics_recompute_only_the_modified_page() {
        let doc = crate::LoroDoc::new_auto_commit();
        for i in 0..PAGE_SIZE * 8 {
            doc.get_map(format!("page-{i}")).insert("v", 0).unwrap();
        }
        doc.commit_then_renew();
        doc.set_memory_budget(crate::memory::MemoryBudget::new().with_max_doc_bytes(1 << 30));
        let map = doc.get_map("page-0");
        let _ = doc.memory_stats();
        PAGE_STATS_RECOMPUTES.with(|n| n.set(0));
        for i in 1..=20 {
            map.insert("v", i).unwrap();
            doc.commit_then_renew();
            let _ = doc.memory_stats();
        }
        PAGE_STATS_RECOMPUTES.with(|n| assert_eq!(n.get(), 20));
    }

    #[test]
    fn read_only_hits_do_not_recompute_page_statistics() {
        let doc = crate::LoroDoc::new_auto_commit();
        let map = doc.get_map("map");
        map.insert("v", 42).unwrap();
        let list = doc.get_list("list");
        list.push("item").unwrap();
        let text = doc.get_text("text");
        text.insert(0, "hello", crate::cursor::PosType::Unicode)
            .unwrap();
        doc.commit_then_renew();

        // Import into a fresh doc so `text` starts as unmaterialized `LazyLoad::Src`.
        let snapshot = doc.export(crate::encoding::ExportMode::Snapshot).unwrap();
        let loaded = crate::LoroDoc::new_auto_commit();
        loaded.import(&snapshot).unwrap();
        let loaded_map = loaded.get_map("map");
        let loaded_list = loaded.get_list("list");
        let loaded_text = loaded.get_text("text");
        // Materialize map and list first, leaving text as `LazyLoad::Src` in its wrapper.
        assert_eq!(loaded_map.get("v"), Some(42.into()));
        assert_eq!(loaded_list.get(0), Some("item".into()));
        let _ = loaded.memory_stats();

        PAGE_STATS_RECOMPUTES.with(|n| n.set(0));
        // Repeated read-only hits on materialized map/list do not invalidate page stats.
        for _ in 0..10 {
            assert_eq!(loaded_map.get("v"), Some(42.into()));
            assert_eq!(loaded_list.get(0), Some("item".into()));
            let _ = loaded.memory_stats();
        }
        PAGE_STATS_RECOMPUTES.with(|n| assert_eq!(n.get(), 0));

        // `to_string` decodes `LazyDecodedValue::Text` without materializing `State`,
        // so page state statistics stay valid across both initial decode and cache hits.
        for _ in 0..10 {
            assert_eq!(loaded_text.to_string(), "hello");
            let _ = loaded.memory_stats();
        }
        PAGE_STATS_RECOMPUTES.with(|n| assert_eq!(n.get(), 0));

        // `slice` materializes `State::RichtextState`, invalidating the page once;
        // subsequent reads on the materialized text state do not recompute page stats.
        assert_eq!(
            loaded_text
                .slice(0, 5, crate::cursor::PosType::Unicode)
                .unwrap(),
            "hello"
        );
        let _ = loaded.memory_stats();
        PAGE_STATS_RECOMPUTES.with(|n| assert_eq!(n.get(), 1));

        PAGE_STATS_RECOMPUTES.with(|n| n.set(0));
        for _ in 0..10 {
            assert_eq!(loaded_text.to_string(), "hello");
            assert_eq!(loaded_text.len_unicode(), 5);
            let _ = loaded.memory_stats();
        }
        PAGE_STATS_RECOMPUTES.with(|n| assert_eq!(n.get(), 0));
    }

    #[test]
    fn full_clock_sweep_visits_every_cold_container() {
        for count in [5, 100, 1000] {
            for hand in [0, 3] {
                let doc = crate::LoroDoc::new_auto_commit();
                for i in 0..count {
                    doc.get_map(format!("m{i}")).insert("k", i as i64).unwrap();
                }
                doc.commit_then_renew();
                let expected = doc.get_deep_value();
                {
                    let mut state = doc.app_state().lock();
                    let store = &mut state.store.store;
                    store.demote_clock_hand = hand;
                    store.demote_cold_states(0, &Default::default());
                    assert!(
                        store
                            .iter_entries()
                            .all(|(_, c)| !c.is_materialized_state()),
                        "count={count}, hand={hand}"
                    );
                }
                assert_eq!(doc.get_deep_value(), expected);
                doc.get_map("m0").insert("after", true).unwrap();
                doc.commit_then_renew();
            }
        }
    }

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

    #[test]
    fn sparse_high_index_uses_single_page_and_frees_on_remove() {
        use crate::state::{ListState, State};
        let arena = SharedArena::new();
        for i in 0..500 {
            let cid = ContainerID::new_root(&format!("r{i}"), ContainerType::List);
            arena.register_container(&cid);
        }
        let target_cid = ContainerID::new_root("target", ContainerType::List);
        let target_idx = arena.register_container(&target_cid);
        assert_eq!(target_idx.to_index(), 500);

        let arena_for_wrap = arena.clone();
        let mut store = InnerStore::new(arena, Configure::default());
        store.begin_created_journal();
        store.ensure_container(target_idx, || {
            ContainerWrapper::new(
                State::ListState(Box::new(ListState::new(target_idx))),
                &arena_for_wrap,
            )
        });
        assert_eq!(store.live_pages, 1);
        assert_eq!(store.pages.len(), 500 / PAGE_SIZE + 1);
        assert!(store.pages[0].is_none());

        let created = store.take_created_journal();
        store.remove_created(&created);
        assert_eq!(store.live_pages, 0);
        assert!(store.pages.is_empty());
    }
}

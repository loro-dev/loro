use self::block_encode::{
    decode_block, decode_cids, decode_header, encode_block, ChangesBlockHeader,
};
use super::{loro_dag::AppDagNodeInner, AppDagNode};
use crate::sync::Mutex;
use crate::{
    arena::{ArenaExtent, CreatorOp, SharedArena, SharedArenaRollback},
    change::Change,
    estimated_size::EstimatedSize,
    kv_store::KvStore,
    op::Op,
    parent::register_container_and_parent_link,
    version::{Frontiers, ImVersionVector},
    InternalString, VersionVector,
};
use block_encode::decode_block_range;
use bytes::Bytes;
use itertools::Itertools;
use loro_common::{
    ContainerID, Counter, HasCounterSpan, HasId, HasIdSpan, HasLamportSpan, IdLp, IdSpan, Lamport,
    LoroError, LoroResult, PeerID, ID,
};
use loro_kv_store::{mem_store::MemKvConfig, MemKvStore};
use once_cell::sync::OnceCell;
use rle::{HasLength, Mergable, RlePush, RleVec, Sliceable};
use rustc_hash::{FxHashMap, FxHashSet};
use std::sync::atomic::AtomicI64;
#[cfg(test)]
use std::sync::atomic::AtomicUsize;
use std::{
    cmp::Ordering,
    collections::{BTreeMap, VecDeque},
    ops::{Bound, Deref},
    sync::Arc,
};
use tracing::{info_span, warn};
mod block_encode;
mod block_meta_encode;
pub(super) mod iter;

#[cfg(not(test))]
const MAX_BLOCK_SIZE: usize = 1024 * 4;
#[cfg(test)]
const MAX_BLOCK_SIZE: usize = 128;
const MAX_ROOT_HISTORY_NAME_BYTES: usize = 256 * 1024;

/// # Invariance
///
/// - We don't allow holes in a block or between two blocks with the same peer id.
///   The [Change] should be continuous for each peer.
/// - However, the first block of a peer can have counter > 0 so that we can trim the history.
///
/// # Locking
///
/// Take the locks in this order, and release them before calling out: `root_history_names`,
/// `external_kv`, `inner`, `external_vv`. Parsing a block takes the arena's lock under them.
/// `parse_failures` is a leaf: it is taken under any of them and takes nothing itself.
/// Most callers hold the document's op log lock, which serializes them, but the arena's
/// creator resolver ([`ChangeStore::creator_resolver`]) does not, so a method that takes two
/// of these locks in another order can deadlock against it. See
/// `context/arena-parent-links.md`.
///
/// # Encoding Schema
///
/// It's based on the underlying KV store.
///
/// The entries of the KV store is made up of the following fields
///
/// |Key                          |Value             |
/// |:--                          |:----             |
/// |b"vv"                        |VersionVector     |
/// |b"fr"                        |Frontiers         |
/// |b"sv"                        |Shallow VV        |
/// |b"sf"                        |Shallow Frontiers |
/// |12 bytes PeerID + Counter    |Encoded Block     |
#[derive(Debug, Clone)]
pub struct ChangeStore {
    inner: Arc<Mutex<ChangeStoreInner>>,
    arena: SharedArena,
    /// A change may be in external_kv or in the mem_parsed_kv.
    /// mem_parsed_kv is more up-to-date.
    ///
    /// We cannot directly write into the external_kv except from the initial load
    external_kv: Arc<Mutex<dyn KvStore>>,
    /// The version vector of the external kv store.
    external_vv: Arc<Mutex<VersionVector>>,
    merge_interval: Arc<AtomicI64>,
    root_history_names: Arc<Mutex<RootHistoryNamesState>>,
    parse_failures: Arc<ParseFailures>,
    #[cfg(test)]
    root_history_scan_count: Arc<AtomicUsize>,
}

/// The blocks of a store that could not be decoded or parsed.
///
/// Snapshot import validates the KV checksums, so such a block is forged or truncated
/// external input. Every reader of the store answers "no such change" for it, because most
/// of them cannot return an error. They all record it here, so the document's fallible entry
/// points can refuse to work from a partial history. See `context/arena-parent-links.md`.
#[derive(Debug, Default)]
struct ParseFailures {
    /// The first block that failed, with its error.
    first: Mutex<Option<(ID, Box<str>)>>,
}

impl ParseFailures {
    fn record(&self, block_id: ID, err: &LoroError) {
        let mut first = self.first.lock();
        if first.is_none() {
            *first = Some((block_id, err.to_string().into_boxed_str()));
        }
    }
}

/// A conservative, size-capped set of every top-level root name that appears in the store's
/// history. It only answers "may history have touched this root?", so retaining stale names
/// (e.g. from a rolled-back import) is harmless — it just forces the general diff path.
#[derive(Debug, Default)]
enum RootHistoryNamesState {
    #[default]
    Uninitialized,
    Valid {
        names: FxHashSet<InternalString>,
        name_bytes: usize,
    },
    Invalid,
}

/// Return false when retaining the name would exceed the fixed memory bound.
fn record_root_name(
    names: &mut FxHashSet<InternalString>,
    name_bytes: &mut usize,
    cid: &ContainerID,
) -> bool {
    let ContainerID::Root { name, .. } = cid else {
        return true;
    };
    if cid.is_mergeable() || names.contains(name) {
        return true;
    }

    let Some(new_bytes) = name_bytes.checked_add(name.len()) else {
        return false;
    };
    if new_bytes > MAX_ROOT_HISTORY_NAME_BYTES {
        return false;
    }

    names.insert(name.clone());
    *name_bytes = new_bytes;
    true
}

#[derive(Debug, Clone)]
struct ChangeStoreInner {
    /// The start version vector of the first block for each peer.
    /// It allows us to trim the history
    start_vv: ImVersionVector,
    /// The last version of the shallow history.
    start_frontiers: Frontiers,
    /// It's more like a parsed cache for binary_kv.
    mem_parsed_kv: BTreeMap<ID, Arc<ChangesBlock>>,
    /// Set by [`ChangeStore::retire`]: the op log replaced this store, so loading from it
    /// must not register anything in the arena any more.
    retired: bool,
}

#[derive(Debug)]
pub(crate) struct ChangeStoreRollback {
    old_vv: VersionVector,
    /// Pre-scope blocks that an import appended to, keyed by block id. `None` means the
    /// block was flushed, so its KV copy is the pre-scope version.
    blocks_before_mutation: BTreeMap<ID, Option<BlockShape>>,
}

/// What an unflushed block looked like before an import appended to it.
///
/// Imports only append to a block (`ChangesBlock::push_change`): they push changes,
/// or push ops onto its last change, where an op may merge into the last op. So the
/// shape only needs the change count, the last change's op count and its last op.
/// Keeping an `Arc` of the whole block instead made the next append copy the block's
/// changes, and cloning the last change costs as much once remote changes keep
/// merging into it.
#[derive(Debug)]
struct BlockShape {
    counter_end: Counter,
    lamport_end: Lamport,
    estimated_size: usize,
    n_changes: usize,
    /// `(merged op count, last op)` of the last change.
    last_change_ops: Option<(usize, Option<Op>)>,
}

impl ChangeStoreRollback {
    pub(crate) fn new(old_vv: VersionVector) -> Self {
        Self {
            old_vv,
            blocks_before_mutation: BTreeMap::new(),
        }
    }

    /// The version when the import scope began.
    pub(crate) fn old_vv(&self) -> &VersionVector {
        &self.old_vv
    }

    fn record_block_before_mutation(&mut self, id: ID, block: &ChangesBlock) {
        let old_end = self.old_vv.get(&id.peer).copied().unwrap_or(0);
        if id.counter >= old_end {
            return;
        }

        self.blocks_before_mutation.entry(id).or_insert_with(|| {
            if block.flushed {
                return None;
            }
            let changes = block
                .content
                .try_changes()
                .expect("an unflushed block always holds parsed changes");
            Some(BlockShape {
                counter_end: block.counter_range.1,
                lamport_end: block.lamport_range.1,
                estimated_size: block.estimated_size,
                n_changes: changes.len(),
                last_change_ops: changes.last().map(|c| (c.ops.len(), c.ops.last().cloned())),
            })
        });
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ChangesBlock {
    peer: PeerID,
    counter_range: (Counter, Counter),
    lamport_range: (Lamport, Lamport),
    /// Estimated size of the block in bytes
    estimated_size: usize,
    flushed: bool,
    content: ChangesBlockContent,
    /// Where the arena reached when `content` was parsed from its bytes (`Both`). See
    /// [`ChangeStore::rollback_arena`].
    parsed_extent: ArenaExtent,
}

#[derive(Clone)]
pub(crate) enum ChangesBlockContent {
    Changes(Arc<Vec<Change>>),
    Bytes(ChangesBlockBytes),
    Both(Arc<Vec<Change>>, ChangesBlockBytes),
}

/// It's cheap to clone this struct because it's cheap to clone the bytes
#[derive(Clone)]
pub(crate) struct ChangesBlockBytes {
    bytes: Bytes,
    header: OnceCell<Arc<ChangesBlockHeader>>,
}

pub const START_VV_KEY: &[u8] = b"sv";
pub const START_FRONTIERS_KEY: &[u8] = b"sf";
pub const VV_KEY: &[u8] = b"vv";
pub const FRONTIERS_KEY: &[u8] = b"fr";

impl ChangeStore {
    pub fn new_mem(a: &SharedArena, merge_interval: Arc<AtomicI64>) -> Self {
        Self {
            inner: Arc::new(Mutex::new(ChangeStoreInner {
                start_vv: ImVersionVector::new(),
                start_frontiers: Frontiers::default(),
                mem_parsed_kv: BTreeMap::new(),
                retired: false,
            })),
            arena: a.clone(),
            external_vv: Arc::new(Mutex::new(VersionVector::new())),
            external_kv: Arc::new(Mutex::new(MemKvStore::new(MemKvConfig::default()))),
            // external_kv: Arc::new(Mutex::new(BTreeMap::default())),
            merge_interval,
            root_history_names: Arc::new(Mutex::new(RootHistoryNamesState::Uninitialized)),
            parse_failures: Default::default(),
            #[cfg(test)]
            root_history_scan_count: Arc::new(AtomicUsize::new(0)),
        }
    }

    #[cfg(test)]
    fn new_for_test() -> Self {
        Self::new_mem(&SharedArena::new(), Arc::new(AtomicI64::new(0)))
    }

    pub(super) fn encode_all(&self, vv: &VersionVector, frontiers: &Frontiers) -> Bytes {
        self.flush_and_compact(vv, frontiers);
        let mut kv = self.external_kv.lock();
        kv.export_all()
    }

    #[tracing::instrument(skip(self), level = "debug")]
    pub(super) fn export_from(
        &self,
        start_vv: &VersionVector,
        start_frontiers: &Frontiers,
        latest_vv: &VersionVector,
        latest_frontiers: &Frontiers,
    ) -> Bytes {
        let new_store = ChangeStore::new_mem(&self.arena, self.merge_interval.clone());
        for span in latest_vv.sub_iter(start_vv) {
            // PERF: this can be optimized by reusing the current encoded blocks
            // In the current method, it needs to parse and re-encode the blocks
            for c in self.iter_changes(span) {
                let start = ((start_vv.get(&c.id.peer).copied().unwrap_or(0) - c.id.counter).max(0)
                    as usize)
                    .min(c.atom_len());
                let end = ((latest_vv.get(&c.id.peer).copied().unwrap_or(0) - c.id.counter).max(0)
                    as usize)
                    .min(c.atom_len());

                if start == end {
                    continue;
                }

                let ch = c.slice(start, end);
                new_store.insert_change(ch, false, false);
            }
        }

        loro_common::debug!(
            "start_vv={:?} start_frontiers={:?}",
            &start_vv,
            start_frontiers
        );
        new_store.encode_from(start_vv, start_frontiers, latest_vv, latest_frontiers)
    }

    pub(super) fn export_blocks_in_range<W: std::io::Write>(&self, spans: &[IdSpan], w: &mut W) {
        // A change store needs each peer's counters to be contiguous, so merge the
        // ranges of each peer and put the k-th range of every peer into the k-th
        // store. The blocks of all stores are written one after another, which the
        // update decoder reads the same way (loro-dev/loro#1155).
        let mut ranges: FxHashMap<PeerID, Vec<(Counter, Counter)>> = FxHashMap::default();
        for span in spans {
            let mut span = *span;
            span.normalize_();
            let start = span.counter.start.max(0);
            let end = span.counter.end.max(0);
            if start < end {
                ranges.entry(span.peer).or_default().push((start, end));
            }
        }

        let mut layers: Vec<Vec<IdSpan>> = Vec::new();
        for (peer, mut peer_ranges) in ranges {
            peer_ranges.sort_unstable();
            let mut merged: Vec<(Counter, Counter)> = Vec::with_capacity(peer_ranges.len());
            for (start, end) in peer_ranges {
                match merged.last_mut() {
                    Some(last) if start <= last.1 => last.1 = last.1.max(end),
                    _ => merged.push((start, end)),
                }
            }
            for (i, (start, end)) in merged.into_iter().enumerate() {
                if layers.len() <= i {
                    layers.push(Vec::new());
                }
                layers[i].push(IdSpan::new(peer, start, end));
            }
        }

        for layer in layers {
            let new_store = ChangeStore::new_mem(&self.arena, self.merge_interval.clone());
            for span in layer {
                // PERF: this can be optimized by reusing the current encoded blocks
                // In the current method, it needs to parse and re-encode the blocks
                for c in self.iter_changes(span) {
                    let start =
                        ((span.counter.start - c.id.counter).max(0) as usize).min(c.atom_len());
                    let end = ((span.counter.end - c.id.counter).max(0) as usize).min(c.atom_len());
                    if start == end {
                        continue;
                    }

                    let ch = c.slice(start, end);
                    new_store.insert_change(ch, false, false);
                }
            }

            encode_blocks_in_store(new_store, &self.arena, w);
        }
    }

    fn encode_from(
        &self,
        start_vv: &VersionVector,
        start_frontiers: &Frontiers,
        latest_vv: &VersionVector,
        latest_frontiers: &Frontiers,
    ) -> Bytes {
        {
            let mut store = self.external_kv.lock();
            store.set(START_VV_KEY, start_vv.encode().into());
            store.set(START_FRONTIERS_KEY, start_frontiers.encode().into());
            let mut inner = self.inner.lock();
            inner.start_frontiers = start_frontiers.clone();
            inner.start_vv = ImVersionVector::from_vv(start_vv);
        }
        self.flush_and_compact(latest_vv, latest_frontiers);
        self.external_kv.lock().export_all()
    }

    /// Read a cold block for comparison without caching parsed ops in the arena.
    /// Cached parsed history keeps using the ordinary, already cheap path.
    pub(crate) fn unparsed_block_bytes(&self, id: ID) -> Option<Bytes> {
        let external = self.external_kv.lock();
        let inner = self.inner.lock();
        if inner.retired {
            return None;
        }
        if let Some((_, block)) = inner.mem_parsed_kv.range(..=id).next_back() {
            if block.peer == id.peer && block.counter_range.1 > id.counter {
                return match &block.content {
                    ChangesBlockContent::Bytes(b) => Some(b.bytes.clone()),
                    _ => None,
                };
            }
        }
        let (key, bytes) = external
            .scan(Bound::Unbounded, Bound::Included(&id.to_bytes()))
            .rfind(|(key, _)| key.len() == 12)?;
        if ID::from_bytes(&key).peer != id.peer
            || decode_block_range(&bytes).ok()?.0 .1 <= id.counter
        {
            return None;
        }
        Some(bytes)
    }

    pub(crate) fn encoded_block_counter_range(bytes: &[u8]) -> LoroResult<(Counter, Counter)> {
        Ok(decode_block_range(bytes)?.0)
    }

    pub(crate) fn check_text_insert_block(
        &self,
        bytes: &[u8],
        mut on_insert: impl FnMut(Counter, &ContainerID, u32, &str, u32, &Frontiers) -> LoroResult<()>,
    ) -> LoroResult<bool> {
        // This reader has stricter eligibility checks than decode_block. Only
        // the normal parser may declare local history unparsable. Delay a content
        // mismatch until the entire block is eligible, otherwise fall back too.
        let mut comparison = Ok(());
        let result =
            block_encode::visit_text_insert_block(bytes, |counter, cid, pos, text, len, deps| {
                if comparison.is_ok() {
                    comparison = on_insert(counter, cid, pos, text, len, deps);
                }
                Ok(())
            });
        match result {
            Ok(true) => comparison.map(|()| true),
            Ok(false) | Err(_) => Ok(false),
        }
    }

    /// Byte-identical, fully known blocks need neither decoding nor comparison.
    /// An unflushed cached block shadows its older KV copy; never compare against
    /// that stale copy. Different encodings fall back to semantic comparison.
    pub(crate) fn contains_encoded_block(&self, id: ID, bytes: &[u8]) -> bool {
        let external = self.external_kv.lock();
        let inner = self.inner.lock();
        if let Some(block) = inner.mem_parsed_kv.get(&id) {
            return match &block.content {
                ChangesBlockContent::Bytes(b) | ChangesBlockContent::Both(_, b) => {
                    b.bytes.as_ref() == bytes
                }
                ChangesBlockContent::Changes(_) => false,
            };
        }
        external
            .get(&id.to_bytes())
            .is_some_and(|b| b.as_ref() == bytes)
    }

    /// Decode only unmatched snapshot blocks in a temporary arena. Known content
    /// never gets copied into the document's arena; move only the checked suffix.
    pub(crate) fn decode_snapshot_for_updates(
        bytes: Bytes,
        oplog: &crate::OpLog,
    ) -> Result<Vec<Change>, LoroError> {
        let arena = SharedArena::new();
        let store = ChangeStore::new_mem(&arena, Arc::new(AtomicI64::new(0)));
        let _ = store.import_all(bytes)?;
        let external = store.external_kv.lock();
        let mut inner = store.inner.lock();
        let mut changes = Vec::new();
        for (key, bytes) in external.scan(Bound::Unbounded, Bound::Unbounded) {
            if key.len() != 12 {
                continue;
            }
            let id = ID::from_bytes(&key);
            if oplog.change_store.contains_encoded_block(id, &bytes) {
                continue;
            }
            // import_all parsed the frontier blocks. Take their changes instead
            // of parsing them again or cloning changes that will be dropped.
            if let Some(block) = inner.mem_parsed_kv.remove(&id) {
                let block = Arc::try_unwrap(block).expect("temporary store owns its blocks");
                match block.content {
                    ChangesBlockContent::Changes(c) | ChangesBlockContent::Both(c, _) => {
                        changes
                            .extend(Arc::try_unwrap(c).expect("temporary store owns its changes"));
                    }
                    ChangesBlockContent::Bytes(b) => changes.extend(b.parse(&arena)?),
                }
            } else {
                changes.extend(Self::decode_block_bytes(bytes, &arena)?);
            }
        }
        drop(inner);
        drop(external);
        changes.sort_unstable_by_key(|c| c.lamport);
        let changes = oplog.check_and_trim_known_part_in_arena(
            changes,
            super::ImportedValues::Exact,
            &arena,
        )?;
        Ok(changes
            .into_iter()
            .map(|mut change| {
                let mut ops = RleVec::new();
                for op in change.ops.iter() {
                    for remote in super::local_op_to_remote(&arena, op) {
                        ops.push(oplog.arena.convert_single_op(
                            &remote.container,
                            change.id.peer,
                            remote.counter,
                            change.lamport + (remote.counter - change.id.counter) as Lamport,
                            remote.content,
                        ));
                    }
                }
                change.ops = ops;
                register_container_and_parent_link(&oplog.arena, &change);
                change
            })
            .collect())
    }

    pub(crate) fn decode_block_bytes(bytes: Bytes, arena: &SharedArena) -> LoroResult<Vec<Change>> {
        ChangesBlockBytes::new(bytes).parse(arena)
    }

    /// Reuse the header on fallback so a new block is decoded only once.
    pub(crate) fn decode_update_block(
        &self,
        bytes: Bytes,
        vv: &VersionVector,
    ) -> LoroResult<Vec<Change>> {
        let block = ChangesBlockBytes::new(bytes);
        block.ensure_header()?;
        let header = block.header.get().unwrap();
        let id = ID::new(header.peer, header.counter);
        if header.counters.last().copied().unwrap_or(0)
            <= vv.get(&header.peer).copied().unwrap_or(0)
            && self.contains_encoded_block(id, &block.bytes)
        {
            Ok(Vec::new())
        } else {
            block.parse(&self.arena)
        }
    }

    /// Rolls back the store and the arena (to `arena`, the checkpoint taken when the import
    /// began). See [`Self::rollback_arena`].
    pub(crate) fn rollback_import(
        &self,
        rollback: ChangeStoreRollback,
        arena: SharedArenaRollback,
    ) {
        let mut inner = self.inner.lock();
        Self::rollback_changes_in(&self.arena, &mut inner, rollback);
        self.rollback_arena_in(&mut inner, arena);
    }

    /// [`Self::rollback_import`] without rolling the arena back: everything registered in it
    /// since the scope began stays registered. For a scope that only held changes whose
    /// containers were registered before it (a rolled back local transaction), where freeing
    /// the registrations made while undoing it from the state would leave state entries at
    /// freed indices.
    pub(crate) fn rollback_import_keeping_arena(&self, rollback: ChangeStoreRollback) {
        let mut inner = self.inner.lock();
        Self::rollback_changes_in(&self.arena, &mut inner, rollback);
    }

    fn rollback_changes_in(
        arena: &SharedArena,
        inner: &mut ChangeStoreInner,
        rollback: ChangeStoreRollback,
    ) {
        // The name set may already include names from changes this rollback removes. That is
        // fine: stale names only make `old_history_may_touch_root_names` conservatively true.
        let mut touched_peers = FxHashSet::default();
        inner.mem_parsed_kv.retain(|id, _| {
            let old_end = rollback.old_vv.get(&id.peer).copied().unwrap_or(0);
            let keep = id.counter < old_end;
            if !keep {
                touched_peers.insert(id.peer);
            }
            keep
        });

        for (id, shape) in rollback.blocks_before_mutation {
            touched_peers.insert(id.peer);
            let Some(shape) = shape else {
                // Flushed before the scope: the KV copy is the pre-scope block.
                inner.mem_parsed_kv.remove(&id);
                continue;
            };
            let block = inner
                .mem_parsed_kv
                .get_mut(&id)
                .expect("a block appended to during the scope stays cached");
            let block = Arc::make_mut(block);
            let changes = Arc::make_mut(
                block
                    .content
                    .changes_mut(arena)
                    .expect("an unflushed block always holds parsed changes"),
            );
            changes.truncate(shape.n_changes);
            if let Some((n_ops, last_op)) = shape.last_change_ops {
                let ops = changes.last_mut().unwrap().ops.vec_mut();
                ops.truncate(n_ops);
                if let Some(last_op) = last_op {
                    *ops.last_mut().unwrap() = last_op;
                }
            }
            block.counter_range.1 = shape.counter_end;
            block.lamport_range.1 = shape.lamport_end;
            block.estimated_size = shape.estimated_size;
        }

        // `insert_change_inner` merges a change into the cached block right before it.
        // A read during the scope (e.g. an lamport lookup) may have cached an older
        // KV block of a touched peer, and removing the scope's newer blocks leaves it
        // in front of the next insert with a counter gap ("counter should be
        // continuous"). Blocks that are flushed are identical to their KV copy, so
        // evict them; they are reloaded on demand.
        if !touched_peers.is_empty() {
            inner
                .mem_parsed_kv
                .retain(|id, block| !block.flushed || !touched_peers.contains(&id.peer));
        }
    }

    /// Rolls the arena back to `arena`, a checkpoint taken before a failed import, and drops
    /// the parsed changes of the cached blocks that were parsed since. Every arena rollback must
    /// go through here (or [`Self::rollback_import`] / [`Self::retire`]).
    ///
    /// Parsing a block registers the containers its ops use with their parent links and
    /// allocates their values. The arena rollback truncates the values and text allocated after
    /// the checkpoint and drops the parent links of containers registered after it (their
    /// indices stay; see `SharedArena::rollback`), so a block parsed in between may hold value
    /// slices that no longer exist, and parsing it again is what registers those links again.
    /// Such a block keeps only its bytes, so the next access parses and registers again. A block parsed before the checkpoint can only refer to what was there
    /// then (its `parsed_extent`), and keeps its parsed changes. A block without bytes was built
    /// in memory from changes inserted before the import, whose containers were registered
    /// then.
    ///
    /// This happens under `inner`, where blocks are parsed. The creator resolver parses
    /// without the op log lock, so otherwise it could parse a block after the bytes are
    /// restored and before the arena is rolled back.
    pub(crate) fn rollback_arena(&self, arena: SharedArenaRollback) {
        let mut inner = self.inner.lock();
        self.rollback_arena_in(&mut inner, arena);
    }

    fn rollback_arena_in(&self, inner: &mut ChangeStoreInner, arena: SharedArenaRollback) {
        for block in inner.mem_parsed_kv.values_mut() {
            if let ChangesBlockContent::Both(_, bytes) = &block.content {
                if !arena.keeps(block.parsed_extent) {
                    let bytes = bytes.clone();
                    Arc::make_mut(block).content = ChangesBlockContent::Bytes(bytes);
                }
            }
        }
        self.arena.rollback(arena);
    }

    /// Rolls the arena back to `arena` after the op log replaced this store (a failed
    /// snapshot import), and stops loading from it. A creator resolver that reached this store
    /// before the replacement then finds nothing instead of registering containers of the
    /// discarded history in the arena.
    pub(crate) fn retire(&self, arena: SharedArenaRollback) {
        let mut inner = self.inner.lock();
        self.arena.rollback(arena);
        inner.mem_parsed_kv.clear();
        inner.retired = true;
    }

    pub fn get_dag_nodes_that_contains(&self, id: ID) -> Option<Vec<AppDagNode>> {
        let block = self.get_block_that_contains(id)?;
        Some(block.content.iter_dag_nodes())
    }

    pub fn get_last_dag_nodes_for_peer(&self, peer: PeerID) -> Option<Vec<AppDagNode>> {
        let block = self.get_the_last_block_of_peer(peer)?;
        Some(block.content.iter_dag_nodes())
    }

    pub fn visit_all_changes(&self, f: &mut dyn FnMut(&Change)) {
        self.ensure_block_loaded_in_range(Bound::Unbounded, Bound::Unbounded);
        let mut inner = self.inner.lock();
        for (id, block) in inner.mem_parsed_kv.iter_mut() {
            if let Err(err) = block.ensure_changes(&self.arena) {
                warn!(block_id = ?id, ?err, "failed to parse change block");
                self.parse_failures.record(*id, &err);
                continue;
            }
            for c in block.content.try_changes().unwrap() {
                f(c);
            }
        }
    }

    fn build_root_history_names(&self) -> RootHistoryNamesState {
        let mut names = FxHashSet::default();
        let mut name_bytes = 0;

        {
            let external = self.external_kv.lock();
            for (key, bytes) in external.scan(Bound::Unbounded, Bound::Unbounded) {
                if key.len() != 12 {
                    continue;
                }
                let header = match decode_header(&bytes)
                    .and_then(|header| decode_cids(&bytes, Some(header)))
                {
                    Ok(header) => header,
                    Err(_) => return RootHistoryNamesState::Invalid,
                };
                let Some(cids) = header.cids.get() else {
                    return RootHistoryNamesState::Invalid;
                };
                for cid in cids.iter() {
                    if !record_root_name(&mut names, &mut name_bytes, cid) {
                        return RootHistoryNamesState::Invalid;
                    }
                }
            }
        }

        let inner = self.inner.lock();
        for block in inner.mem_parsed_kv.values() {
            match &block.content {
                ChangesBlockContent::Changes(changes) | ChangesBlockContent::Both(changes, _) => {
                    for change in changes.iter() {
                        for op in change.ops.iter() {
                            let Some(cid) = self.arena.idx_to_id(op.container) else {
                                return RootHistoryNamesState::Invalid;
                            };
                            if !record_root_name(&mut names, &mut name_bytes, &cid) {
                                return RootHistoryNamesState::Invalid;
                            }
                        }
                    }
                }
                ChangesBlockContent::Bytes(bytes) => {
                    let header = bytes.header.get().map(|header| header.as_ref().clone());
                    let header = match decode_cids(&bytes.bytes, header) {
                        Ok(header) => header,
                        Err(_) => return RootHistoryNamesState::Invalid,
                    };
                    let Some(cids) = header.cids.get() else {
                        return RootHistoryNamesState::Invalid;
                    };
                    for cid in cids.iter() {
                        if !record_root_name(&mut names, &mut name_bytes, cid) {
                            return RootHistoryNamesState::Invalid;
                        }
                    }
                }
            }
        }

        RootHistoryNamesState::Valid { names, name_bytes }
    }

    fn record_change_in_root_history_names(&self, change: &Change) {
        let mut cached = self.root_history_names.lock();
        let RootHistoryNamesState::Valid { names, name_bytes } = &mut *cached else {
            return;
        };

        for op in change.ops.iter() {
            let recorded = self
                .arena
                .idx_to_id(op.container)
                .is_some_and(|cid| record_root_name(names, name_bytes, &cid));
            if !recorded {
                *cached = RootHistoryNamesState::Invalid;
                return;
            }
        }
    }

    /// Return whether the history already in this store may have touched any top-level root in
    /// `names`. Must be called before the candidate changes are inserted.
    ///
    /// The first call scans encoded block container arenas without parsing operations or
    /// populating the parsed-change cache; later inserts update the cached name set, so repeated
    /// independent imports do not rescan the old history. Decode failures or exceeding the size
    /// cap permanently disable this optimization for the store.
    pub(crate) fn old_history_may_touch_root_names(
        &self,
        names: &FxHashSet<InternalString>,
    ) -> bool {
        if names.is_empty() {
            return false;
        }

        let mut cached = self.root_history_names.lock();
        if matches!(*cached, RootHistoryNamesState::Uninitialized) {
            #[cfg(test)]
            self.root_history_scan_count
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            *cached = self.build_root_history_names();
        }

        match &*cached {
            RootHistoryNamesState::Valid {
                names: history_names,
                ..
            } => names.iter().any(|name| history_names.contains(name)),
            RootHistoryNamesState::Invalid | RootHistoryNamesState::Uninitialized => true,
        }
    }

    #[cfg(test)]
    pub(crate) fn root_history_scan_count_for_test(&self) -> usize {
        self.root_history_scan_count
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    pub(crate) fn iter_blocks(&self, id_span: IdSpan) -> Vec<(Arc<ChangesBlock>, usize, usize)> {
        if id_span.counter.start == id_span.counter.end {
            return vec![];
        }

        assert!(id_span.counter.start < id_span.counter.end);
        self.ensure_block_loaded_in_range(
            Bound::Included(id_span.id_start()),
            Bound::Excluded(id_span.id_end()),
        );
        let mut inner = self.inner.lock();
        let next_back = inner.mem_parsed_kv.range(..=id_span.id_start()).next_back();
        match next_back {
            None => {
                return vec![];
            }
            Some(next_back) => {
                if next_back.0.peer != id_span.peer {
                    return vec![];
                }
            }
        }
        let start_counter = next_back.map(|(id, _)| id.counter).unwrap_or(0);
        let ans = inner
            .mem_parsed_kv
            .range_mut(
                ID::new(id_span.peer, start_counter)..ID::new(id_span.peer, id_span.counter.end),
            )
            .filter_map(|(_id, block)| {
                if block.counter_range.1 < id_span.counter.start {
                    return None;
                }

                if let Err(err) = block.ensure_changes(&self.arena) {
                    warn!(block_id = ?_id, ?err, "failed to parse change block");
                    self.parse_failures.record(*_id, &err);
                    return None;
                }
                let changes = block.content.try_changes().unwrap();
                let start;
                let end;
                if id_span.counter.start <= block.counter_range.0
                    && id_span.counter.end >= block.counter_range.1
                {
                    start = 0;
                    end = changes.len();
                } else {
                    start = block
                        .get_change_index_by_counter(id_span.counter.start)
                        .unwrap_or_else(|x| x);

                    match block.get_change_index_by_counter(id_span.counter.end - 1) {
                        Ok(e) => {
                            end = e + 1;
                        }
                        Err(0) => return None,
                        Err(e) => {
                            end = e;
                        }
                    }
                }
                if start == end {
                    return None;
                }

                Some((block.clone(), start, end))
            })
            // TODO: PERF avoid alloc
            .collect_vec();

        ans
    }

    pub fn iter_changes(&self, id_span: IdSpan) -> impl Iterator<Item = BlockChangeRef> + '_ {
        let v = self.iter_blocks(id_span);
        #[cfg(debug_assertions)]
        {
            if !v.is_empty() {
                assert_eq!(v[0].0.peer, id_span.peer);
                assert_eq!(v.last().unwrap().0.peer, id_span.peer);
                {
                    // Test start
                    let (block, start, _end) = v.first().unwrap();
                    let changes = block.content.try_changes().unwrap();
                    assert!(changes[*start].id.counter <= id_span.counter.start);
                }
                {
                    // Test end
                    let (block, _start, end) = v.last().unwrap();
                    let changes = block.content.try_changes().unwrap();
                    assert!(changes[*end - 1].ctr_end() >= id_span.counter.end);
                    assert!(changes[*end - 1].id.counter < id_span.counter.end);
                }
            }
        }

        v.into_iter().flat_map(move |(block, start, end)| {
            (start..end).map(move |i| BlockChangeRef {
                change_index: i,
                block: block.clone(),
            })
        })
    }

    #[allow(dead_code)]
    pub(crate) fn get_blocks_in_range(&self, id_span: IdSpan) -> VecDeque<Arc<ChangesBlock>> {
        let mut inner = self.inner.lock();
        let start_counter = inner
            .mem_parsed_kv
            .range(..=id_span.id_start())
            .next_back()
            .map(|(id, _)| id.counter)
            .unwrap_or(0);
        let vec = inner
            .mem_parsed_kv
            .range_mut(
                ID::new(id_span.peer, start_counter)..ID::new(id_span.peer, id_span.counter.end),
            )
            .filter_map(|(_id, block)| {
                if block.counter_range.1 < id_span.counter.start {
                    return None;
                }

                if let Err(err) = block.ensure_changes(&self.arena) {
                    warn!(block_id = ?_id, ?err, "failed to parse change block");
                    self.parse_failures.record(*_id, &err);
                    return None;
                }
                Some(block.clone())
            })
            // TODO: PERF avoid alloc
            .collect();
        vec
    }

    pub(crate) fn get_block_that_contains(&self, id: ID) -> Option<Arc<ChangesBlock>> {
        self.ensure_block_loaded_in_range(Bound::Included(id), Bound::Included(id));
        let inner = self.inner.lock();
        let block = inner
            .mem_parsed_kv
            .range(..=id)
            .next_back()
            .filter(|(_, block)| {
                block.peer == id.peer
                    && block.counter_range.0 <= id.counter
                    && id.counter < block.counter_range.1
            })
            .map(|(_, block)| block.clone());

        block
    }

    pub(crate) fn get_the_last_block_of_peer(&self, peer: PeerID) -> Option<Arc<ChangesBlock>> {
        let end_id = ID::new(peer, Counter::MAX);
        self.ensure_id_lte(end_id);
        let inner = self.inner.lock();
        let block = inner
            .mem_parsed_kv
            .range(..=end_id)
            .next_back()
            .filter(|(_, block)| block.peer == peer)
            .map(|(_, block)| block.clone());

        block
    }

    pub fn change_num(&self) -> usize {
        self.ensure_block_loaded_in_range(Bound::Unbounded, Bound::Unbounded);
        let mut inner = self.inner.lock();
        inner
            .mem_parsed_kv
            .iter_mut()
            .map(|(_, block)| block.change_num())
            .sum()
    }

    pub fn fork(
        &self,
        arena: SharedArena,
        merge_interval: Arc<AtomicI64>,
        vv: &VersionVector,
        frontiers: &Frontiers,
    ) -> Self {
        self.flush_and_compact(vv, frontiers);
        let external_kv = self.external_kv.lock().clone_store();
        let inner = self.inner.lock();
        Self {
            inner: Arc::new(Mutex::new(ChangeStoreInner {
                start_vv: inner.start_vv.clone(),
                start_frontiers: inner.start_frontiers.clone(),
                mem_parsed_kv: BTreeMap::new(),
                retired: false,
            })),
            arena,
            external_vv: Arc::new(Mutex::new(self.external_vv.lock().clone())),
            external_kv,
            merge_interval,
            root_history_names: Arc::new(Mutex::new(RootHistoryNamesState::Uninitialized)),
            parse_failures: Default::default(),
            #[cfg(test)]
            root_history_scan_count: Arc::new(AtomicUsize::new(0)),
        }
    }

    pub fn kv_size(&self) -> usize {
        self.external_kv
            .lock()
            .scan(Bound::Unbounded, Bound::Unbounded)
            .map(|(k, v)| k.len() + v.len())
            .sum()
    }

    pub(crate) fn export_blocks_from<W: std::io::Write>(
        &self,
        start_vv: &VersionVector,
        shallow_since_vv: &ImVersionVector,
        latest_vv: &VersionVector,
        w: &mut W,
    ) {
        let new_store = ChangeStore::new_mem(&self.arena, self.merge_interval.clone());
        for mut span in latest_vv.sub_iter(start_vv) {
            let counter_lower_bound = shallow_since_vv.get(&span.peer).copied().unwrap_or(0);
            span.counter.start = span.counter.start.max(counter_lower_bound);
            span.counter.end = span.counter.end.max(counter_lower_bound);
            if span.counter.start >= span.counter.end {
                continue;
            }

            // PERF: this can be optimized by reusing the current encoded blocks
            // In the current method, it needs to parse and re-encode the blocks
            for c in self.iter_changes(span) {
                let start = ((start_vv.get(&c.id.peer).copied().unwrap_or(0) - c.id.counter).max(0)
                    as usize)
                    .min(c.atom_len());
                let end = ((latest_vv.get(&c.id.peer).copied().unwrap_or(0) - c.id.counter).max(0)
                    as usize)
                    .min(c.atom_len());

                assert_ne!(start, end);
                let ch = c.slice(start, end);
                new_store.insert_change(ch, false, false);
            }
        }

        let arena = &self.arena;
        encode_blocks_in_store(new_store, arena, w);
    }

    pub(crate) fn fork_changes_up_to(
        &self,
        start_vv: &ImVersionVector,
        frontiers: &Frontiers,
        vv: &VersionVector,
    ) -> Bytes {
        let new_store = ChangeStore::new_mem(&self.arena, self.merge_interval.clone());
        for mut span in vv.sub_iter_im(start_vv) {
            let counter_lower_bound = start_vv.get(&span.peer).copied().unwrap_or(0);
            span.counter.start = span.counter.start.max(counter_lower_bound);
            span.counter.end = span.counter.end.max(counter_lower_bound);
            if span.counter.start >= span.counter.end {
                continue;
            }

            // PERF: this can be optimized by reusing the current encoded blocks
            // In the current method, it needs to parse and re-encode the blocks
            for c in self.iter_changes(span) {
                let start = ((start_vv.get(&c.id.peer).copied().unwrap_or(0) - c.id.counter).max(0)
                    as usize)
                    .min(c.atom_len());
                let end = ((vv.get(&c.id.peer).copied().unwrap_or(0) - c.id.counter).max(0)
                    as usize)
                    .min(c.atom_len());

                assert_ne!(start, end);
                let ch = c.slice(start, end);
                new_store.insert_change(ch, false, false);
            }
        }

        new_store.encode_all(vv, frontiers)
    }
}

fn encode_blocks_in_store<W: std::io::Write>(
    new_store: ChangeStore,
    arena: &SharedArena,
    w: &mut W,
) {
    let mut inner = new_store.inner.lock();
    for (_id, block) in inner.mem_parsed_kv.iter_mut() {
        let bytes = block.to_bytes(arena);
        leb128::write::unsigned(w, bytes.bytes.len() as u64).unwrap();
        w.write_all(&bytes.bytes).unwrap();
    }
}

mod mut_external_kv {
    //! Only this module contains the code that mutate the external kv store
    //! All other modules should only read from the external kv store
    use super::*;

    impl ChangeStore {
        #[tracing::instrument(skip_all, level = "debug", name = "change_store import_all")]
        pub(crate) fn import_all(&self, bytes: Bytes) -> Result<BatchDecodeInfo, LoroError> {
            let mut kv_store = self.external_kv.lock();
            assert!(
                // 2 because there are vv and frontiers
                kv_store.len() <= 2,
                "kv store should be empty when using decode_all"
            );
            // Snapshot/update bytes are external input. Validate the checksums embedded in each
            // SSTable block as well as the document envelope so malformed lazy blocks are rejected
            // during import instead of surfacing from a later read.
            kv_store
                .import_all(bytes)
                .map_err(|e| LoroError::DecodeError(e.into_boxed_str()))?;
            drop(kv_store);
            *self.root_history_names.lock() = RootHistoryNamesState::Uninitialized;
            let vv_bytes = self.external_kv.lock().get(VV_KEY).unwrap_or_default();
            let vv = VersionVector::decode(&vv_bytes)
                .map_err(|_| LoroError::DecodeDataCorruptionError)?;
            let start_vv_bytes = self
                .external_kv
                .lock()
                .get(START_VV_KEY)
                .unwrap_or_default();
            let start_vv = if start_vv_bytes.is_empty() {
                Default::default()
            } else {
                VersionVector::decode(&start_vv_bytes)
                    .map_err(|_| LoroError::DecodeDataCorruptionError)?
            };

            #[cfg(test)]
            {
                // This is for tests
                for (peer, cnt) in vv.iter() {
                    self.get_change(ID::new(*peer, *cnt - 1))
                        .ok_or(LoroError::DecodeDataCorruptionError)?;
                }
            }

            *self.external_vv.lock() = vv.clone();
            let frontiers_bytes = self
                .external_kv
                .lock()
                .get(FRONTIERS_KEY)
                .unwrap_or_default();
            let frontiers = Frontiers::decode(&frontiers_bytes)
                .map_err(|_| LoroError::DecodeDataCorruptionError)?;
            let start_frontiers = self
                .external_kv
                .lock()
                .get(START_FRONTIERS_KEY)
                .unwrap_or_default();
            let start_frontiers = if start_frontiers.is_empty() {
                Default::default()
            } else {
                Frontiers::decode(&start_frontiers)
                    .map_err(|_| LoroError::DecodeDataCorruptionError)?
            };

            let mut max_lamport = None;
            let mut max_timestamp = 0;
            for id in frontiers.iter() {
                let c = self
                    .get_change(id)
                    .ok_or(LoroError::DecodeDataCorruptionError)?;
                debug_assert_ne!(c.atom_len(), 0);
                let l = c.lamport_last();
                if let Some(x) = max_lamport {
                    if l > x {
                        max_lamport = Some(l);
                    }
                } else {
                    max_lamport = Some(l);
                }

                let t = c.timestamp;
                if t > max_timestamp {
                    max_timestamp = t;
                }
            }

            Ok(BatchDecodeInfo {
                vv,
                frontiers,
                start_version: if start_vv.is_empty() {
                    None
                } else {
                    let mut inner = self.inner.lock();
                    inner.start_frontiers = start_frontiers.clone();
                    inner.start_vv = ImVersionVector::from_vv(&start_vv);
                    Some((start_vv, start_frontiers))
                },
            })
        }

        /// Flush the cached change to kv_store
        pub(crate) fn flush_and_compact(&self, vv: &VersionVector, frontiers: &Frontiers) {
            let mut store = self.external_kv.lock();
            let mut inner = self.inner.lock();
            let mut external_vv = self.external_vv.lock();
            for (id, block) in inner.mem_parsed_kv.iter_mut() {
                if !block.flushed {
                    let id_bytes = id.to_bytes();
                    let counter_start = external_vv.get(&id.peer).copied().unwrap_or(0);
                    assert!(
                        counter_start < block.counter_range.1,
                        "Peer={} Block Counter Range={:?}, counter_start={}",
                        id.peer,
                        &block.counter_range,
                        counter_start
                    );
                    if counter_start > block.counter_range.0 {
                        assert!(store.get(&id_bytes).is_some());
                    }
                    external_vv.insert(id.peer, block.counter_range.1);
                    let bytes = block.to_bytes(&self.arena);
                    store.set(&id_bytes, bytes.bytes);
                    Arc::make_mut(block).flushed = true;
                }
            }

            if inner.start_vv.is_empty() {
                assert_eq!(&*external_vv, vv);
            } else {
                #[cfg(debug_assertions)]
                {
                    // TODO: makes some assertions here?
                }
            }
            let vv_bytes = vv.encode();
            let frontiers_bytes = frontiers.encode();
            store.set(VV_KEY, vv_bytes.into());
            store.set(FRONTIERS_KEY, frontiers_bytes.into());
        }
    }
}

mod mut_inner_kv {
    //! Only this module contains the code that mutate the internal kv store
    //! All other modules should only read from the internal kv store

    use super::*;
    impl ChangeStore {
        /// This method is the **only place** that push a new change into the change store
        ///
        /// The new change either merges with the previous block or is put into a new block.
        /// This method only updates the internal kv store.
        pub fn insert_change(&self, change: Change, split_when_exceeds: bool, is_local: bool) {
            self.insert_change_inner(change, split_when_exceeds, is_local, None);
        }

        pub(crate) fn insert_change_with_rollback(
            &self,
            change: Change,
            split_when_exceeds: bool,
            is_local: bool,
            rollback: &mut ChangeStoreRollback,
        ) {
            self.insert_change_inner(change, split_when_exceeds, is_local, Some(rollback));
        }

        fn insert_change_inner(
            &self,
            mut change: Change,
            split_when_exceeds: bool,
            is_local: bool,
            mut rollback: Option<&mut ChangeStoreRollback>,
        ) {
            self.record_change_in_root_history_names(&change);

            #[cfg(debug_assertions)]
            {
                let vv = self.external_vv.lock();
                assert!(vv.get(&change.id.peer).copied().unwrap_or(0) <= change.id.counter);
            }

            let s = info_span!("change_store insert_change", id = ?change.id);
            let _e = s.enter();
            let estimated_size = change.estimate_storage_size();
            if estimated_size > MAX_BLOCK_SIZE && split_when_exceeds {
                self.split_change_then_insert(change, rollback.as_deref_mut());
                return;
            }

            let id = change.id;
            let mut inner = self.inner.lock();

            // try to merge with previous block
            if let Some((_id, block)) = inner.mem_parsed_kv.range_mut(..id).next_back() {
                if block.peer == change.id.peer {
                    if block.counter_range.1 != change.id.counter {
                        panic!("counter should be continuous")
                    }

                    if let Some(rollback) = &mut rollback {
                        rollback.record_block_before_mutation(*_id, block);
                    }

                    match block.push_change(
                        change,
                        estimated_size,
                        if is_local {
                            // local change should try to merge with previous change when
                            // the timestamp interval <= the `merge_interval`
                            self.merge_interval
                                .load(std::sync::atomic::Ordering::Acquire)
                        } else {
                            0
                        },
                        &self.arena,
                    ) {
                        Ok(_) => {
                            drop(inner);
                            debug_assert!(self.get_change(id).is_some());
                            return;
                        }
                        Err(c) => change = c,
                    }
                }
            }

            inner
                .mem_parsed_kv
                .insert(id, Arc::new(ChangesBlock::new(change, &self.arena)));
            drop(inner);
            debug_assert!(self.get_change(id).is_some());
        }

        pub fn get_change(&self, id: ID) -> Option<BlockChangeRef> {
            let block = self.get_parsed_block(id)?;
            Some(BlockChangeRef {
                change_index: block.get_change_index_by_counter(id.counter).unwrap(),
                block: block.clone(),
            })
        }

        /// Get the change with the given peer and lamport.
        ///
        /// If not found, return the change with the greatest lamport that is smaller than the given lamport.
        pub fn get_change_by_lamport_lte(&self, idlp: IdLp) -> Option<BlockChangeRef> {
            // This method is complicated because we impl binary search on top of the range api
            // It can be simplified
            // Lock order: `external_kv` before `inner`. The scan below may need it.
            let kv_store = self.external_kv.lock();
            let mut inner = self.inner.lock();
            let mut iter = inner
                .mem_parsed_kv
                .range_mut(ID::new(idlp.peer, 0)..ID::new(idlp.peer, i32::MAX));

            // This won't change, we only adjust upper_bound
            let mut lower_bound = 0;
            let mut upper_bound = i32::MAX;
            let mut is_binary_searching = false;
            // The binary search below can stop making progress (e.g. when
            // `(lower_bound + upper_bound) / 2` becomes a fixed point while the
            // target block is missing from `mem_parsed_kv`, or when block
            // metadata is inconsistent). Cap its steps and fall back to the
            // external kv scan, which is always correct.
            let mut binary_search_steps = 0;
            loop {
                if is_binary_searching {
                    binary_search_steps += 1;
                    if binary_search_steps > 128 {
                        warn!(
                            "get_change_by_lamport_lte binary search did not converge; \
                             falling back to kv scan"
                        );
                        break;
                    }
                }
                match iter.next_back() {
                    Some((&id, block)) => {
                        if block.lamport_range.0 <= idlp.lamport
                            && (!is_binary_searching || idlp.lamport < block.lamport_range.1)
                        {
                            if !is_binary_searching
                                && upper_bound != i32::MAX
                                && upper_bound != block.counter_range.1
                            {
                                warn!(
                                    "There is a hole between the last block and the current block"
                                );
                                // There is hole between the last block and the current block
                                // We need to load it from the kv store
                                break;
                            }

                            // Found the block
                            if let Err(err) = block.ensure_changes(&self.arena) {
                                warn!(block_id = ?id, ?err, "failed to parse change block");
                                self.parse_failures.record(id, &err);
                                return None;
                            }
                            let index = block.get_change_index_by_lamport_lte(idlp.lamport)?;
                            return Some(BlockChangeRef {
                                change_index: index,
                                block: block.clone(),
                            });
                        }

                        if is_binary_searching {
                            let mid_bound = (lower_bound + upper_bound) / 2;
                            if block.lamport_range.1 <= idlp.lamport {
                                // Target is larger than the current block (pointed by mid_bound)
                                lower_bound = mid_bound;
                            } else {
                                debug_assert!(
                                    idlp.lamport < block.lamport_range.0,
                                    "{} {:?}",
                                    idlp,
                                    &block.lamport_range
                                );
                                // Target is smaller than the current block (pointed by mid_bound)
                                upper_bound = mid_bound;
                            }

                            let mid_bound = (lower_bound + upper_bound) / 2;
                            iter = inner
                                .mem_parsed_kv
                                .range_mut(ID::new(idlp.peer, 0)..ID::new(idlp.peer, mid_bound));
                        } else {
                            // Test whether we need to switch to binary search by measuring the gap
                            if block.lamport_range.0 - idlp.lamport > MAX_BLOCK_SIZE as Lamport * 8
                            {
                                // Use binary search to find the block
                                upper_bound = id.counter;
                                let mid_bound = (lower_bound + upper_bound) / 2;
                                iter = inner.mem_parsed_kv.range_mut(
                                    ID::new(idlp.peer, 0)..ID::new(idlp.peer, mid_bound),
                                );
                                is_binary_searching = true;
                            }

                            upper_bound = id.counter;
                        }
                    }
                    None => {
                        if !is_binary_searching {
                            break;
                        }

                        let mid_bound = (lower_bound + upper_bound) / 2;
                        lower_bound = mid_bound;
                        if upper_bound - lower_bound <= MAX_BLOCK_SIZE as i32 {
                            // If they are too close, we can just scan the range
                            iter = inner.mem_parsed_kv.range_mut(
                                ID::new(idlp.peer, lower_bound)..ID::new(idlp.peer, upper_bound),
                            );
                            is_binary_searching = false;
                        } else {
                            let mid_bound = (lower_bound + upper_bound) / 2;
                            iter = inner
                                .mem_parsed_kv
                                .range_mut(ID::new(idlp.peer, 0)..ID::new(idlp.peer, mid_bound));
                        }
                    }
                }
            }

            let counter_end = upper_bound;

            // The answer may live only in `mem_parsed_kv` (e.g. local changes
            // that have not been flushed to the external kv store yet) or only
            // in `external_kv` (blocks that were never parsed into memory).
            // Check both and use the block with the greatest start counter:
            // within a peer, lamport grows with counter, so that block holds
            // the greatest matching lamport.
            let mem_block_id: Option<ID> = inner
                .mem_parsed_kv
                .range(ID::new(idlp.peer, 0)..ID::new(idlp.peer, counter_end))
                .rev()
                .find(|(_, block)| block.lamport_range.0 <= idlp.lamport)
                .map(|(id, _)| *id);

            let external_block = 'block_scan: {
                let scan_end = ID::new(idlp.peer, counter_end).to_bytes();
                let iter = kv_store
                    .scan(
                        Bound::Included(&ID::new(idlp.peer, 0).to_bytes()),
                        Bound::Excluded(&scan_end),
                    )
                    .rev();

                for (id, bytes) in iter {
                    let mut block = ChangesBlockBytes::new(bytes.clone());
                    let (lamport_start, _lamport_end) = match block.lamport_range() {
                        Ok(range) => range,
                        Err(err) => {
                            let block_id = ID::from_bytes(&id);
                            warn!(
                                ?block_id,
                                ?err,
                                "failed to decode external change block range"
                            );
                            self.parse_failures.record(block_id, &err);
                            continue;
                        }
                    };
                    if lamport_start <= idlp.lamport {
                        break 'block_scan Some((ID::from_bytes(&id), bytes));
                    }
                }

                None
            };

            let use_external = match (mem_block_id, &external_block) {
                (Some(mem_id), Some((external_id, _))) => external_id.counter > mem_id.counter,
                (Some(_), None) => false,
                (None, Some(_)) => true,
                (None, None) => return None,
            };

            if !use_external {
                let block_id = mem_block_id.unwrap();
                let block = inner.mem_parsed_kv.get_mut(&block_id).unwrap();
                if let Err(err) = block.ensure_changes(&self.arena) {
                    warn!(?block_id, ?err, "failed to parse change block");
                    self.parse_failures.record(block_id, &err);
                    return None;
                }
                let block = block.clone();
                let index = block.get_change_index_by_lamport_lte(idlp.lamport)?;
                return Some(BlockChangeRef {
                    change_index: index,
                    block,
                });
            }

            let (block_id, bytes) = external_block.unwrap();
            let mut block = match ChangesBlock::from_bytes(bytes) {
                Ok(block) => Arc::new(block),
                Err(err) => {
                    warn!(?block_id, ?err, "failed to decode external change block");
                    self.parse_failures.record(block_id, &err);
                    return None;
                }
            };
            if let Err(err) = block.ensure_changes(&self.arena) {
                warn!(?block_id, ?err, "failed to parse external change block");
                self.parse_failures.record(block_id, &err);
                return None;
            }
            inner.mem_parsed_kv.insert(block_id, block.clone());
            let index = block.get_change_index_by_lamport_lte(idlp.lamport)?;
            Some(BlockChangeRef {
                change_index: index,
                block,
            })
        }

        fn split_change_then_insert(
            &self,
            change: Change,
            mut rollback: Option<&mut ChangeStoreRollback>,
        ) {
            let original_len = change.atom_len();
            let mut new_change = Change {
                ops: RleVec::new(),
                deps: change.deps,
                id: change.id,
                lamport: change.lamport,
                timestamp: change.timestamp,
                commit_msg: change.commit_msg.clone(),
            };

            let mut total_len = 0;
            let mut estimated_size = new_change.estimate_storage_size();
            'outer: for mut op in change.ops.into_iter() {
                if op.estimate_storage_size() >= MAX_BLOCK_SIZE - estimated_size {
                    new_change = self._insert_splitted_change(
                        new_change,
                        &mut total_len,
                        &mut estimated_size,
                        rollback.as_deref_mut(),
                    );
                }

                while let Some(end) =
                    op.check_whether_slice_content_to_fit_in_size(MAX_BLOCK_SIZE - estimated_size)
                {
                    // The new op can take the rest of the room
                    let new = op.slice(0, end);
                    new_change.ops.push(new);
                    new_change = self._insert_splitted_change(
                        new_change,
                        &mut total_len,
                        &mut estimated_size,
                        rollback.as_deref_mut(),
                    );

                    if end < op.atom_len() {
                        op = op.slice(end, op.atom_len());
                    } else {
                        continue 'outer;
                    }
                }

                estimated_size += op.estimate_storage_size();
                if estimated_size > MAX_BLOCK_SIZE && !new_change.ops.is_empty() {
                    new_change = self._insert_splitted_change(
                        new_change,
                        &mut total_len,
                        &mut estimated_size,
                        rollback.as_deref_mut(),
                    );
                    new_change.ops.push(op);
                } else {
                    new_change.ops.push(op);
                }
            }

            if !new_change.ops.is_empty() {
                total_len += new_change.atom_len();
                self.insert_change_inner(new_change, false, false, rollback);
            }

            assert_eq!(total_len, original_len);
        }

        fn _insert_splitted_change(
            &self,
            new_change: Change,
            total_len: &mut usize,
            estimated_size: &mut usize,
            rollback: Option<&mut ChangeStoreRollback>,
        ) -> Change {
            if new_change.atom_len() == 0 {
                return new_change;
            }

            let ctr_end = new_change.id.counter + new_change.atom_len() as Counter;
            let next_lamport = new_change.lamport + new_change.atom_len() as Lamport;
            *total_len += new_change.atom_len();
            let ans = Change {
                ops: RleVec::new(),
                deps: ID::new(new_change.id.peer, ctr_end - 1).into(),
                id: ID::new(new_change.id.peer, ctr_end),
                lamport: next_lamport,
                timestamp: new_change.timestamp,
                commit_msg: new_change.commit_msg.clone(),
            };

            self.insert_change_inner(new_change, false, false, rollback);
            *estimated_size = ans.estimate_storage_size();
            ans
        }

        /// A callback for [`SharedArena::set_creator_resolver`]: loads and parses the change
        /// block holding the op with the given ID, if the store has it. Parsing registers the
        /// parent link of every container the block's ops create.
        ///
        /// It holds weak references, so the arena (which this store refers to) does not keep
        /// the store alive.
        ///
        /// Locking: this is the only access to the store that does not hold the document's op
        /// log lock (it runs under the state lock, or under none while events are emitted), so
        /// it relies on the store's lock order (see [`ChangeStore`]). It takes the arena's lock
        /// after the store's, so the arena must call it without holding its own. See
        /// `context/arena-parent-links.md`.
        pub(crate) fn creator_resolver(
            &self,
        ) -> impl Fn(&SharedArena, ID) -> CreatorOp + Send + Sync + 'static {
            let inner = Arc::downgrade(&self.inner);
            let external_kv = Arc::downgrade(&self.external_kv);
            let parse_failures = self.parse_failures.clone();
            move |arena, id| {
                let (Some(inner), Some(external_kv)) = (inner.upgrade(), external_kv.upgrade())
                else {
                    // The op log is gone, and its history with it.
                    return CreatorOp::Absent;
                };
                match Self::load_parsed_block(&inner, &external_kv, arena, id) {
                    Ok(Some(_)) => CreatorOp::Loaded,
                    Ok(None) => CreatorOp::Absent,
                    // Answering "no such op" alone would report the container as deleted,
                    // and a panic here would unwind under the state lock (and trap the
                    // WASM instance). Record the block instead; see [`ParseFailures`].
                    Err((block_id, err)) => {
                        tracing::error!(
                            %block_id, %id, ?err,
                            "cannot parse change block; the document's history is corrupt"
                        );
                        parse_failures.record(block_id, &err);
                        CreatorOp::Corrupt
                    }
                }
            }
        }

        /// `Err` once a read of this store has hit a block it cannot decode or parse. See
        /// [`ParseFailures`].
        pub(crate) fn corrupt_block_error(&self) -> LoroResult<()> {
            match &*self.parse_failures.first.lock() {
                None => Ok(()),
                Some((block_id, err)) => Err(LoroError::DecodeError(
                    format!("cannot parse change block {block_id}: {err}").into_boxed_str(),
                )),
            }
        }

        fn get_parsed_block(&self, id: ID) -> Option<Arc<ChangesBlock>> {
            match Self::load_parsed_block(&self.inner, &self.external_kv, &self.arena, id) {
                Ok(block) => block,
                Err((block_id, err)) => {
                    warn!(?block_id, ?err, "failed to parse change block");
                    self.parse_failures.record(block_id, &err);
                    None
                }
            }
        }

        /// The parsed block holding `id`, `Ok(None)` if the store has no such block, or the ID
        /// of a block of `id`'s peer that cannot be decoded or parsed.
        fn load_parsed_block(
            inner: &Mutex<ChangeStoreInner>,
            external_kv: &Mutex<dyn KvStore>,
            arena: &SharedArena,
            id: ID,
        ) -> Result<Option<Arc<ChangesBlock>>, (ID, LoroError)> {
            // A cached block needs only `inner`.
            {
                let mut inner = inner.lock();
                if inner.retired {
                    return Ok(None);
                }
                if let Some(block) = Self::parse_cached_block(&mut inner, arena, id) {
                    return block.map(Some);
                }
            }

            // Lock order: `external_kv` before `inner`. Another thread may have loaded the
            // block (or retired the store) in between, so look again.
            let store = external_kv.lock();
            let mut inner = inner.lock();
            if inner.retired {
                return Ok(None);
            }
            if let Some(block) = Self::parse_cached_block(&mut inner, arena, id) {
                return block.map(Some);
            }

            let Some((b_id, b_bytes)) = store
                .scan(Bound::Unbounded, Bound::Included(&id.to_bytes()))
                .rfind(|(id, _)| id.len() == 12)
            else {
                return Ok(None);
            };
            let block_id: ID = ID::from_bytes(&b_id[..]);
            if block_id.peer != id.peer {
                return Ok(None);
            }
            let block = ChangesBlock::from_bytes(b_bytes).map_err(|err| (block_id, err))?;
            if block.counter_range.1 <= id.counter {
                return Ok(None);
            }
            let mut block = Arc::new(block);
            block.ensure_changes(arena).map_err(|err| (block_id, err))?;
            inner.mem_parsed_kv.insert(block_id, block.clone());
            Ok(Some(block))
        }

        /// `Some` if a cached block holds `id`: the block, parsed, or why it cannot be parsed.
        fn parse_cached_block(
            inner: &mut ChangeStoreInner,
            arena: &SharedArena,
            id: ID,
        ) -> Option<Result<Arc<ChangesBlock>, (ID, LoroError)>> {
            let (block_id, block) = inner.mem_parsed_kv.range_mut(..=id).next_back()?;
            if block.peer != id.peer || block.counter_range.1 <= id.counter {
                return None;
            }
            Some(match block.ensure_changes(arena) {
                Ok(()) => Ok(block.clone()),
                Err(err) => Err((*block_id, err)),
            })
        }

        /// Load all the blocks that have overlapped with the given ID range into `inner_mem_parsed_kv`
        ///
        /// This is fast because we don't actually parse the content.
        // TODO: PERF: This method feels slow.
        pub(super) fn ensure_block_loaded_in_range(&self, start: Bound<ID>, end: Bound<ID>) {
            let mut whether_need_scan_backward = match start {
                Bound::Included(id) => Some(id),
                Bound::Excluded(id) => Some(id.inc(1)),
                Bound::Unbounded => None,
            };

            {
                let start = start.map(|id| id.to_bytes());
                let end = end.map(|id| id.to_bytes());
                let kv = self.external_kv.lock();
                let mut inner = self.inner.lock();
                for (id, bytes) in kv
                    .scan(
                        start.as_ref().map(|x| x.as_slice()),
                        end.as_ref().map(|x| x.as_slice()),
                    )
                    .filter(|(id, _)| id.len() == 12)
                {
                    let id = ID::from_bytes(&id);
                    if let Some(expected_start_id) = whether_need_scan_backward {
                        if id == expected_start_id {
                            whether_need_scan_backward = None;
                        }
                    }

                    if inner.mem_parsed_kv.contains_key(&id) {
                        continue;
                    }

                    let block = match ChangesBlock::from_bytes(bytes.clone()) {
                        Ok(block) => block,
                        Err(err) => {
                            warn!(?id, ?err, "failed to decode external change block");
                            self.parse_failures.record(id, &err);
                            continue;
                        }
                    };
                    inner.mem_parsed_kv.insert(id, Arc::new(block));
                }
            }

            if let Some(start_id) = whether_need_scan_backward {
                self.ensure_id_lte(start_id);
            }
        }

        pub(super) fn ensure_id_lte(&self, id: ID) {
            let kv = self.external_kv.lock();
            let mut inner = self.inner.lock();
            let Some((next_back_id, next_back_bytes)) = kv
                .scan(Bound::Unbounded, Bound::Included(&id.to_bytes()))
                .rfind(|(id, _)| id.len() == 12)
            else {
                return;
            };

            let next_back_id = ID::from_bytes(&next_back_id);
            if next_back_id.peer == id.peer {
                if inner.mem_parsed_kv.contains_key(&next_back_id) {
                    return;
                }

                let block = match ChangesBlock::from_bytes(next_back_bytes) {
                    Ok(block) => block,
                    Err(err) => {
                        warn!(
                            ?next_back_id,
                            ?err,
                            "failed to decode external change block"
                        );
                        self.parse_failures.record(next_back_id, &err);
                        return;
                    }
                };
                inner.mem_parsed_kv.insert(next_back_id, Arc::new(block));
            }
        }
    }
}

#[must_use]
#[derive(Clone, Debug)]
pub(crate) struct BatchDecodeInfo {
    pub vv: VersionVector,
    pub frontiers: Frontiers,
    pub start_version: Option<(VersionVector, Frontiers)>,
}

#[derive(Clone, Debug)]
pub struct BlockChangeRef {
    block: Arc<ChangesBlock>,
    change_index: usize,
}

impl Deref for BlockChangeRef {
    type Target = Change;
    fn deref(&self) -> &Change {
        &self.block.content.try_changes().unwrap()[self.change_index]
    }
}

impl BlockChangeRef {
    pub(crate) fn get_op_with_counter(&self, counter: Counter) -> Option<BlockOpRef> {
        if counter >= self.ctr_end() {
            return None;
        }

        let index = self.ops.search_atom_index(counter);
        Some(BlockOpRef {
            block: self.block.clone(),
            change_index: self.change_index,
            op_index: index,
        })
    }
}

#[derive(Clone, Debug)]
pub(crate) struct BlockOpRef {
    pub block: Arc<ChangesBlock>,
    pub change_index: usize,
    pub op_index: usize,
}

impl Deref for BlockOpRef {
    type Target = Op;

    fn deref(&self) -> &Op {
        &self.block.content.try_changes().unwrap()[self.change_index].ops[self.op_index]
    }
}

impl BlockOpRef {
    pub fn lamport(&self) -> Lamport {
        let change = &self.block.content.try_changes().unwrap()[self.change_index];
        let op = &change.ops[self.op_index];
        (op.counter - change.id.counter) as Lamport + change.lamport
    }
}

impl ChangesBlock {
    fn from_bytes(bytes: Bytes) -> LoroResult<Self> {
        let len = bytes.len();
        let bytes = ChangesBlockBytes::new(bytes);
        bytes.ensure_header()?;
        let header = bytes
            .header
            .get()
            .expect("header should be initialized after ensure_header");
        let peer = header.peer;
        let counter_range = (
            header.counter,
            *header.counters.last().ok_or_else(|| {
                LoroError::DecodeError("Decode block error: missing counters".into())
            })?,
        );
        // `header.lamports` only stores the start lamport of each change (n entries),
        // while `header.counters` has n + 1 entries ending with the block's end counter.
        // The block's exclusive end lamport is the last change's start lamport plus its len.
        let last_change_start = *header
            .counters
            .len()
            .checked_sub(2)
            .and_then(|i| header.counters.get(i))
            .ok_or_else(|| LoroError::DecodeError("Decode block error: missing counters".into()))?;
        let last_change_len = counter_range.1 - last_change_start;
        let lamport_range = (
            *header.lamports.first().ok_or_else(|| {
                LoroError::DecodeError("Decode block error: missing lamports".into())
            })?,
            header
                .lamports
                .last()
                .ok_or_else(|| {
                    LoroError::DecodeError("Decode block error: missing lamports".into())
                })?
                .checked_add(last_change_len as Lamport)
                .ok_or_else(|| {
                    LoroError::DecodeError("Decode block error: lamport overflow".into())
                })?,
        );
        let content = ChangesBlockContent::Bytes(bytes);
        Ok(Self {
            peer,
            estimated_size: len,
            counter_range,
            lamport_range,
            flushed: true,
            content,
            parsed_extent: ArenaExtent::default(),
        })
    }

    #[allow(dead_code)]
    pub(crate) fn content(&self) -> &ChangesBlockContent {
        &self.content
    }

    fn new(change: Change, _a: &SharedArena) -> Self {
        let atom_len = change.atom_len();
        let counter_range = (change.id.counter, change.id.counter + atom_len as Counter);
        let lamport_range = (change.lamport, change.lamport + atom_len as Lamport);
        let estimated_size = change.estimate_storage_size();
        let peer = change.id.peer;
        let content = ChangesBlockContent::Changes(Arc::new(vec![change]));
        Self {
            peer,
            counter_range,
            lamport_range,
            estimated_size,
            content,
            flushed: false,
            parsed_extent: ArenaExtent::default(),
        }
    }

    #[allow(unused)]
    fn cmp_id(&self, id: ID) -> Ordering {
        self.peer.cmp(&id.peer).then_with(|| {
            if self.counter_range.0 > id.counter {
                Ordering::Greater
            } else if self.counter_range.1 <= id.counter {
                Ordering::Less
            } else {
                Ordering::Equal
            }
        })
    }

    #[allow(unused)]
    fn cmp_idlp(&self, idlp: (PeerID, Lamport)) -> Ordering {
        self.peer.cmp(&idlp.0).then_with(|| {
            if self.lamport_range.0 > idlp.1 {
                Ordering::Greater
            } else if self.lamport_range.1 <= idlp.1 {
                Ordering::Less
            } else {
                Ordering::Equal
            }
        })
    }

    #[allow(unused)]
    fn is_full(&self) -> bool {
        self.estimated_size > MAX_BLOCK_SIZE
    }

    #[allow(clippy::result_large_err)]
    fn push_change(
        self: &mut Arc<Self>,
        change: Change,
        new_change_size: usize,
        merge_interval: i64,
        a: &SharedArena,
    ) -> Result<(), Change> {
        if self.counter_range.1 != change.id.counter {
            return Err(change);
        }

        let atom_len = change.atom_len();
        let next_lamport = change.lamport + atom_len as Lamport;
        let next_counter = change.id.counter + atom_len as Counter;

        let is_full = new_change_size + self.estimated_size > MAX_BLOCK_SIZE;
        let this = Arc::make_mut(self);
        let changes = this.content.changes_mut(a).unwrap();
        let changes = Arc::make_mut(changes);
        match changes.last_mut() {
            Some(last)
                if last.can_merge_right(&change, merge_interval)
                    && (!is_full
                        || (change.ops.len() == 1
                            && last.ops.last().unwrap().is_mergable(&change.ops[0], &()))) =>
            {
                for op in change.ops.into_iter() {
                    let size = op.estimate_storage_size();
                    if !last.ops.push(op) {
                        this.estimated_size += size;
                    }
                }
            }
            _ => {
                if is_full {
                    return Err(change);
                } else {
                    this.estimated_size += new_change_size;
                    changes.push(change);
                }
            }
        }

        this.flushed = false;
        this.counter_range.1 = next_counter;
        this.lamport_range.1 = next_lamport;
        Ok(())
    }

    fn to_bytes(self: &mut Arc<Self>, a: &SharedArena) -> ChangesBlockBytes {
        match &self.content {
            ChangesBlockContent::Bytes(bytes) => bytes.clone(),
            ChangesBlockContent::Both(_, bytes) => {
                let bytes = bytes.clone();
                let this = Arc::make_mut(self);
                this.content = ChangesBlockContent::Bytes(bytes.clone());
                bytes
            }
            ChangesBlockContent::Changes(changes) => {
                let bytes = ChangesBlockBytes::serialize(changes, a);
                let this = Arc::make_mut(self);
                this.content = ChangesBlockContent::Bytes(bytes.clone());
                bytes
            }
        }
    }

    fn ensure_changes(self: &mut Arc<Self>, a: &SharedArena) -> LoroResult<()> {
        match &self.content {
            ChangesBlockContent::Changes(_) => Ok(()),
            ChangesBlockContent::Both(_, _) => Ok(()),
            ChangesBlockContent::Bytes(bytes) => {
                let changes = bytes.parse(a)?;
                let b = bytes.clone();
                let this = Arc::make_mut(self);
                this.content = ChangesBlockContent::Both(Arc::new(changes), b);
                this.parsed_extent = a.extent();
                Ok(())
            }
        }
    }

    fn get_change_index_by_counter(&self, counter: Counter) -> Result<usize, usize> {
        let changes = self.content.try_changes().unwrap();
        changes.binary_search_by(|c| {
            if c.id.counter > counter {
                Ordering::Greater
            } else if (c.id.counter + c.content_len() as Counter) <= counter {
                Ordering::Less
            } else {
                Ordering::Equal
            }
        })
    }

    fn get_change_index_by_lamport_lte(&self, lamport: Lamport) -> Option<usize> {
        let changes = self.content.try_changes().unwrap();
        let r = changes.binary_search_by(|c| {
            if c.lamport > lamport {
                Ordering::Greater
            } else if (c.lamport + c.content_len() as Lamport) <= lamport {
                Ordering::Less
            } else {
                Ordering::Equal
            }
        });

        match r {
            Ok(found) => Some(found),
            Err(idx) => {
                if idx == 0 {
                    None
                } else {
                    Some(idx - 1)
                }
            }
        }
    }

    #[allow(unused)]
    fn id(&self) -> ID {
        ID::new(self.peer, self.counter_range.0)
    }

    pub fn change_num(&self) -> usize {
        match &self.content {
            ChangesBlockContent::Changes(c) => c.len(),
            ChangesBlockContent::Bytes(b) => b.len_changes(),
            ChangesBlockContent::Both(c, _) => c.len(),
        }
    }
}

impl ChangesBlockContent {
    // TODO: PERF: We can use Iter to replace Vec
    pub fn iter_dag_nodes(&self) -> Vec<AppDagNode> {
        let mut dag_nodes = Vec::new();
        match self {
            ChangesBlockContent::Changes(c) | ChangesBlockContent::Both(c, _) => {
                for change in c.iter() {
                    let new_node = AppDagNodeInner {
                        peer: change.id.peer,
                        cnt: change.id.counter,
                        lamport: change.lamport,
                        deps: change.deps.clone(),
                        vv: OnceCell::new(),
                        has_succ: false,
                        len: change.atom_len(),
                    }
                    .into();

                    dag_nodes.push_rle_element(new_node);
                }
            }
            ChangesBlockContent::Bytes(b) => {
                b.ensure_header().unwrap();
                let header = b.header.get().unwrap();
                let n = header.n_changes;
                for i in 0..n {
                    let new_node = AppDagNodeInner {
                        peer: header.peer,
                        cnt: header.counters[i],
                        lamport: header.lamports[i],
                        deps: header.deps_groups[i].clone(),
                        vv: OnceCell::new(),
                        has_succ: false,
                        len: (header.counters[i + 1] - header.counters[i]) as usize,
                    }
                    .into();

                    dag_nodes.push_rle_element(new_node);
                }
            }
        }

        dag_nodes
    }

    /// Note that this method will invalidate the stored bytes
    fn changes_mut(&mut self, a: &SharedArena) -> LoroResult<&mut Arc<Vec<Change>>> {
        match self {
            ChangesBlockContent::Changes(changes) => Ok(changes),
            ChangesBlockContent::Both(changes, _) => {
                *self = ChangesBlockContent::Changes(std::mem::take(changes));
                self.changes_mut(a)
            }
            ChangesBlockContent::Bytes(bytes) => {
                let changes = bytes.parse(a)?;
                *self = ChangesBlockContent::Changes(Arc::new(changes));
                self.changes_mut(a)
            }
        }
    }

    pub(crate) fn try_changes(&self) -> Option<&Vec<Change>> {
        match self {
            ChangesBlockContent::Changes(changes) => Some(changes),
            ChangesBlockContent::Both(changes, _) => Some(changes),
            ChangesBlockContent::Bytes(_) => None,
        }
    }

    #[allow(dead_code)]
    pub(crate) fn len_changes(&self) -> usize {
        match self {
            ChangesBlockContent::Changes(changes) => changes.len(),
            ChangesBlockContent::Both(changes, _) => changes.len(),
            ChangesBlockContent::Bytes(bytes) => bytes.len_changes(),
        }
    }
}

impl std::fmt::Debug for ChangesBlockContent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ChangesBlockContent::Changes(changes) => f
                .debug_tuple("ChangesBlockContent::Changes")
                .field(changes)
                .finish(),
            ChangesBlockContent::Bytes(_bytes) => {
                f.debug_tuple("ChangesBlockContent::Bytes").finish()
            }
            ChangesBlockContent::Both(changes, _bytes) => f
                .debug_tuple("ChangesBlockContent::Both")
                .field(changes)
                .finish(),
        }
    }
}

impl ChangesBlockBytes {
    fn new(bytes: Bytes) -> Self {
        Self {
            header: OnceCell::new(),
            bytes,
        }
    }

    fn ensure_header(&self) -> LoroResult<()> {
        self.header
            .get_or_try_init(|| decode_header(&self.bytes).map(Arc::new))?;
        Ok(())
    }

    fn parse(&self, a: &SharedArena) -> LoroResult<Vec<Change>> {
        self.ensure_header()?;
        let ans: Vec<Change> = decode_block(&self.bytes, a, self.header.get().map(|h| h.as_ref()))?;
        for c in ans.iter() {
            // PERF: This can be made faster (low priority)
            register_container_and_parent_link(a, c)
        }

        Ok(ans)
    }

    fn serialize(changes: &[Change], a: &SharedArena) -> Self {
        let bytes = encode_block(changes, a);
        // TODO: Perf we can calculate header directly without parsing the bytes
        let bytes = ChangesBlockBytes::new(Bytes::from(bytes));
        bytes.ensure_header().unwrap();
        bytes
    }

    fn lamport_range(&mut self) -> LoroResult<(Lamport, Lamport)> {
        if let Some(header) = self.header.get() {
            Ok((header.lamports[0], *header.lamports.last().unwrap()))
        } else {
            decode_block_range(&self.bytes).map(|(_, lamport_range)| lamport_range)
        }
    }

    /// Length of the changes
    fn len_changes(&self) -> usize {
        self.ensure_header().unwrap();
        self.header.get().unwrap().n_changes
    }
}

#[cfg(test)]
mod test {
    use crate::cursor::PosType;
    use crate::{
        loro::ExportMode, oplog::convert_change_to_remote, state::TreeParentId, ListHandler,
        LoroDoc, MovableListHandler, TextHandler, TreeHandler,
    };

    use super::*;

    fn test_encode_decode(doc: LoroDoc) {
        doc.commit_then_renew();
        let oplog = doc.oplog().lock();
        let bytes = oplog
            .change_store
            .encode_all(oplog.vv(), oplog.dag.frontiers());
        let store = ChangeStore::new_for_test();
        let _ = store.import_all(bytes.clone()).unwrap();
        assert_eq!(store.external_kv.lock().export_all(), bytes);
        let mut changes_parsed = Vec::new();
        let a = store.arena.clone();
        store.visit_all_changes(&mut |c| {
            changes_parsed.push(convert_change_to_remote(&a, c));
        });
        let mut changes = Vec::new();
        oplog.change_store.visit_all_changes(&mut |c| {
            changes.push(convert_change_to_remote(&oplog.arena, c));
        });
        assert_eq!(changes_parsed, changes);
    }

    #[test]
    fn identical_encoded_block_stays_lazy_and_dirty_cache_shadows_kv() {
        let source = LoroDoc::new_auto_commit();
        source.set_peer_id(1).unwrap();
        source.set_change_merge_interval(-1);
        for _ in 0..40 {
            let text = source.get_text("t");
            text.insert(text.len_unicode(), "abcd", PosType::Unicode)
                .unwrap();
            source.commit_then_renew();
        }
        let bytes = {
            let oplog = source.oplog().lock();
            oplog.change_store.encode_all(oplog.vv(), oplog.frontiers())
        };
        let store = ChangeStore::new_for_test();
        store.import_all(bytes).unwrap();
        let (id, block_bytes) = store
            .external_kv
            .lock()
            .scan(Bound::Unbounded, Bound::Unbounded)
            .find(|(key, _)| key.len() == 12)
            .map(|(key, bytes)| (ID::from_bytes(&key), bytes))
            .unwrap();
        assert!(!store.inner.lock().mem_parsed_kv.contains_key(&id));
        let before = store.arena.utf16_len();
        assert!(store
            .decode_update_block(block_bytes.clone(), &source.oplog_vv())
            .unwrap()
            .is_empty());
        assert_eq!(store.arena.utf16_len(), before);
        assert!(!store.inner.lock().mem_parsed_kv.contains_key(&id));

        let original = store.get_change(id).unwrap();
        let mut dirty = original.block.as_ref().clone();
        // Model an unflushed mutation. Its stale KV copy must not authorize a
        // shortcut, even if the imported bytes match that old copy exactly.
        let mut changes = dirty.content.try_changes().unwrap().clone();
        changes[0].deps = Frontiers::from_id(ID::new(9, 0));
        dirty.content = ChangesBlockContent::Changes(Arc::new(changes));
        dirty.flushed = false;
        store.inner.lock().mem_parsed_kv.insert(id, Arc::new(dirty));
        assert!(!store.contains_encoded_block(id, &block_bytes));
        assert!(!store
            .decode_update_block(block_bytes, &source.oplog_vv())
            .unwrap()
            .is_empty());
    }

    #[test]
    fn merged_cold_text_overlap_is_accepted_without_parsing_the_known_block() {
        let history = LoroDoc::new_auto_commit();
        history.set_peer_id(1).unwrap();
        history.set_change_merge_interval(-1);
        for i in 0..40 {
            history.get_text("t").insert_unicode(i * 2, "a😀").unwrap();
            history.commit_then_renew();
        }
        let target = LoroDoc::new_auto_commit();
        target
            .import(&history.export(ExportMode::Snapshot).unwrap())
            .unwrap();

        let merged = LoroDoc::new_auto_commit();
        merged.set_peer_id(1).unwrap();
        merged
            .get_text("t")
            .insert_unicode(0, &("a😀".repeat(40) + "Z"))
            .unwrap();
        merged.commit_then_renew();
        {
            let incoming = merged.oplog().lock();
            let change = (*incoming.get_change_at(ID::new(1, 0)).unwrap()).clone();
            let local = target.oplog().lock();
            // cfg(test) loads the frontier block at snapshot import; explicitly
            // clear that cache to exercise the real cold-block comparison.
            local.change_store.inner.lock().mem_parsed_kv.clear();
            let id = ID::new(1, 0);
            let bytes = local.change_store.unparsed_block_bytes(id).unwrap();
            assert_ne!(
                bytes.as_ref(),
                block_encode::encode_block(&[change.clone()], &incoming.arena)
            );
            let suffix = local
                .check_and_trim_known_part_in_arena(
                    vec![change],
                    crate::oplog::known_history::ImportedValues::Exact,
                    &incoming.arena,
                )
                .unwrap();
            assert_eq!(suffix.len(), 1);
            assert_eq!(suffix[0].id, ID::new(1, 80));
            // A general comparison would have populated the parsed cache.
            assert!(local.change_store.unparsed_block_bytes(id).is_some());
        }
        target
            .import(&merged.export(ExportMode::all_updates()).unwrap())
            .unwrap();
        assert_eq!(target.get_deep_value(), merged.get_deep_value());
        assert_eq!(target.oplog_vv(), merged.oplog_vv());
    }

    #[test]
    fn repeated_snapshot_updates_allocate_only_the_new_text_and_list_values() {
        let source = LoroDoc::new_auto_commit();
        source.set_peer_id(1).unwrap();
        source.set_change_merge_interval(-1);
        for i in 0..40 {
            let text = source.get_text("t");
            text.insert(text.len_unicode(), "wörld 😀", PosType::Unicode)
                .unwrap();
            source.get_list("l").push(i).unwrap();
            source.commit_then_renew();
        }
        let target = LoroDoc::new_auto_commit();
        target
            .import(&source.export(ExportMode::Snapshot).unwrap())
            .unwrap();
        // Once local history is parsed, repeated snapshots must not allocate
        // another copy of any known strings or list values, even with Unicode.
        target
            .oplog()
            .lock()
            .change_store
            .visit_all_changes(&mut |_| {});
        let text_before = target.oplog().lock().arena.utf16_len();
        let value_count = |doc: &LoroDoc| doc.oplog().lock().arena.extent().values_for_test();
        let values_before = value_count(&target);
        for i in 0..4 {
            let text = source.get_text("t");
            text.insert(text.len_unicode(), "Z", PosType::Unicode)
                .unwrap();
            source.get_list("l").push(100 + i).unwrap();
            source.commit_then_renew();
            target
                .import(&source.export(ExportMode::Snapshot).unwrap())
                .unwrap();
            assert_eq!(
                target.oplog().lock().arena.utf16_len(),
                text_before + i as usize + 1
            );
            assert_eq!(value_count(&target), values_before + i as usize + 1);
        }
        assert_eq!(target.get_deep_value(), source.get_deep_value());
    }

    #[test]
    fn decoded_block_lamport_range_matches_counter_range() {
        // Regression test for a checkout hang after snapshot import.
        // `ChangesBlock::from_bytes` used the start lamport of the block's last
        // change as the block's end lamport, producing degenerate lamport
        // ranges (empty for single-change blocks). The binary search in
        // `get_change_by_lamport_lte` then misclassified the block containing
        // the target lamport and looped forever.
        let doc = LoroDoc::new_auto_commit();
        doc.set_peer_id(1).unwrap();
        // One big commit that splits into many blocks, with enough ops that
        // lamport lookups engage the binary search path
        // (lamport gap > MAX_BLOCK_SIZE * 8).
        for i in 0..100 {
            let text = doc.get_text(format!("t{i}").as_str());
            text.insert(0, &"x".repeat(30), PosType::Unicode).unwrap();
        }
        doc.commit_then_renew();

        let (bytes, end_counter) = {
            let oplog = doc.oplog().lock();
            let end = oplog.vv().get(&1).copied().unwrap();
            let bytes = oplog
                .change_store
                .encode_all(oplog.vv(), oplog.dag.frontiers());
            (bytes, end)
        };

        let store = ChangeStore::new_for_test();
        let _ = store.import_all(bytes).unwrap();
        // Parse every block out of the external kv store
        let mut c = 0;
        while c < end_counter {
            let change = store.get_change(ID::new(1, c)).unwrap();
            c = change.id.counter + change.atom_len() as Counter;
        }

        {
            let inner = store.inner.lock();
            assert!(
                inner.mem_parsed_kv.len() > 1,
                "the change should be split into multiple blocks"
            );
            for (id, block) in inner.mem_parsed_kv.iter() {
                // Single-peer linear history: lamport == counter for every op
                assert_eq!(id.counter, block.counter_range.0);
                assert_eq!(block.lamport_range.0 as Counter, block.counter_range.0);
                assert_eq!(block.lamport_range.1 as Counter, block.counter_range.1);
            }
        }

        for l in (0..end_counter as Lamport).step_by(7) {
            let change = store.get_change_by_lamport_lte(IdLp::new(1, l)).unwrap();
            assert!(change.lamport <= l);
            assert!(l < change.lamport + change.atom_len() as Lamport);
        }
    }

    /// Peer 1 history split into many blocks, encoded into a fresh store's KV only
    /// (nothing parsed into `mem_parsed_kv`), plus the change that comes next.
    fn kv_only_store_and_next_change() -> (ChangeStore, Counter, Change) {
        let doc = LoroDoc::new_auto_commit();
        doc.set_peer_id(1).unwrap();
        for i in 0..100 {
            let text = doc.get_text(format!("t{i}").as_str());
            text.insert(0, &"x".repeat(30), PosType::Unicode).unwrap();
            doc.commit_then_renew();
        }
        let (bytes, end) = {
            let oplog = doc.oplog().lock();
            let end = oplog.vv().get(&1).copied().unwrap();
            let bytes = oplog
                .change_store
                .encode_all(oplog.vv(), oplog.dag.frontiers());
            (bytes, end)
        };
        doc.get_text("t0").insert(0, "y", PosType::Unicode).unwrap();
        doc.commit_then_renew();
        let next = {
            let oplog = doc.oplog().lock();
            let change = oplog.get_change_at(ID::new(1, end)).unwrap();
            (*change).clone()
        };
        let store = ChangeStore::new_for_test();
        let _ = store.import_all(bytes).unwrap();
        // Test builds of `import_all` parse each peer's last block; release builds do not.
        store.inner.lock().mem_parsed_kv.clear();
        (store, end, next)
    }

    #[test]
    fn lamport_lookup_reads_kv_only_blocks() {
        // Regression: `decode_block_range` read a version varint the block encoding
        // does not have, shifting every field. KV-only blocks that did not start at
        // counter 0 were skipped and block `0@P` got its lamport length as its start.
        let (store, end, _) = kv_only_store_and_next_change();
        for l in (0..end as Lamport).step_by(13) {
            store.inner.lock().mem_parsed_kv.clear();
            let change = store
                .get_change_by_lamport_lte(IdLp::new(1, l))
                .unwrap_or_else(|| panic!("lamport {l} should be found"));
            assert!(change.lamport <= l);
            assert!(l < change.lamport + change.atom_len() as Lamport);
        }
    }

    #[test]
    fn rollback_drops_only_blocks_parsed_after_the_checkpoint() {
        // A failed import must not keep blocks it parsed (they may refer to what the arena
        // rollback drops), but the blocks parsed before it are still valid, and reparsing them
        // after every failed import would cost a full history pass.
        let (store, end, _) = kv_only_store_and_next_change();
        let is_parsed = |id: ID| {
            let inner = store.inner.lock();
            let (_, block) = inner.mem_parsed_kv.range(..=id).next_back().unwrap();
            matches!(block.content, ChangesBlockContent::Both(..))
        };
        let early = ID::new(1, 0);
        let late = ID::new(1, end - 1);
        store.get_change(early).unwrap();
        let checkpoint = store.arena.checkpoint_for_rollback();
        // Something the failed import registered.
        store.arena.register_container(&ContainerID::new_root(
            "new",
            loro_common::ContainerType::Map,
        ));
        store.get_change(late).unwrap();
        assert!(is_parsed(early) && is_parsed(late));
        store.rollback_arena(checkpoint);
        assert!(is_parsed(early));
        assert!(!is_parsed(late));
        assert!(store.get_change(late).is_some());
    }

    #[test]
    fn a_retired_store_resolves_nothing() {
        // A resolver that reached the store before a failed snapshot import replaced it
        // must not register containers of the discarded history.
        let (store, _, _) = kv_only_store_and_next_change();
        let resolve = store.creator_resolver();
        let checkpoint = store.arena.checkpoint_for_rollback();
        assert_eq!(resolve(&store.arena, ID::new(1, 0)), CreatorOp::Loaded);
        store.retire(checkpoint);
        assert_eq!(resolve(&store.arena, ID::new(1, 0)), CreatorOp::Absent);
        assert!(store.inner.lock().mem_parsed_kv.is_empty());
    }

    /// Truncates the stored bytes of the block holding `id`: its header still names the
    /// counter range, but the body no longer parses.
    fn truncate_block(store: &ChangeStore, id: ID) {
        let mut kv = store.external_kv.lock();
        let (key, bytes) = kv
            .scan(Bound::Unbounded, Bound::Included(&id.to_bytes()))
            .rfind(|(key, _)| key.len() == 12)
            .unwrap();
        assert_eq!(ID::from_bytes(&key).peer, id.peer);
        kv.set(&key, bytes.slice(..bytes.len() / 2));
        drop(kv);
        store.inner.lock().mem_parsed_kv.clear();
    }

    #[test]
    fn the_creator_resolver_reports_an_unparsable_block_instead_of_panicking() {
        let (store, _, _) = kv_only_store_and_next_change();
        truncate_block(&store, ID::new(1, 0));
        let resolve = store.creator_resolver();
        assert!(store.corrupt_block_error().is_ok());
        assert_eq!(resolve(&store.arena, ID::new(1, 0)), CreatorOp::Corrupt);
        let err = store.corrupt_block_error().unwrap_err();
        assert!(
            matches!(&err, LoroError::DecodeError(msg) if msg.contains("cannot parse change block")),
            "{err}"
        );
    }

    #[test]
    fn every_reader_records_an_unparsable_block() {
        let (store, end, _) = kv_only_store_and_next_change();
        truncate_block(&store, ID::new(1, 0));
        // Readers of the other blocks do not record anything.
        assert!(store.get_change(ID::new(1, end - 1)).is_some());
        assert!(store.corrupt_block_error().is_ok());
        assert!(store.get_change(ID::new(1, 0)).is_none());
        assert!(store.corrupt_block_error().is_err());

        let (store, _, _) = kv_only_store_and_next_change();
        truncate_block(&store, ID::new(1, 0));
        let mut visited = 0;
        store.visit_all_changes(&mut |_| visited += 1);
        assert!(visited > 0);
        assert!(store.corrupt_block_error().is_err());

        let (store, _, _) = kv_only_store_and_next_change();
        truncate_block(&store, ID::new(1, 0));
        assert!(store.get_change_by_lamport_lte(IdLp::new(1, 0)).is_none());
        assert!(store.corrupt_block_error().is_err());
    }

    /// A doc loaded from a snapshot whose history holds the metadata of a tree node that was
    /// created under a deleted parent: only the change that created the node knows the
    /// container (loro-dev/loro#1158), and that change is in a block that is not parsed at
    /// load. Returns the snapshot, the metadata id, and the peer that created the node.
    fn snapshot_with_a_container_only_the_history_knows() -> (Vec<u8>, ContainerID, PeerID) {
        let a = LoroDoc::new_auto_commit();
        a.set_peer_id(1).unwrap();
        let tree = a.get_tree("tree");
        let parent = tree.create(TreeParentId::Root).unwrap();
        a.commit_then_renew();
        let b = a.fork();
        b.set_peer_id(2).unwrap();
        let child = b
            .get_tree("tree")
            .create(TreeParentId::Node(parent))
            .unwrap();
        b.commit_then_renew();
        // Test builds parse each peer's last block at load, so push the create out of it.
        for _ in 0..60 {
            b.get_text("filler")
                .insert(0, &"x".repeat(30), PosType::Unicode)
                .unwrap();
            b.commit_then_renew();
        }
        tree.delete(parent).unwrap();
        a.commit_then_renew();
        a.import(&b.export(ExportMode::all_updates()).unwrap())
            .unwrap();
        let snapshot = a.export(ExportMode::Snapshot).unwrap();
        (snapshot, child.associated_meta_container(), 2)
    }

    #[test]
    fn a_doc_with_an_unparsable_block_neither_panics_nor_exports_partial_history() {
        let (snapshot, meta, peer) = snapshot_with_a_container_only_the_history_knows();
        let load = || {
            let doc = LoroDoc::new();
            doc.import(&snapshot).unwrap();
            doc
        };
        // With the block intact, the lookup finds the container through the history.
        assert!(load().has_container(&meta));

        let doc = load();
        truncate_block(&doc.oplog().lock().change_store, ID::new(peer, 0));
        let value = doc.get_deep_value();
        // The same lookup reaches the creator resolver, which used to panic under the state
        // lock. It now answers "not a container" and records the block.
        assert!(!doc.has_container(&meta));
        assert_eq!(doc.get_deep_value(), value);

        // Entry points that return a `Result` refuse to work from the partial history.
        let is_corrupt = |err: LoroError| {
            assert!(
                err.to_string().contains("cannot parse change block"),
                "{err}"
            );
        };
        is_corrupt(
            doc.checkout(&Frontiers::from(ID::new(peer, 10)))
                .unwrap_err(),
        );
        is_corrupt(
            doc.diff(&Frontiers::from(ID::new(peer, 10)), &doc.oplog_frontiers())
                .unwrap_err(),
        );
        is_corrupt(doc.import(&snapshot).unwrap_err());
        is_corrupt(doc.fork_at(&doc.oplog_frontiers()).map(|_| ()).unwrap_err());
        assert!(doc.export(ExportMode::all_updates()).is_err());
        assert!(doc.export(ExportMode::Snapshot).is_err());
        assert!(LoroDoc::new().merge(&doc).is_err());
        assert_eq!(doc.get_deep_value(), value);
    }

    #[test]
    fn a_recorded_parse_failure_does_not_panic_where_no_error_can_be_returned() {
        // `undo`, `checkout_to_latest` and a detached `fork` run the same checkout and export
        // code as the public entry points, and `unwrap` its result. So the recorded failure
        // must only be checked by the entry points that return it. It is recorded by hand
        // here: with a block that is really broken, these calls can still panic when the DAG
        // needs a node of that block, as they did before the failure was recorded at all.
        let (snapshot, _, _) = snapshot_with_a_container_only_the_history_knows();
        let doc = LoroDoc::new();
        doc.import(&snapshot).unwrap();
        doc.set_peer_id(9).unwrap();
        doc.start_auto_commit();
        let undo = crate::UndoManager::new(&doc);
        doc.get_text("new")
            .insert(0, "edit", PosType::Unicode)
            .unwrap();
        doc.commit_then_renew();
        let latest = doc.oplog_frontiers();
        doc.oplog()
            .lock()
            .change_store
            .parse_failures
            .record(ID::new(2, 0), &LoroError::DecodeDataCorruptionError);

        assert!(doc.checkout(&latest).is_err());
        assert!(doc.export(ExportMode::Snapshot).is_err());
        assert!(doc.fork_at(&latest).is_err());
        assert!(doc.diff(&latest, &latest).is_err());
        assert!(doc.revert_to(&latest).is_err());

        doc.detach();
        let fork = doc.fork();
        assert_eq!(fork.get_deep_value(), doc.get_deep_value());
        doc.checkout_to_latest();
        assert!(!doc.is_detached());
        assert_eq!(doc.fork().get_deep_value(), doc.get_deep_value());
        assert!(undo.undo().unwrap());
        assert_eq!(doc.get_text("new").to_string(), "");
    }

    #[test]
    fn an_export_that_finds_an_unparsable_block_fails_instead_of_skipping_it() {
        // Nothing has recorded the block yet, so the export starts; it must not return
        // updates that silently leave the block's changes out.
        let (snapshot, _, peer) = snapshot_with_a_container_only_the_history_knows();
        let doc = LoroDoc::new();
        doc.import(&snapshot).unwrap();
        truncate_block(&doc.oplog().lock().change_store, ID::new(peer, 0));
        let err = doc.export(ExportMode::all_updates()).unwrap_err();
        assert!(
            err.to_string().contains("cannot parse change block"),
            "{err}"
        );
    }

    #[test]
    fn rollback_evicts_older_blocks_cached_during_the_scope() {
        // An import creates the peer's newest block without loading the older ones,
        // then a read caches an old KV block (the element lookup of the movable-list
        // validator does this). Rolling back removes the newest block; the cached old
        // block must not end up right before the next insert of the same change.
        let (store, end, next) = kv_only_store_and_next_change();
        let mut old_vv = VersionVector::new();
        old_vv.insert(1, end);
        let mut rollback = ChangeStoreRollback::new(old_vv);
        let arena = store.arena.checkpoint_for_rollback();
        store.insert_change_with_rollback(next.clone(), true, false, &mut rollback);
        assert!(store.get_change(ID::new(1, 0)).is_some());
        store.rollback_import(rollback, arena);
        store.insert_change(next, true, false);
        assert!(store.get_change(ID::new(1, end)).is_some());
        assert!(store.get_change(ID::new(1, 0)).is_some());
    }

    #[test]
    fn rollback_restores_unflushed_block_that_was_appended_to() {
        // The rollback keeps only the shape of a block an import appended to and
        // truncates back to it. Appends both push changes and merge ops into the last
        // change, so the restored store must encode exactly like one that never saw
        // the rolled-back changes.
        let doc = LoroDoc::new_auto_commit();
        doc.set_peer_id(1).unwrap();
        // Keep the commits as separate changes; the store below merges them itself.
        doc.set_change_merge_interval(-1);
        let text = doc.get_text("t");
        for i in 0..40 {
            text.insert(i, "a", PosType::Unicode).unwrap();
            if i % 3 == 0 {
                doc.get_map("m").insert("k", i as i64).unwrap();
            }
            doc.commit_then_renew();
        }
        let mut changes = Vec::new();
        let arena = {
            let oplog = doc.oplog().lock();
            oplog
                .change_store
                .visit_all_changes(&mut |c| changes.push(c.clone()));
            oplog.arena.clone()
        };
        // The changes reference the doc's containers and values.
        let new_store = || ChangeStore::new_mem(&arena, Arc::new(AtomicI64::new(0)));
        assert!(changes.len() > 10);
        let (first, rest) = changes.split_at(changes.len() / 2);
        let vv_of = |cs: &[Change]| {
            let mut vv = VersionVector::new();
            vv.insert(1, cs.last().unwrap().ctr_end());
            vv
        };

        let rolled_back = new_store();
        for c in first {
            rolled_back.insert_change(c.clone(), true, false);
        }
        let mut rollback = ChangeStoreRollback::new(vv_of(first));
        let arena_checkpoint = arena.checkpoint_for_rollback();
        for c in &rest[..rest.len() / 2] {
            rolled_back.insert_change_with_rollback(c.clone(), true, false, &mut rollback);
        }
        rolled_back.rollback_import(rollback, arena_checkpoint);
        assert!(rolled_back
            .get_change(ID::new(1, vv_of(first)[&1]))
            .is_none());
        for c in rest {
            rolled_back.insert_change(c.clone(), true, false);
        }

        let direct = new_store();
        for c in &changes {
            direct.insert_change(c.clone(), true, false);
        }
        let vv = vv_of(&changes);
        let frontiers = Frontiers::from_id(changes.last().unwrap().id_last());
        assert_eq!(
            rolled_back.encode_all(&vv, &frontiers),
            direct.encode_all(&vv, &frontiers)
        );
    }

    #[test]
    fn lamport_lookup_finds_unflushed_mem_blocks() {
        // Regression: when the lamport binary search bails out, the fallback
        // used to scan only the external kv store. Local changes may exist
        // solely in `mem_parsed_kv` before any flush, so a lookup targeting a
        // lamport gap incorrectly returned `None`.
        let doc = LoroDoc::new_auto_commit();
        doc.set_peer_id(1).unwrap();
        let text = doc.get_text("t");
        text.insert(0, &"x".repeat(500), PosType::Unicode).unwrap();
        doc.commit_then_renew();

        // An independent large change from peer 2 pushes peer 1's next
        // lamport far above its first change (gap > MAX_BLOCK_SIZE * 8, so
        // lookups below engage the binary search path).
        let doc2 = LoroDoc::new_auto_commit();
        doc2.set_peer_id(2).unwrap();
        let text2 = doc2.get_text("t2");
        text2
            .insert(0, &"y".repeat(3000), PosType::Unicode)
            .unwrap();
        doc2.commit_then_renew();
        doc.import(&doc2.export(ExportMode::all_updates()).unwrap())
            .unwrap();

        let text = doc.get_text("t");
        text.insert(0, &"z".repeat(500), PosType::Unicode).unwrap();
        doc.commit_then_renew();

        // Query a lamport inside the gap between peer 1's two changes,
        // without flushing the change store. The answer (the tail of peer 1's
        // first commit) lives only in `mem_parsed_kv`.
        let oplog = doc.oplog().lock();
        let change = oplog
            .change_store
            .get_change_by_lamport_lte(IdLp::new(1, 700))
            .expect("the change should be found in the unflushed mem blocks");
        assert_eq!(change.id.peer, 1);
        assert!(change.lamport <= 700);
        assert_eq!(change.id.counter + change.atom_len() as Counter, 500);
    }

    #[test]
    fn root_history_names_reject_oversized_name_without_retaining_it() {
        let mut names = FxHashSet::default();
        let mut name_bytes = 0;
        let oversized_name = "x".repeat(MAX_ROOT_HISTORY_NAME_BYTES + 1);
        let oversized = ContainerID::new_root(&oversized_name, crate::ContainerType::Map);

        assert!(!record_root_name(&mut names, &mut name_bytes, &oversized));
        assert!(names.is_empty());
        assert_eq!(name_bytes, 0);
    }

    #[test]
    fn test_change_store() {
        let doc = LoroDoc::new_auto_commit();
        doc.set_record_timestamp(true);
        let t = doc.get_text("t");
        t.insert(0, "hello", PosType::Unicode).unwrap();
        doc.commit_then_renew();
        let t = doc.get_list("t");
        t.insert(0, "hello").unwrap();
        test_encode_decode(doc);
    }

    #[test]
    fn test_synced_doc() -> LoroResult<()> {
        let doc_a = LoroDoc::new_auto_commit();
        let doc_b = LoroDoc::new_auto_commit();
        let doc_c = LoroDoc::new_auto_commit();

        {
            // A: Create initial structure
            let map = doc_a.get_map("root");
            map.insert_container("text", TextHandler::new_detached())?;
            map.insert_container("list", ListHandler::new_detached())?;
            map.insert_container("tree", TreeHandler::new_detached())?;
        }

        {
            // Sync initial state to B and C
            let initial_state = doc_a.export(ExportMode::all_updates()).unwrap();
            doc_b.import(&initial_state)?;
            doc_c.import(&initial_state)?;
        }

        {
            // B: Edit text and list
            let map = doc_b.get_map("root");
            let text = map
                .insert_container("text", TextHandler::new_detached())
                .unwrap();
            text.insert(0, "Hello, ", PosType::Unicode)?;

            let list = map
                .insert_container("list", ListHandler::new_detached())
                .unwrap();
            list.push("world")?;
        }

        {
            // C: Edit tree and movable list
            let map = doc_c.get_map("root");
            let tree = map
                .insert_container("tree", TreeHandler::new_detached())
                .unwrap();
            let node_id = tree.create(TreeParentId::Root)?;
            tree.get_meta(node_id)?.insert("key", "value")?;
            let node_b = tree.create(TreeParentId::Root)?;
            tree.move_to(node_b, TreeParentId::Root, 0).unwrap();

            let movable_list = map
                .insert_container("movable", MovableListHandler::new_detached())
                .unwrap();
            movable_list.push("item1".into())?;
            movable_list.push("item2".into())?;
            movable_list.mov(0, 1)?;
        }

        // Sync B's changes to A
        let b_changes = doc_b
            .export(ExportMode::updates(&doc_a.oplog_vv()))
            .unwrap();
        doc_a.import(&b_changes)?;

        // Sync C's changes to A
        let c_changes = doc_c
            .export(ExportMode::updates(&doc_a.oplog_vv()))
            .unwrap();
        doc_a.import(&c_changes)?;

        test_encode_decode(doc_a);
        Ok(())
    }
}

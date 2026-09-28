mod str_arena;
use self::str_arena::{StrArena, StrArenaCheckpoint};
use crate::sync::{Mutex, MutexGuard, RwLock, RwLockWriteGuard};
use crate::{
    change::Lamport,
    container::{
        idx::ContainerIdx,
        list::list_op::{InnerListOp, ListOp},
        map::MapSet,
        ContainerID,
    },
    id::Counter,
    op::{InnerContent, ListSlice, Op, RawOp, RawOpContent, SliceRange},
    LoroValue,
};
use append_only_bytes::BytesSlice;
use loro_common::{PeerID, ID};
use rustc_hash::FxHashMap;
use std::fmt;
use std::{
    num::NonZeroU16,
    ops::{Range, RangeBounds},
    sync::Arc,
};

pub(crate) struct LoadAllFlag;
type ParentResolver = dyn Fn(ContainerID) -> Option<ContainerID> + Send + Sync + 'static;
/// Loads the change holding the op with the given ID, if the op log has it. Parsing a change
/// registers the parent link of every container its ops create, and a normal container's ID is
/// the ID of the op that created it. It panics if the op log has the change but cannot parse
/// it. See `context/arena-parent-links.md`.
type CreatorResolver = dyn Fn(&SharedArena, ID) -> CreatorOp + Send + Sync + 'static;

/// What the op log's history knows about an op ID, answered by the [`CreatorResolver`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CreatorOp {
    /// The change holding the op is parsed, so the parent of every container the op creates
    /// is registered.
    Loaded,
    /// The history has no op with this ID: it was not received or committed yet, or it is
    /// before the shallow root.
    Absent,
}

#[derive(Default)]
struct ArenaContainers {
    container_idx_to_id: Vec<ContainerID>,
    /// Cached container depth. `None` means unknown or waiting on an unknown parent depth.
    /// Use `get_depth()` for authoritative reads; direct access is only a cache fast path.
    depth: Vec<Option<NonZeroU16>>,
    container_id_to_idx: FxHashMap<ContainerID, ContainerIdx>,
    /// The parent of each container.
    parents: FxHashMap<ContainerIdx, Option<ContainerIdx>>,
    /// All retention roots: top-level user roots **and** mergeable cids. Used by
    /// alive-container / shallow-snapshot retention walks that must see both.
    root_c_idx: Vec<ContainerIdx>,
    /// Subset of `root_c_idx` containing only top-level (non-mergeable) roots. This is the
    /// list user-facing APIs enumerate (`preferred_root_containers`, `get_value`,
    /// `get_deep_value`, jsonpath, ...). Keeping it pre-filtered means those paths do
    /// not pay a per-mergeable `is_mergeable()` parse on every call.
    top_level_root_c_idx: Vec<ContainerIdx>,
    /// Optional resolver used when querying parent for a container that has not been registered yet.
    /// If set, `get_parent` will try this resolver to lazily fetch and register the parent.
    ///
    /// Locking: the resolver may read the state KV store. Code that loads from KV must snapshot
    /// KV data and release the KV lock before taking the arena lock.
    parent_resolver: Option<Arc<ParentResolver>>,
    /// Set by the op log. Used after `parent_resolver` when a container's parent is still
    /// unknown, e.g. the meta of a tree node that was never alive in a document loaded from a
    /// snapshot, whose change blocks are parsed lazily.
    ///
    /// Locking: it takes the change store's locks and parses changes, which registers
    /// containers in this arena, so it must be called without holding the arena lock. It runs
    /// without the op log lock; see `ChangeStore::creator_resolver`.
    creator_resolver: Option<Arc<CreatorResolver>>,
}

#[derive(Default)]
struct InnerSharedArena {
    // Container metadata is a single consistency domain. Keep it under one
    // mutex so container id/index/parent/depth updates cannot acquire locks in
    // inconsistent orders.
    containers: RwLock<ArenaContainers>,
    values: Mutex<Vec<LoroValue>>,
    str: Arc<Mutex<StrArena>>,
}

impl fmt::Debug for InnerSharedArena {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("InnerSharedArena")
            .field("containers", &"<Mutex<_>>")
            .field("values", &"<Mutex<_>>")
            .field("str", &"<Arc<Mutex<_>>>")
            .finish()
    }
}

/// This is shared between [OpLog] and [AppState].
///
#[derive(Debug, Clone)]
pub struct SharedArena {
    inner: Arc<InnerSharedArena>,
}

pub(crate) struct SharedArenaRollback {
    container_len: usize,
    root_len: usize,
    top_level_root_len: usize,
    values_len: usize,
    str: StrArenaCheckpoint,
}

/// How far the arena reached when something that refers into it was built, e.g. a parsed
/// change block: it can only refer to containers, values, and text allocated before.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct ArenaExtent {
    containers: usize,
    values: usize,
    str_bytes: usize,
}

impl SharedArenaRollback {
    /// Whether rolling back to this checkpoint keeps everything within `extent`.
    pub(crate) fn keeps(&self, extent: ArenaExtent) -> bool {
        extent.containers <= self.container_len
            && extent.values <= self.values_len
            && extent.str_bytes <= self.str.bytes_len()
    }
}

#[derive(Debug)]
pub struct StrAllocResult {
    /// unicode start
    pub start: usize,
    /// unicode end
    pub end: usize,
}

impl ArenaContainers {
    /// Add a freshly-registered cid to the retention-root tracking vectors.
    ///
    /// Centralizes the `root_c_idx ⊇ top_level_root_c_idx` invariant: `root_c_idx` is the set of
    /// retention roots (top-level user roots **and** mergeable cids) that seed the alive-walk for
    /// shallow snapshot; `top_level_root_c_idx` is the user-visible subset enumerated by
    /// `preferred_root_containers` etc. Every push to either vector should go through this method,
    /// so the invariant is maintained by construction rather than by remembering to push to two
    /// vectors at every call site.
    fn push_root(&mut self, idx: ContainerIdx, is_mergeable: bool) {
        self.root_c_idx.push(idx);
        if !is_mergeable {
            self.top_level_root_c_idx.push(idx);
        }
    }

    fn register_container(&mut self, id: &ContainerID) -> ContainerIdx {
        if let Some(&idx) = self.container_id_to_idx.get(id) {
            return idx;
        }

        let idx = self.container_idx_to_id.len();
        self.container_idx_to_id.push(id.clone());
        let idx = ContainerIdx::from_index_and_type(idx as u32, id.container_type());
        self.container_id_to_idx.insert(id.clone(), idx);
        // Resolve the cid's kind once. `is_mergeable` is non-trivial for mergeable names,
        // so we avoid calling it twice on the same id.
        let mergeable_parts = if id.is_root() {
            id.parse_mergeable()
        } else {
            None
        };
        match (id.is_root(), mergeable_parts) {
            (true, None) => {
                self.push_root(idx, false);
                self.parents.insert(idx, None);
                self.depth.push(NonZeroU16::new(1));
            }
            (true, Some((parent_id, _key, _kind))) => {
                // Mergeable Roots are retention roots AND logical children. Push to `root_c_idx`
                // so shallow snapshot's `retain_keys` does not GC the loser of a concurrent-kind
                // conflict (whose state is reachable only by its deterministic cid, not through
                // the parent marker); then `set_parent` for path/event resolution. They are
                // deliberately absent from `top_level_root_c_idx` so user-facing root
                // enumeration does not have to filter them out.
                self.push_root(idx, true);
                self.depth.push(None);
                let parent_idx = self.register_container(&parent_id);
                self.set_parent(idx, Some(parent_idx));
            }
            (false, _) => {
                self.depth.push(None);
            }
        }
        idx
    }

    fn set_parent(&mut self, child: ContainerIdx, parent: Option<ContainerIdx>) {
        self.parents.insert(child, parent);

        match parent {
            Some(p) => {
                // Keep parent linking side-effect free. Calling `get_depth()` here may invoke the
                // lazy parent resolver, which can acquire the state KV lock. Import/load paths may
                // be assembling parent edges from KV snapshots, so resolving depth here would make
                // arena -> KV and KV -> arena lock orders coexist.
                if let Some(d) = self.depth[p.to_index() as usize] {
                    self.depth[child.to_index() as usize] = NonZeroU16::new(d.get() + 1);
                } else {
                    self.depth[child.to_index() as usize] = None;
                }
            }
            None => {
                self.depth[child.to_index() as usize] = NonZeroU16::new(1);
            }
        }
    }

    fn get_depth(&mut self, target: ContainerIdx) -> Option<NonZeroU16> {
        if let Some(d) = self.depth[target.to_index() as usize] {
            return Some(d);
        }

        let parent: Option<ContainerIdx> = if let Some(p) = self.parents.get(&target) {
            *p
        } else {
            let id = self
                .container_idx_to_id
                .get(target.to_index() as usize)
                .unwrap()
                .clone();
            if id.is_root() && !id.is_mergeable() {
                None
            } else {
                // Mergeable Roots get registered via the main `register_container` path, which
                // already wires their parent edge — they should never reach here with a missing
                // `parents` entry. But for ordinary children whose parent isn't in the arena
                // yet, fall back to the resolver.
                let resolver = self.parent_resolver.clone()?;
                let parent_id = resolver(id)?;
                // Route through `register_container` so a freshly-discovered parent (especially
                // a mergeable cid) lands in `root_c_idx`, gets its own parent edge wired up,
                // and has `parents` initialized — instead of being half-registered with just
                // `idx_to_id` / `id_to_idx` / `depth` and silently missing from retention walks.
                let parent_idx =
                    if let Some(idx) = self.container_id_to_idx.get(&parent_id).copied() {
                        // Already in the arena. For a top-level root that was hand-registered
                        // somewhere else without a `parents` entry, ensure it has one.
                        if parent_id.is_root() && !parent_id.is_mergeable() {
                            self.parents.entry(idx).or_insert(None);
                            if self.depth[idx.to_index() as usize].is_none() {
                                self.depth[idx.to_index() as usize] = NonZeroU16::new(1);
                            }
                        }
                        idx
                    } else {
                        self.register_container(&parent_id)
                    };

                Some(parent_idx)
            }
        };

        let d = match parent {
            Some(p) => NonZeroU16::new(self.get_depth(p)?.get() + 1),
            None => NonZeroU16::new(1),
        };
        self.depth[target.to_index() as usize] = d;
        d
    }

    fn container_id(&self, idx: ContainerIdx) -> Option<ContainerID> {
        self.container_idx_to_id
            .get(idx.to_index() as usize)
            .cloned()
    }
}

pub(crate) struct ArenaGuards<'a> {
    containers: RwLockWriteGuard<'a, ArenaContainers>,
}

impl ArenaGuards<'_> {
    pub fn register_container(&mut self, id: &ContainerID) -> ContainerIdx {
        self.containers.register_container(id)
    }

    pub fn set_parent(&mut self, child: ContainerIdx, parent: Option<ContainerIdx>) {
        self.containers.set_parent(child, parent);
    }
}

impl SharedArena {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self {
            inner: Arc::new(InnerSharedArena::default()),
        }
    }

    pub fn fork(&self) -> Self {
        Self {
            inner: Arc::new(InnerSharedArena {
                containers: RwLock::new({
                    let containers = self.inner.containers.read();
                    ArenaContainers {
                        container_idx_to_id: containers.container_idx_to_id.clone(),
                        depth: containers.depth.clone(),
                        container_id_to_idx: containers.container_id_to_idx.clone(),
                        parents: containers.parents.clone(),
                        root_c_idx: containers.root_c_idx.clone(),
                        top_level_root_c_idx: containers.top_level_root_c_idx.clone(),
                        parent_resolver: containers.parent_resolver.clone(),
                        // It reads the source document's op log.
                        creator_resolver: None,
                    }
                }),
                values: Mutex::new(self.inner.values.lock().clone()),
                str: self.inner.str.clone(),
            }),
        }
    }

    /// See [`ArenaExtent`].
    pub(crate) fn extent(&self) -> ArenaExtent {
        ArenaExtent {
            containers: self.inner.containers.read().container_idx_to_id.len(),
            values: self.inner.values.lock().len(),
            str_bytes: self.inner.str.lock().bytes_len(),
        }
    }

    pub(crate) fn checkpoint_for_rollback(&self) -> SharedArenaRollback {
        let containers = self.inner.containers.read();
        let container_len = containers.container_idx_to_id.len();
        let root_len = containers.root_c_idx.len();
        let top_level_root_len = containers.top_level_root_c_idx.len();
        drop(containers);
        let values_len = self.inner.values.lock().len();
        let str = self.inner.str.lock().checkpoint();
        SharedArenaRollback {
            container_len,
            root_len,
            top_level_root_len,
            values_len,
            str,
        }
    }

    /// Call it through the op log's change store (`ChangeStore::rollback_arena` and friends),
    /// which does it under the lock the creator resolver parses under and drops the parsed
    /// changes that may refer to what this removes. See `context/arena-parent-links.md`.
    pub(crate) fn rollback(&self, checkpoint: SharedArenaRollback) {
        let mut containers = self.inner.containers.write();
        let removed_ids = containers
            .container_idx_to_id
            .split_off(checkpoint.container_len);
        for id in removed_ids {
            containers.container_id_to_idx.remove(&id);
        }
        containers.depth.truncate(checkpoint.container_len);
        containers.root_c_idx.truncate(checkpoint.root_len);
        containers
            .top_level_root_c_idx
            .truncate(checkpoint.top_level_root_len);
        containers.parents.retain(|child, parent| {
            let child_is_kept = (child.to_index() as usize) < checkpoint.container_len;
            let parent_is_kept = parent
                .map(|p| (p.to_index() as usize) < checkpoint.container_len)
                .unwrap_or(true);
            child_is_kept && parent_is_kept
        });
        drop(containers);

        self.inner.values.lock().truncate(checkpoint.values_len);
        self.inner.str.lock().rollback(checkpoint.str);
    }

    pub(crate) fn with_guards(&self, f: impl FnOnce(&mut ArenaGuards)) {
        let mut guards = self.get_arena_guards();
        f(&mut guards);
    }

    fn get_arena_guards(&self) -> ArenaGuards<'_> {
        ArenaGuards {
            containers: self.inner.containers.write(),
        }
    }

    pub fn register_container(&self, id: &ContainerID) -> ContainerIdx {
        self.inner.containers.write().register_container(id)
    }

    pub fn get_container_id(&self, idx: ContainerIdx) -> Option<ContainerID> {
        self.inner.containers.read().container_id(idx)
    }

    /// Fast map from `ContainerID` to `ContainerIdx` for containers already registered
    /// in the arena.
    ///
    /// Important: This is not an existence check. Absence here does not imply that a
    /// container does not exist, since registration can be lazy and containers may
    /// be persisted only in the state KV store until first use.
    ///
    /// For existence-aware lookup that consults persisted state and performs lazy
    /// registration, prefer `DocState::resolve_idx`.
    pub fn id_to_idx(&self, id: &ContainerID) -> Option<ContainerIdx> {
        self.inner
            .containers
            .read()
            .container_id_to_idx
            .get(id)
            .copied()
    }

    #[inline]
    pub fn idx_to_id(&self, id: ContainerIdx) -> Option<ContainerID> {
        self.inner.containers.read().container_id(id)
    }

    #[inline]
    pub fn with_idx_to_id<R>(&self, f: impl FnOnce(&Vec<ContainerID>) -> R) -> R {
        let containers = self.inner.containers.read();
        f(&containers.container_idx_to_id)
    }

    pub fn alloc_str(&self, str: &str) -> StrAllocResult {
        let mut text_lock = self.inner.str.lock();
        _alloc_str(&mut text_lock, str)
    }

    /// return slice and unicode index
    pub fn alloc_str_with_slice(&self, str: &str) -> (BytesSlice, StrAllocResult) {
        let mut text_lock = self.inner.str.lock();
        _alloc_str_with_slice(&mut text_lock, str)
    }

    /// alloc str without extra info
    pub fn alloc_str_fast(&self, bytes: &[u8]) {
        let mut text_lock = self.inner.str.lock();
        text_lock.alloc(std::str::from_utf8(bytes).unwrap());
    }

    #[inline]
    pub fn utf16_len(&self) -> usize {
        self.inner.str.lock().len_utf16()
    }

    #[inline]
    pub fn alloc_value(&self, value: LoroValue) -> usize {
        let mut values_lock = self.inner.values.lock();
        _alloc_value(&mut values_lock, value)
    }

    #[inline]
    pub fn alloc_values(&self, values: impl Iterator<Item = LoroValue>) -> std::ops::Range<usize> {
        let mut values_lock = self.inner.values.lock();
        _alloc_values(&mut values_lock, values)
    }

    #[inline]
    pub fn set_parent(&self, child: ContainerIdx, parent: Option<ContainerIdx>) {
        self.inner.containers.write().set_parent(child, parent);
    }

    pub fn log_hierarchy(&self) {
        if cfg!(debug_assertions) {
            let containers = self.inner.containers.read();
            for (c, p) in containers.parents.iter() {
                tracing::info!(
                    "container {:?} {:?} {:?}",
                    c,
                    containers.container_id(*c),
                    p.and_then(|x| containers.container_id(x))
                );
            }
        }
    }

    pub fn log_all_containers(&self) {
        let containers = self.inner.containers.read();
        containers.container_id_to_idx.iter().for_each(|(id, idx)| {
            tracing::info!("container {:?} {:?}", id, idx);
        });
        containers
            .container_idx_to_id
            .iter()
            .enumerate()
            .for_each(|(i, id)| {
                tracing::info!("container {} {:?}", i, id);
            });
    }

    /// The parent of `child`, or `None` for a top-level root.
    ///
    /// Also `None` for a normal container that no op in the history creates (an ID from the
    /// user that is not a container, an op not received yet, or a container that is not in
    /// the state of a shallow document and whose creating op was trimmed): no path leads to
    /// it, so it is treated like a container whose parent no longer holds it. A history that
    /// has the op but cannot parse it panics instead (see [`CreatorResolver`]).
    pub fn get_parent(&self, child: ContainerIdx) -> Option<ContainerIdx> {
        match self.resolve_parent(child) {
            Some(parent) => parent,
            None => {
                assert!(
                    self.inner.containers.read().creator_resolver.is_some(),
                    "InternalError: Parent is not registered"
                );
                None
            }
        }
    }

    /// Finds and registers `child`'s parent edge: the registered edge, else the state KV
    /// (`parent_resolver`), else the change that created it (`creator_resolver`).
    ///
    /// Returns `Some(parent)` when the edge is known (`parent` is `None` for a top-level root)
    /// and `None` when no source knows the container.
    fn resolve_parent(&self, child: ContainerIdx) -> Option<Option<ContainerIdx>> {
        let (child_id, resolver, creator_resolver) = {
            let containers = self.inner.containers.read();
            let child_id = containers.container_id(child).unwrap();
            if child_id.is_root() && !child_id.is_mergeable() {
                // TODO: PERF: we can speed this up by use a special bit in ContainerIdx to indicate
                // whether the target is a root container
                return Some(None);
            }

            // Try fast path first
            if let Some(p) = containers.parents.get(&child).copied() {
                return Some(p);
            }

            // Fallback: try to resolve parent lazily via the resolvers if provided.
            (
                child_id,
                containers.parent_resolver.clone(),
                containers.creator_resolver.clone(),
            )
        };
        if let Some(resolver) = resolver {
            if let Some(parent_id) = resolver(child_id.clone()) {
                let parent_idx = self.register_container(&parent_id);
                self.set_parent(child, Some(parent_idx));
                return Some(Some(parent_idx));
            }
        }

        if let (Some(resolver), ContainerID::Normal { peer, counter, .. }) =
            (creator_resolver, &child_id)
        {
            return match resolver(self, ID::new(*peer, *counter)) {
                // Registered unless the op does not create this container.
                CreatorOp::Loaded => self.get_registered_parent(child),
                CreatorOp::Absent => None,
            };
        }

        None
    }

    /// The index of a normal container that is not registered yet but that an op in the
    /// history creates. Loading that op's change registers the container and its parent.
    pub(crate) fn find_created_container(&self, id: &ContainerID) -> Option<ContainerIdx> {
        if let Some(idx) = self.id_to_idx(id) {
            return Some(idx);
        }
        let ContainerID::Normal { peer, counter, .. } = id else {
            return None;
        };
        let resolver = self.inner.containers.read().creator_resolver.clone()?;
        match resolver(self, ID::new(*peer, *counter)) {
            CreatorOp::Loaded => self.id_to_idx(id),
            CreatorOp::Absent => None,
        }
    }

    /// Return the parent edge already stored in the arena without invoking the lazy resolver.
    ///
    /// The outer `Option` distinguishes an unregistered edge from a registered root edge.
    pub(crate) fn get_registered_parent(
        &self,
        child: ContainerIdx,
    ) -> Option<Option<ContainerIdx>> {
        self.inner.containers.read().parents.get(&child).copied()
    }

    /// Call `f` on each ancestor of `container`, including `container` itself.
    ///
    /// f(ContainerIdx, is_first)
    pub fn with_ancestors(&self, container: ContainerIdx, mut f: impl FnMut(ContainerIdx, bool)) {
        let mut container = Some(container);
        let mut is_first = true;
        while let Some(c) = container {
            f(c, is_first);
            is_first = false;
            container = self.get_parent(c)
        }
    }

    #[inline]
    pub fn slice_by_unicode(&self, range: impl RangeBounds<usize>) -> BytesSlice {
        self.inner.str.lock().slice_by_unicode(range)
    }

    #[inline]
    pub fn slice_by_utf8(&self, range: impl RangeBounds<usize>) -> BytesSlice {
        self.inner.str.lock().slice_bytes(range)
    }

    #[inline]
    pub fn slice_str_by_unicode_range(&self, range: Range<usize>) -> String {
        let mut s = self.inner.str.lock();
        let s: &mut StrArena = &mut s;
        let mut ans = String::with_capacity(range.len());
        ans.push_str(s.slice_str_by_unicode(range));
        ans
    }

    #[inline]
    pub fn with_text_slice(&self, range: Range<usize>, mut f: impl FnMut(&str)) {
        f(self.inner.str.lock().slice_str_by_unicode(range))
    }

    #[inline]
    pub fn get_value(&self, idx: usize) -> Option<LoroValue> {
        self.inner.values.lock().get(idx).cloned()
    }

    #[inline]
    pub fn get_values(&self, range: Range<usize>) -> Vec<LoroValue> {
        (self.inner.values.lock()[range]).to_vec()
    }

    /// Borrow the values in `range` without cloning them (unlike
    /// [`Self::get_values`], which clones into a fresh `Vec`).
    #[inline]
    pub fn with_values<R>(&self, range: Range<usize>, f: impl FnOnce(&[LoroValue]) -> R) -> R {
        f(&self.inner.values.lock()[range])
    }

    pub fn convert_single_op(
        &self,
        container: &ContainerID,
        peer: PeerID,
        counter: Counter,
        lamport: Lamport,
        content: RawOpContent,
    ) -> Op {
        let container = self.register_container(container);
        self.inner_convert_op(content, peer, counter, lamport, container)
    }

    pub fn can_import_snapshot(&self) -> bool {
        let str_empty = self.inner.str.lock().is_empty();
        let values_empty = self.inner.values.lock().is_empty();
        str_empty && values_empty
    }

    fn inner_convert_op(
        &self,
        content: RawOpContent<'_>,
        _peer: PeerID,
        counter: i32,
        _lamport: Lamport,
        container: ContainerIdx,
    ) -> Op {
        match content {
            crate::op::RawOpContent::Map(MapSet { key, value }) => Op {
                counter,
                container,
                content: crate::op::InnerContent::Map(MapSet { key, value }),
            },
            crate::op::RawOpContent::List(list) => match list {
                ListOp::Insert { slice, pos } => match slice {
                    ListSlice::RawData(values) => {
                        let range = self.alloc_values(values.iter().cloned());
                        Op {
                            counter,
                            container,
                            content: crate::op::InnerContent::List(InnerListOp::Insert {
                                slice: SliceRange::from(range.start as u32..range.end as u32),
                                pos,
                            }),
                        }
                    }
                    ListSlice::RawStr { str, unicode_len } => {
                        let (slice, info) = self.alloc_str_with_slice(&str);
                        Op {
                            counter,
                            container,
                            content: crate::op::InnerContent::List(InnerListOp::InsertText {
                                slice,
                                unicode_start: info.start as u32,
                                unicode_len: unicode_len as u32,
                                pos: pos as u32,
                            }),
                        }
                    }
                },
                ListOp::Delete(span) => Op {
                    counter,
                    container,
                    content: crate::op::InnerContent::List(InnerListOp::Delete(span)),
                },
                ListOp::StyleStart {
                    start,
                    end,
                    info,
                    key,
                    value,
                } => Op {
                    counter,
                    container,
                    content: InnerContent::List(InnerListOp::StyleStart {
                        start,
                        end,
                        key,
                        info,
                        value,
                    }),
                },
                ListOp::StyleEnd => Op {
                    counter,
                    container,
                    content: InnerContent::List(InnerListOp::StyleEnd),
                },
                ListOp::Move {
                    from,
                    to,
                    elem_id: from_id,
                } => Op {
                    counter,
                    container,
                    content: InnerContent::List(InnerListOp::Move {
                        from,
                        to,
                        elem_id: from_id,
                    }),
                },
                ListOp::Set { elem_id, value } => Op {
                    counter,
                    container,
                    content: InnerContent::List(InnerListOp::Set { elem_id, value }),
                },
            },
            crate::op::RawOpContent::Tree(tree) => Op {
                counter,
                container,
                content: crate::op::InnerContent::Tree(tree.clone()),
            },
            #[cfg(feature = "counter")]
            crate::op::RawOpContent::Counter(c) => Op {
                counter,
                container,
                content: crate::op::InnerContent::Future(crate::op::FutureInnerContent::Counter(c)),
            },
            crate::op::RawOpContent::Unknown { prop, value } => Op {
                counter,
                container,
                content: crate::op::InnerContent::Future(crate::op::FutureInnerContent::Unknown {
                    prop,
                    value: Box::new(value),
                }),
            },
        }
    }

    #[inline]
    pub fn convert_raw_op(&self, op: &RawOp) -> Op {
        self.inner_convert_op(
            op.content.clone(),
            op.id.peer,
            op.id.counter,
            op.lamport,
            op.container,
        )
    }

    #[inline]
    pub fn export_containers(&self) -> Vec<ContainerID> {
        self.inner.containers.read().container_idx_to_id.clone()
    }

    pub fn export_parents(&self) -> Vec<Option<ContainerIdx>> {
        let containers = self.inner.containers.read();
        containers
            .container_idx_to_id
            .iter()
            .enumerate()
            .map(|(x, id)| {
                let idx = ContainerIdx::from_index_and_type(x as u32, id.container_type());
                let parent_idx = containers.parents.get(&idx)?;
                *parent_idx
            })
            .collect()
    }

    /// Returns all the possible root containers of the docs
    ///
    /// We need to load all the cached kv in DocState before we can ensure all root contains are covered.
    /// So we need the flag type here.
    ///
    /// This includes mergeable cids (which are also retention roots). Callers that only want
    /// user-visible top-level roots should use [`Self::top_level_root_containers`].
    #[inline]
    pub(crate) fn root_containers(&self, _f: LoadAllFlag) -> Vec<ContainerIdx> {
        self.inner.containers.read().root_c_idx.clone()
    }

    /// Returns only the user-visible top-level root containers (excludes mergeable cids).
    ///
    /// Used by `preferred_root_containers`, `get_value` / `get_deep_value`, jsonpath, etc. —
    /// any path that enumerates the doc's top-level roots. Pre-filtering at registration time
    /// keeps these calls O(top_level_roots) instead of O(top_level_roots + mergeable_cids).
    #[inline]
    pub(crate) fn top_level_root_containers(&self, _f: LoadAllFlag) -> Vec<ContainerIdx> {
        self.inner.containers.read().top_level_root_c_idx.clone()
    }

    // TODO: this can return a u16 directly now, since the depths are always valid
    pub(crate) fn get_depth(&self, container: ContainerIdx) -> Option<NonZeroU16> {
        if let Some(d) = self.inner.containers.read().depth[container.to_index() as usize] {
            return Some(d);
        }

        // `ArenaContainers::get_depth` runs under the arena lock, where the creator resolver
        // cannot run, so resolve the missing parent links first.
        let mut c = container;
        // Every step goes one level up, and no valid chain is deeper than `u16::MAX`.
        let mut steps = 0usize;
        loop {
            steps += 1;
            assert!(
                steps <= u16::MAX as usize + 1,
                "InternalError: the parent links of {:?} form a cycle",
                self.idx_to_id(container)
            );
            {
                let containers = self.inner.containers.read();
                if containers.depth[c.to_index() as usize].is_some() {
                    break;
                }
                match containers.parents.get(&c).copied() {
                    Some(Some(p)) => {
                        c = p;
                        continue;
                    }
                    Some(None) => break,
                    None => {}
                }
            }
            match self.resolve_parent(c) {
                Some(Some(p)) => c = p,
                _ => break,
            }
        }
        self.inner.containers.write().get_depth(container)
    }

    pub(crate) fn iter_value_slice(
        &self,
        range: Range<usize>,
    ) -> impl Iterator<Item = LoroValue> + '_ {
        let values = self.inner.values.lock();
        range
            .into_iter()
            .map(move |i| values.get(i).unwrap().clone())
    }

    #[allow(unused)]
    pub(crate) fn log_all_values(&self) {
        let values = self.inner.values.lock();
        for (i, v) in values.iter().enumerate() {
            loro_common::debug!("value {} {:?}", i, v);
        }
    }
}

fn _alloc_str_with_slice(
    text_lock: &mut MutexGuard<'_, StrArena>,
    str: &str,
) -> (BytesSlice, StrAllocResult) {
    let start = text_lock.len_bytes();
    let ans = _alloc_str(text_lock, str);
    (text_lock.slice_bytes(start..), ans)
}

fn _alloc_values(
    values_lock: &mut MutexGuard<'_, Vec<LoroValue>>,
    values: impl Iterator<Item = LoroValue>,
) -> Range<usize> {
    values_lock.reserve(values.size_hint().0);
    let start = values_lock.len();
    for value in values {
        values_lock.push(value);
    }

    start..values_lock.len()
}

fn _alloc_value(values_lock: &mut MutexGuard<'_, Vec<LoroValue>>, value: LoroValue) -> usize {
    values_lock.push(value);
    values_lock.len() - 1
}

fn _alloc_str(text_lock: &mut MutexGuard<'_, StrArena>, str: &str) -> StrAllocResult {
    let start = text_lock.len_unicode();
    text_lock.alloc(str);
    StrAllocResult {
        start,
        end: text_lock.len_unicode(),
    }
}

fn _slice_str(range: Range<usize>, s: &mut StrArena) -> String {
    let mut ans = String::with_capacity(range.len());
    ans.push_str(s.slice_str_by_unicode(range));
    ans
}

impl SharedArena {
    /// Register or clear a resolver to lazily determine a container's parent when missing.
    ///
    /// - The resolver receives the child `ContainerIdx` and returns an optional `ContainerID` of its parent.
    /// - If the resolver returns `Some`, `SharedArena` will register the parent in the arena and link it.
    /// - If the resolver is `None` or returns `None`, `get_parent` will panic for non-root containers as before.
    pub fn set_parent_resolver<F>(&self, resolver: Option<F>)
    where
        F: Fn(ContainerID) -> Option<ContainerID> + Send + Sync + 'static,
    {
        self.inner.containers.write().parent_resolver =
            resolver.map(|f| Arc::new(f) as Arc<ParentResolver>);
    }

    /// Register the op log's [`CreatorResolver`]. See `creator_resolver` in `ArenaContainers`.
    pub(crate) fn set_creator_resolver<F>(&self, resolver: F)
    where
        F: Fn(&SharedArena, ID) -> CreatorOp + Send + Sync + 'static,
    {
        self.inner.containers.write().creator_resolver = Some(Arc::new(resolver));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use loro_common::ContainerType;
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };

    /// When a non-mergeable child is registered, then later asks for its depth and the parent
    /// resolver yields a mergeable cid, the mergeable parent must be registered through the
    /// retention-root path (push to `root_c_idx`, recursively link its own parent) — exactly the
    /// same way the main `register_container` path would handle a freshly-encountered mergeable
    /// cid. Without this, shallow snapshot's alive-walk would silently GC the mergeable parent's
    /// state.
    #[test]
    fn get_depth_resolver_registers_mergeable_parent_as_retention_root() {
        let arena = SharedArena::new();

        let top_root = ContainerID::new_root("state", ContainerType::Map);
        let mergeable_parent = ContainerID::new_mergeable(&top_root, "profile", ContainerType::Map);
        let child = ContainerID::new_normal(loro_common::ID::new(1, 0), ContainerType::List);

        let mergeable_parent_for_resolver = mergeable_parent.clone();
        let child_for_resolver = child.clone();
        arena.set_parent_resolver(Some(move |q: ContainerID| {
            if q == child_for_resolver {
                Some(mergeable_parent_for_resolver.clone())
            } else {
                None
            }
        }));

        let child_idx = arena.register_container(&child);
        let _ = arena.get_depth(child_idx);

        let flag = LoadAllFlag;
        let mergeable_idx = arena
            .id_to_idx(&mergeable_parent)
            .expect("resolver should have registered the mergeable parent");
        assert!(
            arena.root_containers(flag).contains(&mergeable_idx),
            "mergeable parent discovered via resolver must be a retention root"
        );

        let flag = LoadAllFlag;
        assert!(
            !arena
                .top_level_root_containers(flag)
                .contains(&mergeable_idx),
            "mergeable parent must not appear in user-facing top-level enumeration"
        );

        let flag = LoadAllFlag;
        let top_root_idx = arena
            .id_to_idx(&top_root)
            .expect("mergeable parent's own grandparent must also be registered");
        assert!(
            arena
                .top_level_root_containers(flag)
                .contains(&top_root_idx),
            "the mergeable parent's grandparent (a top-level root) must be in the top-level list"
        );
    }

    /// The creator resolver parses changes, which registers containers in the arena, so
    /// `get_parent` and `get_depth` must call it without holding the arena lock.
    #[test]
    fn creator_resolver_runs_outside_the_arena_lock() {
        let arena = SharedArena::new();
        let root = ContainerID::new_root("tree", ContainerType::Tree);
        let meta = ContainerID::new_normal(ID::new(1, 3), ContainerType::Map);
        let meta_for_resolver = meta.clone();
        arena.set_creator_resolver(move |arena: &SharedArena, id: ID| {
            assert_eq!(id, ID::new(1, 3));
            // What parsing the change does: register the container and its parent.
            let parent = arena.register_container(&root);
            let child = arena.register_container(&meta_for_resolver);
            arena.set_parent(child, Some(parent));
            CreatorOp::Loaded
        });

        let meta_idx = arena.register_container(&meta);
        assert_eq!(arena.get_depth(meta_idx).map(|d| d.get()), Some(2));
        let root_idx = arena.id_to_idx(&ContainerID::new_root("tree", ContainerType::Tree));
        assert_eq!(arena.get_parent(meta_idx), root_idx);
    }

    #[test]
    fn a_container_that_no_op_creates_has_no_parent() {
        for answer in [CreatorOp::Loaded, CreatorOp::Absent] {
            // `Loaded`: the op exists but creates something else.
            let arena = SharedArena::new();
            arena.set_creator_resolver(move |_: &SharedArena, _: ID| answer);
            let id = ContainerID::new_normal(ID::new(1, 3), ContainerType::Map);
            let idx = arena.register_container(&id);
            assert_eq!(arena.get_parent(idx), None);
            assert_eq!(arena.get_depth(idx), None);
            let missing = ContainerID::new_normal(ID::new(1, 4), ContainerType::Text);
            assert_eq!(arena.find_created_container(&missing), None);
        }
    }

    #[test]
    #[should_panic(expected = "form a cycle")]
    fn a_parent_cycle_fails_fast() {
        let arena = SharedArena::new();
        let a =
            arena.register_container(&ContainerID::new_normal(ID::new(1, 0), ContainerType::Map));
        let b =
            arena.register_container(&ContainerID::new_normal(ID::new(1, 1), ContainerType::Map));
        arena.set_parent(a, Some(b));
        arena.set_parent(b, Some(a));
        arena.get_depth(a);
    }

    /// Without an op log to ask, an unknown parent is still an internal error.
    #[test]
    #[should_panic(expected = "Parent is not registered")]
    fn unknown_parent_without_a_creator_resolver_panics() {
        let arena = SharedArena::new();
        let idx =
            arena.register_container(&ContainerID::new_normal(ID::new(1, 3), ContainerType::Map));
        arena.get_parent(idx);
    }

    #[test]
    fn set_parent_does_not_resolve_missing_parent_depth() {
        let arena = SharedArena::new();
        let parent = ContainerID::new_normal(loro_common::ID::new(1, 0), ContainerType::Map);
        let child = ContainerID::new_mergeable(&parent, "field", ContainerType::Text);
        let resolver_called = Arc::new(AtomicBool::new(false));
        let called = resolver_called.clone();
        arena.set_parent_resolver(Some(move |_| {
            called.store(true, Ordering::SeqCst);
            None
        }));

        arena.register_container(&child);

        assert!(
            !resolver_called.load(Ordering::SeqCst),
            "registering a mergeable child must not invoke the lazy parent resolver"
        );
    }
}

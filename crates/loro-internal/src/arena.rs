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
use loro_common::PeerID;
use rustc_hash::FxHashMap;
use std::fmt;
use std::{
    num::NonZeroU16,
    ops::{Range, RangeBounds},
    sync::Arc,
};

pub(crate) struct LoadAllFlag;
type ParentResolver = dyn Fn(ContainerID) -> Option<ContainerID> + Send + Sync + 'static;

#[derive(Default)]
struct ArenaContainers {
    container_idx_to_id: Vec<ContainerID>,
    // 0 stands for unknown, 1 stands for root containers
    depth: Vec<Option<NonZeroU16>>,
    container_id_to_idx: FxHashMap<ContainerID, ContainerIdx>,
    /// The parent of each container.
    parents: FxHashMap<ContainerIdx, Option<ContainerIdx>>,
    root_c_idx: Vec<ContainerIdx>,
    /// Optional resolver used when querying parent for a container that has not been registered yet.
    /// If set, `get_parent` will try this resolver to lazily fetch and register the parent.
    parent_resolver: Option<Arc<ParentResolver>>,
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
    values_len: usize,
    str: StrArenaCheckpoint,
}

#[derive(Debug)]
pub struct StrAllocResult {
    /// unicode start
    pub start: usize,
    /// unicode end
    pub end: usize,
}

impl ArenaContainers {
    fn register_container(&mut self, id: &ContainerID) -> ContainerIdx {
        if let Some(&idx) = self.container_id_to_idx.get(id) {
            return idx;
        }

        let idx = self.container_idx_to_id.len();
        self.container_idx_to_id.push(id.clone());
        let idx = ContainerIdx::from_index_and_type(idx as u32, id.container_type());
        self.container_id_to_idx.insert(id.clone(), idx);
        if id.is_root() {
            self.root_c_idx.push(idx);
            self.parents.insert(idx, None);
            self.depth.push(NonZeroU16::new(1));
        } else {
            self.depth.push(None);
        }
        idx
    }

    fn set_parent(&mut self, child: ContainerIdx, parent: Option<ContainerIdx>) {
        self.parents.insert(child, parent);

        match parent {
            Some(p) => {
                if let Some(d) = self.get_depth(p) {
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

    fn get_depth(&mut self, container: ContainerIdx) -> Option<NonZeroU16> {
        get_depth(
            container,
            &mut self.depth,
            &mut self.parents,
            &self.parent_resolver,
            &mut self.container_idx_to_id,
            &mut self.container_id_to_idx,
            &mut self.root_c_idx,
        )
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
                        parent_resolver: containers.parent_resolver.clone(),
                    }
                }),
                values: Mutex::new(self.inner.values.lock().clone()),
                str: self.inner.str.clone(),
            }),
        }
    }

    pub(crate) fn checkpoint_for_rollback(&self) -> SharedArenaRollback {
        let containers = self.inner.containers.read();
        let container_len = containers.container_idx_to_id.len();
        let root_len = containers.root_c_idx.len();
        drop(containers);
        let values_len = self.inner.values.lock().len();
        let str = self.inner.str.lock().checkpoint();
        SharedArenaRollback {
            container_len,
            root_len,
            values_len,
            str,
        }
    }

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

    pub fn get_parent(&self, child: ContainerIdx) -> Option<ContainerIdx> {
        let (child_id, resolver) = {
            let containers = self.inner.containers.read();
            let child_id = containers.container_id(child).unwrap();
            if child_id.is_root() {
                // TODO: PERF: we can speed this up by use a special bit in ContainerIdx to indicate
                // whether the target is a root container
                return None;
            }

            // Try fast path first
            if let Some(p) = containers.parents.get(&child).copied() {
                return p;
            }

            // Fallback: try to resolve parent lazily via the resolver if provided.
            (child_id, containers.parent_resolver.clone())
        };
        if let Some(resolver) = resolver {
            if let Some(parent_id) = resolver(child_id.clone()) {
                let parent_idx = self.register_container(&parent_id);
                self.set_parent(child, Some(parent_idx));
                return Some(parent_idx);
            }
        }

        panic!("InternalError: Parent is not registered")
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
    #[inline]
    pub(crate) fn root_containers(&self, _f: LoadAllFlag) -> Vec<ContainerIdx> {
        self.inner.containers.read().root_c_idx.clone()
    }

    // TODO: this can return a u16 directly now, since the depths are always valid
    pub(crate) fn get_depth(&self, container: ContainerIdx) -> Option<NonZeroU16> {
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

    pub(crate) fn get_root_container_idx_by_key(
        &self,
        root_index: &loro_common::InternalString,
    ) -> Option<ContainerIdx> {
        let containers = self.inner.containers.read();
        for t in loro_common::ContainerType::ALL_TYPES.iter() {
            let cid = ContainerID::Root {
                name: root_index.clone(),
                container_type: *t,
            };
            if let Some(idx) = containers.container_id_to_idx.get(&cid) {
                return Some(*idx);
            }
        }
        None
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

fn get_depth(
    target: ContainerIdx,
    depth: &mut Vec<Option<NonZeroU16>>,
    parents: &mut FxHashMap<ContainerIdx, Option<ContainerIdx>>,
    parent_resolver: &Option<Arc<ParentResolver>>,
    idx_to_id: &mut Vec<ContainerID>,
    id_to_idx: &mut FxHashMap<ContainerID, ContainerIdx>,
    root_c_idx: &mut Vec<ContainerIdx>,
) -> Option<NonZeroU16> {
    let mut d = depth[target.to_index() as usize];
    if d.is_some() {
        return d;
    }

    let parent: Option<ContainerIdx> = if let Some(p) = parents.get(&target) {
        *p
    } else {
        let id = idx_to_id.get(target.to_index() as usize).unwrap();
        if id.is_root() {
            None
        } else if let Some(parent_resolver) = parent_resolver.as_ref() {
            let parent_id = parent_resolver(id.clone())?;
            let parent_is_root = parent_id.is_root();
            let mut parent_was_new = false;
            // If the parent is not registered yet, register it lazily instead of unwrapping.
            let parent_idx = if let Some(idx) = id_to_idx.get(&parent_id).copied() {
                idx
            } else {
                let new_index = idx_to_id.len();
                idx_to_id.push(parent_id.clone());
                let new_idx =
                    ContainerIdx::from_index_and_type(new_index as u32, parent_id.container_type());
                id_to_idx.insert(parent_id.clone(), new_idx);
                // Keep depth vector in sync with containers list.
                if parent_is_root {
                    depth.push(NonZeroU16::new(1));
                } else {
                    depth.push(None);
                }
                parent_was_new = true;
                new_idx
            };

            if parent_is_root {
                if parent_was_new {
                    parents.insert(parent_idx, None);
                    root_c_idx.push(parent_idx);
                } else {
                    parents.entry(parent_idx).or_insert(None);
                }
                if depth[parent_idx.to_index() as usize].is_none() {
                    depth[parent_idx.to_index() as usize] = NonZeroU16::new(1);
                }
            }

            Some(parent_idx)
        } else {
            return None;
        }
    };

    match parent {
        Some(p) => {
            d = NonZeroU16::new(
                get_depth(
                    p,
                    depth,
                    parents,
                    parent_resolver,
                    idx_to_id,
                    id_to_idx,
                    root_c_idx,
                )?
                .get()
                    + 1,
            );
            depth[target.to_index() as usize] = d;
        }
        None => {
            depth[target.to_index() as usize] = NonZeroU16::new(1);
            d = NonZeroU16::new(1);
        }
    }

    d
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
}

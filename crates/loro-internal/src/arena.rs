mod str_arena;
use self::str_arena::StrArena;
use crate::sync::{Mutex, MutexGuard};
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
use rustc_hash::FxHashMap;
use loro_common::PeerID;
use std::fmt;
use std::{
    num::NonZeroU16,
    ops::{Range, RangeBounds},
    sync::Arc,
};

pub(crate) struct LoadAllFlag;
type ParentResolver = dyn Fn(ContainerID) -> Option<ContainerID> + Send + Sync + 'static;

#[derive(Default)]
struct InnerSharedArena {
    // The locks should not be exposed outside this file.
    // It might be better to use RwLock in the future
    container_idx_to_id: Mutex<Vec<ContainerID>>,
    // 0 stands for unknown, 1 stands for root containers
    depth: Mutex<Vec<Option<NonZeroU16>>>,
    container_id_to_idx: Mutex<FxHashMap<ContainerID, ContainerIdx>>,
    /// The parent of each container.
    parents: Mutex<FxHashMap<ContainerIdx, Option<ContainerIdx>>>,
    values: Mutex<Vec<LoroValue>>,
    root_c_idx: Mutex<Vec<ContainerIdx>>,
    str: Arc<Mutex<StrArena>>,
    /// Optional resolver used when querying parent for a container that has not been registered yet.
    /// If set, `get_parent` will try this resolver to lazily fetch and register the parent.
    parent_resolver: Mutex<Option<Arc<ParentResolver>>>,
}

impl fmt::Debug for InnerSharedArena {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("InnerSharedArena")
            .field("container_idx_to_id", &"<Mutex<_>>")
            .field("depth", &"<Mutex<_>>")
            .field("container_id_to_idx", &"<Mutex<_>>")
            .field("parents", &"<Mutex<_>>")
            .field("values", &"<Mutex<_>>")
            .field("root_c_idx", &"<Mutex<_>>")
            .field("str", &"<Arc<Mutex<_>>>")
            .field("parent_resolver", &"<Mutex<Option<...>>>")
            .finish()
    }
}

/// This is shared between [OpLog] and [AppState].
///
#[derive(Debug, Clone)]
pub struct SharedArena {
    inner: Arc<InnerSharedArena>,
}

#[derive(Debug)]
pub struct StrAllocResult {
    /// unicode start
    pub start: usize,
    /// unicode end
    pub end: usize,
}

pub(crate) struct ArenaGuards<'a> {
    container_id_to_idx: MutexGuard<'a, FxHashMap<ContainerID, ContainerIdx>>,
    container_idx_to_id: MutexGuard<'a, Vec<ContainerID>>,
    depth: MutexGuard<'a, Vec<Option<NonZeroU16>>>,
    parents: MutexGuard<'a, FxHashMap<ContainerIdx, Option<ContainerIdx>>>,
    root_c_idx: MutexGuard<'a, Vec<ContainerIdx>>,
    parent_resolver: MutexGuard<'a, Option<Arc<ParentResolver>>>,
}

impl ArenaGuards<'_> {
    pub fn register_container(&mut self, id: &ContainerID) -> ContainerIdx {
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

    pub fn set_parent(&mut self, child: ContainerIdx, parent: Option<ContainerIdx>) {
        self.parents.insert(child, parent);

        match parent {
            Some(p) => {
                if let Some(d) = get_depth(
                    p,
                    &mut self.depth,
                    &self.parents,
                    &self.parent_resolver,
                    &mut self.container_idx_to_id,
                    &mut self.container_id_to_idx,
                ) {
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
                container_idx_to_id: Mutex::new(
                    self.inner.container_idx_to_id.lock().unwrap().clone(),
                ),
                depth: Mutex::new(self.inner.depth.lock().unwrap().clone()),
                container_id_to_idx: Mutex::new(
                    self.inner.container_id_to_idx.lock().unwrap().clone(),
                ),
                parents: Mutex::new(self.inner.parents.lock().unwrap().clone()),
                values: Mutex::new(self.inner.values.lock().unwrap().clone()),
                root_c_idx: Mutex::new(self.inner.root_c_idx.lock().unwrap().clone()),
                str: self.inner.str.clone(),
                parent_resolver: Mutex::new(self.inner.parent_resolver.lock().unwrap().clone()),
            }),
        }
    }

    pub(crate) fn with_guards(&self, f: impl FnOnce(&mut ArenaGuards)) {
        let mut guards = self.get_arena_guards();
        f(&mut guards);
    }

    fn get_arena_guards(&self) -> ArenaGuards<'_> {
        ArenaGuards {
            container_id_to_idx: self.inner.container_id_to_idx.lock().unwrap(),
            container_idx_to_id: self.inner.container_idx_to_id.lock().unwrap(),
            depth: self.inner.depth.lock().unwrap(),
            parents: self.inner.parents.lock().unwrap(),
            root_c_idx: self.inner.root_c_idx.lock().unwrap(),
            parent_resolver: self.inner.parent_resolver.lock().unwrap(),
        }
    }

    pub fn register_container(&self, id: &ContainerID) -> ContainerIdx {
        let mut container_id_to_idx = self.inner.container_id_to_idx.lock().unwrap();
        if let Some(&idx) = container_id_to_idx.get(id) {
            return idx;
        }

        let mut container_idx_to_id = self.inner.container_idx_to_id.lock().unwrap();
        let idx = container_idx_to_id.len();
        container_idx_to_id.push(id.clone());
        let idx = ContainerIdx::from_index_and_type(idx as u32, id.container_type());
        container_id_to_idx.insert(id.clone(), idx);
        if id.is_root() {
            self.inner.root_c_idx.lock().unwrap().push(idx);
            self.inner.parents.lock().unwrap().insert(idx, None);
            self.inner.depth.lock().unwrap().push(NonZeroU16::new(1));
        } else {
            self.inner.depth.lock().unwrap().push(None);
        }
        idx
    }

    pub fn get_container_id(&self, idx: ContainerIdx) -> Option<ContainerID> {
        let lock = self.inner.container_idx_to_id.lock().unwrap();
        lock.get(idx.to_index() as usize).cloned()
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
            .container_id_to_idx
            .lock()
            .unwrap()
            .get(id)
            .copied()
    }

    #[inline]
    pub fn idx_to_id(&self, id: ContainerIdx) -> Option<ContainerID> {
        let lock = self.inner.container_idx_to_id.lock().unwrap();
        lock.get(id.to_index() as usize).cloned()
    }

    #[inline]
    pub fn with_idx_to_id<R>(&self, f: impl FnOnce(&Vec<ContainerID>) -> R) -> R {
        let lock = self.inner.container_idx_to_id.lock().unwrap();
        f(&lock)
    }

    pub fn alloc_str(&self, str: &str) -> StrAllocResult {
        let mut text_lock = self.inner.str.lock().unwrap();
        _alloc_str(&mut text_lock, str)
    }

    /// return slice and unicode index
    pub fn alloc_str_with_slice(&self, str: &str) -> (BytesSlice, StrAllocResult) {
        let mut text_lock = self.inner.str.lock().unwrap();
        _alloc_str_with_slice(&mut text_lock, str)
    }

    /// alloc str without extra info
    pub fn alloc_str_fast(&self, bytes: &[u8]) {
        let mut text_lock = self.inner.str.lock().unwrap();
        text_lock.alloc(std::str::from_utf8(bytes).unwrap());
    }

    #[inline]
    pub fn utf16_len(&self) -> usize {
        self.inner.str.lock().unwrap().len_utf16()
    }

    #[inline]
    pub fn alloc_value(&self, value: LoroValue) -> usize {
        let mut values_lock = self.inner.values.lock().unwrap();
        _alloc_value(&mut values_lock, value)
    }

    #[inline]
    pub fn alloc_values(&self, values: impl Iterator<Item = LoroValue>) -> std::ops::Range<usize> {
        let mut values_lock = self.inner.values.lock().unwrap();
        _alloc_values(&mut values_lock, values)
    }

    #[inline]
    pub fn set_parent(&self, child: ContainerIdx, parent: Option<ContainerIdx>) {
        let parents = &mut self.inner.parents.lock().unwrap();
        parents.insert(child, parent);
        let mut depth = self.inner.depth.lock().unwrap();

        match parent {
            Some(p) => {
                // Acquire the two maps as mutable guards so we can lazily register
                // unknown parents while computing depth.
                let mut idx_to_id_guard = self.inner.container_idx_to_id.lock().unwrap();
                let mut id_to_idx_guard = self.inner.container_id_to_idx.lock().unwrap();
                if let Some(d) = get_depth(
                    p,
                    &mut depth,
                    parents,
                    &self.inner.parent_resolver.lock().unwrap(),
                    &mut idx_to_id_guard,
                    &mut id_to_idx_guard,
                ) {
                    depth[child.to_index() as usize] = NonZeroU16::new(d.get() + 1);
                } else {
                    depth[child.to_index() as usize] = None;
                }
            }
            None => {
                depth[child.to_index() as usize] = NonZeroU16::new(1);
            }
        }
    }

    pub fn log_hierarchy(&self) {
        if cfg!(debug_assertions) {
            for (c, p) in self.inner.parents.lock().unwrap().iter() {
                tracing::info!(
                    "container {:?} {:?} {:?}",
                    c,
                    self.get_container_id(*c),
                    p.map(|x| self.get_container_id(x))
                );
            }
        }
    }

    pub fn log_all_containers(&self) {
        self.inner
            .container_id_to_idx
            .lock()
            .unwrap()
            .iter()
            .for_each(|(id, idx)| {
                tracing::info!("container {:?} {:?}", id, idx);
            });
        self.inner
            .container_idx_to_id
            .lock()
            .unwrap()
            .iter()
            .enumerate()
            .for_each(|(i, id)| {
                tracing::info!("container {} {:?}", i, id);
            });
    }

    pub fn get_parent(&self, child: ContainerIdx) -> Option<ContainerIdx> {
        if self.get_container_id(child).unwrap().is_root() {
            // TODO: PERF: we can speed this up by use a special bit in ContainerIdx to indicate
            // whether the target is a root container
            return None;
        }

        // Try fast path first
        if let Some(p) = self.inner.parents.lock().unwrap().get(&child).copied() {
            return p;
        }

        // Fallback: try to resolve parent lazily via the resolver if provided.
        let resolver = self.inner.parent_resolver.lock().unwrap().clone();
        if let Some(resolver) = resolver {
            let child_id = self.get_container_id(child).unwrap();
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
        self.inner.str.lock().unwrap().slice_by_unicode(range)
    }

    #[inline]
    pub fn slice_by_utf8(&self, range: impl RangeBounds<usize>) -> BytesSlice {
        self.inner.str.lock().unwrap().slice_bytes(range)
    }

    #[inline]
    pub fn slice_str_by_unicode_range(&self, range: Range<usize>) -> String {
        let mut s = self.inner.str.lock().unwrap();
        let s: &mut StrArena = &mut s;
        let mut ans = String::with_capacity(range.len());
        ans.push_str(s.slice_str_by_unicode(range));
        ans
    }

    #[inline]
    pub fn with_text_slice(&self, range: Range<usize>, mut f: impl FnMut(&str)) {
        f(self.inner.str.lock().unwrap().slice_str_by_unicode(range))
    }

    #[inline]
    pub fn get_value(&self, idx: usize) -> Option<LoroValue> {
        self.inner.values.lock().unwrap().get(idx).cloned()
    }

    #[inline]
    pub fn get_values(&self, range: Range<usize>) -> Vec<LoroValue> {
        (self.inner.values.lock().unwrap()[range]).to_vec()
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
        self.inner.str.lock().unwrap().is_empty() && self.inner.values.lock().unwrap().is_empty()
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
        self.inner.container_idx_to_id.lock().unwrap().clone()
    }

    pub fn export_parents(&self) -> Vec<Option<ContainerIdx>> {
        let parents = self.inner.parents.lock().unwrap();
        let containers = self.inner.container_idx_to_id.lock().unwrap();
        containers
            .iter()
            .enumerate()
            .map(|(x, id)| {
                let idx = ContainerIdx::from_index_and_type(x as u32, id.container_type());
                let parent_idx = parents.get(&idx)?;
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
        self.inner.root_c_idx.lock().unwrap().clone()
    }

    // TODO: this can return a u16 directly now, since the depths are always valid
    pub(crate) fn get_depth(&self, container: ContainerIdx) -> Option<NonZeroU16> {
        {
            let mut depth_guard = self.inner.depth.lock().unwrap();
            let parents_guard = self.inner.parents.lock().unwrap();
            let resolver_guard = self.inner.parent_resolver.lock().unwrap();
            let mut idx_to_id_guard = self.inner.container_idx_to_id.lock().unwrap();
            let mut id_to_idx_guard = self.inner.container_id_to_idx.lock().unwrap();
            get_depth(
                container,
                &mut depth_guard,
                &parents_guard,
                &resolver_guard,
                &mut idx_to_id_guard,
                &mut id_to_idx_guard,
            )
        }
    }

    pub(crate) fn iter_value_slice(
        &self,
        range: Range<usize>,
    ) -> impl Iterator<Item = LoroValue> + '_ {
        let values = self.inner.values.lock().unwrap();
        range
            .into_iter()
            .map(move |i| values.get(i).unwrap().clone())
    }

    pub(crate) fn get_root_container_idx_by_key(
        &self,
        root_index: &loro_common::InternalString,
    ) -> Option<ContainerIdx> {
        let inner = self.inner.container_id_to_idx.lock().unwrap();
        for t in loro_common::ContainerType::ALL_TYPES.iter() {
            let cid = ContainerID::Root {
                name: root_index.clone(),
                container_type: *t,
            };
            if let Some(idx) = inner.get(&cid) {
                return Some(*idx);
            }
        }
        None
    }

    #[allow(unused)]
    pub(crate) fn log_all_values(&self) {
        let values = self.inner.values.lock().unwrap();
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
    parents: &FxHashMap<ContainerIdx, Option<ContainerIdx>>,
    parent_resolver: &Option<Arc<ParentResolver>>,
    idx_to_id: &mut Vec<ContainerID>,
    id_to_idx: &mut FxHashMap<ContainerID, ContainerIdx>,
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
                if parent_id.is_root() {
                    depth.push(NonZeroU16::new(1));
                } else {
                    depth.push(None);
                }
                new_idx
            };
            Some(parent_idx)
        } else {
            return None;
        }
    };

    match parent {
        Some(p) => {
            d = NonZeroU16::new(
                get_depth(p, depth, parents, parent_resolver, idx_to_id, id_to_idx)?.get() + 1,
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
        let mut slot = self.inner.parent_resolver.lock().unwrap();
        *slot = resolver.map(|f| Arc::new(f) as Arc<ParentResolver>);
    }
}

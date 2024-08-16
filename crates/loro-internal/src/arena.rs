mod str_arena;

use std::{
    num::NonZeroU16,
    ops::{Range, RangeBounds},
    sync::{Arc, Mutex, MutexGuard},
};

use append_only_bytes::BytesSlice;
use fxhash::FxHashMap;
use loro_common::PeerID;

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

use self::str_arena::StrArena;

#[derive(Default, Debug)]
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
    str: Mutex<StrArena>,
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
                str: Mutex::new(self.inner.str.lock().unwrap().clone()),
            }),
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
                if let Some(d) = get_depth(p, &mut depth, parents) {
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

    pub fn log_all_container(&self) {
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
        self.inner
            .parents
            .lock()
            .unwrap()
            .get(&child)
            .copied()
            .flatten()
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
            container = self.get_parent(c);
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

    #[inline]
    pub fn root_containers(&self) -> Vec<ContainerIdx> {
        self.inner.root_c_idx.lock().unwrap().clone()
    }

    // TODO: this can return a u16 directly now, since the depths are always valid
    pub(crate) fn get_depth(&self, container: ContainerIdx) -> Option<NonZeroU16> {
        get_depth(
            container,
            &mut self.inner.depth.lock().unwrap(),
            &self.inner.parents.lock().unwrap(),
        )
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
) -> Option<NonZeroU16> {
    let mut d = depth[target.to_index() as usize];
    if d.is_some() {
        return d;
    }

    let parent = parents.get(&target)?;
    match parent {
        Some(p) => {
            d = NonZeroU16::new(get_depth(*p, depth, parents)?.get() + 1);
            depth[target.to_index() as usize] = d;
        }
        None => {
            depth[target.to_index() as usize] = NonZeroU16::new(1);
            d = NonZeroU16::new(1);
        }
    }

    d
}

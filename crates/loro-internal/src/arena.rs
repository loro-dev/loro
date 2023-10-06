mod str_arena;

use std::{
    ops::Range,
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
        map::{InnerMapSet, MapSet},
        richtext::StyleOp,
        text::text_content::SliceRange,
        ContainerID,
    },
    id::Counter,
    op::{InnerContent, Op, RawOp, RawOpContent},
    LoroValue,
};

use self::str_arena::StrArena;

#[derive(Default)]
struct InnerSharedArena {
    // The locks should not be exposed outside this file.
    // It might be better to use RwLock in the future
    container_idx_to_id: Mutex<Vec<ContainerID>>,
    container_id_to_idx: Mutex<FxHashMap<ContainerID, ContainerIdx>>,
    /// The parent of each container.
    parents: Mutex<FxHashMap<ContainerIdx, Option<ContainerIdx>>>,
    values: Mutex<Vec<LoroValue>>,
    root_c_idx: Mutex<Vec<ContainerIdx>>,
    str: Mutex<StrArena>,
}

/// This is shared between [OpLog] and [AppState].
///
#[derive(Default, Clone)]
pub struct SharedArena {
    inner: Arc<InnerSharedArena>,
}

pub struct StrAllocResult {
    pub start: usize,
    pub end: usize,
    // TODO: remove this field?
    pub utf16_len: usize,
}

pub(crate) struct OpConverter<'a> {
    container_idx_to_id: MutexGuard<'a, Vec<ContainerID>>,
    container_id_to_idx: MutexGuard<'a, FxHashMap<ContainerID, ContainerIdx>>,
    str: MutexGuard<'a, StrArena>,
    values: MutexGuard<'a, Vec<LoroValue>>,
    root_c_idx: MutexGuard<'a, Vec<ContainerIdx>>,
    parents: MutexGuard<'a, FxHashMap<ContainerIdx, Option<ContainerIdx>>>,
}

impl<'a> OpConverter<'a> {
    pub fn convert_single_op(
        &mut self,
        id: &ContainerID,
        peer: PeerID,
        counter: Counter,
        lamport: Lamport,
        content: RawOpContent,
    ) -> Op {
        let container = 'out: {
            if let Some(&idx) = self.container_id_to_idx.get(id) {
                break 'out idx;
            }

            let container_idx_to_id = &mut self.container_idx_to_id;
            let idx = container_idx_to_id.len();
            container_idx_to_id.push(id.clone());
            let idx = ContainerIdx::from_index_and_type(idx as u32, id.container_type());
            self.container_id_to_idx.insert(id.clone(), idx);
            if id.is_root() {
                self.root_c_idx.push(idx);
                self.parents.insert(idx, None);
            }

            idx
        };

        match content {
            crate::op::RawOpContent::Map(MapSet { key, value }) => {
                let value = if let Some(value) = value {
                    Some(_alloc_value(&mut self.values, value) as u32)
                } else {
                    None
                };
                Op {
                    counter,
                    container,
                    content: crate::op::InnerContent::Map(InnerMapSet { key, value }),
                }
            }
            crate::op::RawOpContent::List(list) => match list {
                ListOp::Insert { slice, pos } => match slice {
                    crate::text::text_content::ListSlice::RawData(values) => {
                        let range = _alloc_values(&mut self.values, values.iter().cloned());
                        Op {
                            counter,
                            container,
                            content: crate::op::InnerContent::List(InnerListOp::Insert {
                                slice: SliceRange::from(range.start as u32..range.end as u32),
                                pos,
                            }),
                        }
                    }
                    crate::text::text_content::ListSlice::RawStr {
                        str,
                        unicode_len: _,
                    } => {
                        let slice = _alloc_str(&mut self.str, &str);
                        Op {
                            counter,
                            container,
                            content: crate::op::InnerContent::List(InnerListOp::Insert {
                                slice: SliceRange::from(slice.start as u32..slice.end as u32),
                                pos,
                            }),
                        }
                    }
                },
                ListOp::Delete(span) => Op {
                    counter,
                    container,
                    content: InnerContent::List(InnerListOp::Delete(span)),
                },
                ListOp::StyleStart { pos, info, key } => Op {
                    counter,
                    container,
                    content: InnerContent::List(InnerListOp::StyleStart {
                        pos,
                        style: Arc::new(crate::container::richtext::StyleOp {
                            lamport,
                            peer,
                            cnt: counter,
                            key,
                            info,
                        }),
                    }),
                },
                ListOp::StyleEnd { pos, key, info } => Op {
                    counter,
                    container,
                    content: InnerContent::List(InnerListOp::StyleEnd {
                        pos,
                        // PERF we may reuse the style object created by StyleStart
                        style: Arc::new(crate::container::richtext::StyleOp {
                            lamport,
                            peer,
                            cnt: counter,
                            key,
                            info,
                        }),
                    }),
                },
            },
        }
    }
}

impl SharedArena {
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

    /// return utf16 len
    pub fn alloc_str(&self, str: &str) -> StrAllocResult {
        let mut text_lock = self.inner.str.lock().unwrap();
        _alloc_str(&mut text_lock, str)
    }

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
        self.inner.parents.lock().unwrap().insert(child, parent);
    }

    pub fn log_hierarchy(&self) {
        if cfg!(debug_assertions) {
            for (c, p) in self.inner.parents.lock().unwrap().iter() {
                debug_log::debug_log!(
                    "container {:?} {:?} {:?}",
                    c,
                    self.get_container_id(*c),
                    p.map(|x| self.get_container_id(x))
                );
            }
        }
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
    pub fn slice_str(&self, range: Range<usize>) -> String {
        let mut s = self.inner.str.lock().unwrap();
        _slice_str(range, &mut s)
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

    #[inline]
    pub fn slice_by_unicode(&self, range: Range<usize>) -> BytesSlice {
        self.inner.str.lock().unwrap().slice_by_unicode(range)
    }

    #[inline(always)]
    pub(crate) fn with_op_converter<R>(&self, f: impl FnOnce(&mut OpConverter) -> R) -> R {
        let mut op_converter = OpConverter {
            container_idx_to_id: self.inner.container_idx_to_id.lock().unwrap(),
            container_id_to_idx: self.inner.container_id_to_idx.lock().unwrap(),
            str: self.inner.str.lock().unwrap(),
            values: self.inner.values.lock().unwrap(),
            root_c_idx: self.inner.root_c_idx.lock().unwrap(),
            parents: self.inner.parents.lock().unwrap(),
        };
        f(&mut op_converter)
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

    pub fn is_empty(&self) -> bool {
        self.inner.container_idx_to_id.lock().unwrap().is_empty()
            && self.inner.container_id_to_idx.lock().unwrap().is_empty()
            && self.inner.str.lock().unwrap().is_empty()
            && self.inner.values.lock().unwrap().is_empty()
            && self.inner.parents.lock().unwrap().is_empty()
    }

    fn inner_convert_op(
        &self,
        content: RawOpContent<'_>,
        peer: PeerID,
        counter: i32,
        lamport: Lamport,
        container: ContainerIdx,
    ) -> Op {
        match content {
            crate::op::RawOpContent::Map(MapSet { key, value }) => {
                let value = value.map(|value| self.alloc_value(value) as u32);
                Op {
                    counter,
                    container,
                    content: crate::op::InnerContent::Map(InnerMapSet { key, value }),
                }
            }
            crate::op::RawOpContent::List(list) => match list {
                ListOp::Insert { slice, pos } => match slice {
                    crate::text::text_content::ListSlice::RawData(values) => {
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
                    crate::text::text_content::ListSlice::RawStr {
                        str,
                        unicode_len: _,
                    } => {
                        let slice = self.alloc_str(&str);
                        Op {
                            counter,
                            container,
                            content: crate::op::InnerContent::List(InnerListOp::Insert {
                                slice: SliceRange::from(slice.start as u32..slice.end as u32),
                                pos,
                            }),
                        }
                    }
                },
                ListOp::Delete(span) => Op {
                    counter,
                    container,
                    content: crate::op::InnerContent::List(InnerListOp::Delete(span)),
                },
                ListOp::StyleStart { pos, info, key } => Op {
                    counter,
                    container,
                    content: InnerContent::List(InnerListOp::StyleStart {
                        pos,
                        style: Arc::new(StyleOp {
                            lamport,
                            peer,
                            cnt: counter,
                            key,
                            info,
                        }),
                    }),
                },
                ListOp::StyleEnd { pos, key, info } => Op {
                    counter,
                    container,
                    content: InnerContent::List(InnerListOp::StyleEnd {
                        pos,
                        style: Arc::new(StyleOp {
                            lamport,
                            peer,
                            cnt: counter,
                            key,
                            info,
                        }),
                    }),
                },
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
    let start_wchars = text_lock.len_utf16();
    text_lock.alloc(str);
    StrAllocResult {
        utf16_len: text_lock.len_utf16() - start_wchars,
        start,
        end: text_lock.len_unicode(),
    }
}

fn _slice_str(range: Range<usize>, s: &mut StrArena) -> String {
    let mut ans = String::with_capacity(range.len());
    ans.push_str(s.slice_str_by_unicode(range));
    ans
}

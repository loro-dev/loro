use std::{
    ops::{Range, RangeBounds},
    sync::{atomic::AtomicUsize, Arc, Mutex},
};

use append_only_bytes::{AppendOnlyBytes, BytesSlice};
use fxhash::FxHashMap;

use crate::{
    container::{
        list::list_op::{InnerListOp, ListOp},
        map::InnerMapSet,
        registry::ContainerIdx,
        text::text_content::SliceRange,
        ContainerID,
    },
    id::Counter,
    op::{Op, RawOp, RawOpContent},
    text::utf16::count_utf16_chars,
    LoroValue,
};

#[derive(Default)]
struct InnerSharedArena {
    // The locks should not be exposed outside this file.
    // It might be better to use RwLock in the future
    container_idx_to_id: Mutex<Vec<ContainerID>>,
    container_id_to_idx: Mutex<FxHashMap<ContainerID, ContainerIdx>>,
    /// The parent of each container.
    parents: Mutex<FxHashMap<ContainerIdx, Option<ContainerIdx>>>,
    text: Mutex<AppendOnlyBytes>,
    text_utf16_len: AtomicUsize,
    values: Mutex<Vec<LoroValue>>,
    root_c_idx: Mutex<Vec<ContainerIdx>>,
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
    pub utf16_len: usize,
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

    pub fn idx_to_id(&self, id: ContainerIdx) -> Option<ContainerID> {
        let lock = self.inner.container_idx_to_id.lock().unwrap();
        lock.get(id.to_index() as usize).cloned()
    }

    /// return utf16 len
    pub fn alloc_str(&self, str: &str) -> StrAllocResult {
        let mut text_lock = self.inner.text.lock().unwrap();
        let start = text_lock.len();
        let utf16_len = count_utf16_chars(str.as_bytes());
        text_lock.push_slice(str.as_bytes());
        self.inner
            .text_utf16_len
            .fetch_add(utf16_len, std::sync::atomic::Ordering::SeqCst);
        StrAllocResult {
            start,
            end: text_lock.len(),
            utf16_len,
        }
    }

    pub fn alloc_str_fast(&self, bytes: &[u8]) {
        let mut text_lock = self.inner.text.lock().unwrap();
        let utf16_len = count_utf16_chars(bytes);
        self.inner
            .text_utf16_len
            .fetch_add(utf16_len, std::sync::atomic::Ordering::SeqCst);
        text_lock.push_slice(bytes);
    }

    pub fn utf16_len(&self) -> usize {
        self.inner
            .text_utf16_len
            .load(std::sync::atomic::Ordering::SeqCst)
    }

    pub fn alloc_value(&self, value: LoroValue) -> usize {
        let mut values_lock = self.inner.values.lock().unwrap();
        values_lock.push(value);
        values_lock.len() - 1
    }

    pub fn alloc_values(&self, values: impl Iterator<Item = LoroValue>) -> std::ops::Range<usize> {
        let mut values_lock = self.inner.values.lock().unwrap();
        values_lock.reserve(values.size_hint().0);
        let start = values_lock.len();
        for value in values {
            values_lock.push(value);
        }

        start..values_lock.len()
    }

    pub fn set_parent(&self, child: ContainerIdx, parent: Option<ContainerIdx>) {
        debug_log::debug_log!(
            "set parent {:?} {:?} {:?} {:?}",
            child,
            parent,
            self.get_container_id(child),
            parent.map(|x| self.get_container_id(x))
        );
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
        self.log_hierarchy();
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

    pub fn slice_bytes(&self, range: impl RangeBounds<usize>) -> BytesSlice {
        self.inner.text.lock().unwrap().slice(range)
    }

    pub fn get_value(&self, idx: usize) -> Option<LoroValue> {
        self.inner.values.lock().unwrap().get(idx).cloned()
    }

    pub fn get_values(&self, range: Range<usize>) -> Vec<LoroValue> {
        (self.inner.values.lock().unwrap()[range]).to_vec()
    }

    pub fn convert_single_op(
        &self,
        container: &ContainerID,
        counter: Counter,
        content: RawOpContent,
    ) -> Op {
        let container = self.register_container(container);
        self.inner_convert_op(content, counter, container)
    }

    pub fn is_empty(&self) -> bool {
        self.inner.container_idx_to_id.lock().unwrap().is_empty()
            && self.inner.container_id_to_idx.lock().unwrap().is_empty()
            && self.inner.text.lock().unwrap().is_empty()
            && self.inner.values.lock().unwrap().is_empty()
            && self.inner.parents.lock().unwrap().is_empty()
    }

    fn inner_convert_op(
        &self,
        content: RawOpContent<'_>,
        counter: i32,
        container: ContainerIdx,
    ) -> Op {
        match content {
            crate::op::RawOpContent::Map(map) => {
                let value = self.alloc_value(map.value) as u32;
                Op {
                    counter,
                    container,
                    content: crate::op::InnerContent::Map(InnerMapSet {
                        key: map.key,
                        value,
                    }),
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
                    crate::text::text_content::ListSlice::RawStr(str) => {
                        let bytes = self.alloc_str(&str);
                        Op {
                            counter,
                            container,
                            content: crate::op::InnerContent::List(InnerListOp::Insert {
                                slice: SliceRange::from(bytes.start as u32..bytes.end as u32),
                                pos,
                            }),
                        }
                    }
                    crate::text::text_content::ListSlice::Unknown(u) => Op {
                        counter,
                        container,
                        content: crate::op::InnerContent::List(InnerListOp::Insert {
                            slice: SliceRange::new_unknown(u as u32),
                            pos,
                        }),
                    },
                    crate::text::text_content::ListSlice::RawBytes(x) => {
                        let bytes = self.alloc_str(std::str::from_utf8(&x).unwrap());
                        Op {
                            counter,
                            container,
                            content: crate::op::InnerContent::List(InnerListOp::Insert {
                                slice: SliceRange::from(bytes.start as u32..bytes.end as u32),
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
            },
        }
    }

    pub fn convert_raw_op(&self, op: &RawOp) -> Op {
        self.inner_convert_op(op.content.clone(), op.id.counter, op.container)
    }

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

    pub fn root_containers(&self) -> Vec<ContainerIdx> {
        self.inner.root_c_idx.lock().unwrap().clone()
    }
}

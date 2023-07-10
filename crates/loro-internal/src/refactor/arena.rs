use std::sync::{atomic::AtomicUsize, Arc, Mutex};

use append_only_bytes::{AppendOnlyBytes, BytesSlice};
use fxhash::FxHashMap;
use smallvec::SmallVec;

use crate::{
    container::{
        list::list_op::{InnerListOp, ListOp},
        map::InnerMapSet,
        registry::ContainerIdx,
        text::text_content::SliceRange,
        ContainerID,
    },
    id::Counter,
    op::{Op, RemoteContent, RemoteOp},
    text::utf16::count_utf16_chars,
    LoroValue,
};

/// This is shared between [OpLog] and [AppState].
///
#[derive(Clone, Default)]
pub(super) struct SharedArena {
    // The locks should not be exposed outside this file.
    // It might be better to use RwLock in the future
    container_idx_to_id: Arc<Mutex<Vec<ContainerID>>>,
    container_id_to_idx: Arc<Mutex<FxHashMap<ContainerID, ContainerIdx>>>,
    /// The parent of each container.
    parents: Arc<Mutex<FxHashMap<ContainerIdx, Option<ContainerIdx>>>>,
    text: Arc<Mutex<AppendOnlyBytes>>,
    text_utf16_len: Arc<AtomicUsize>,
    values: Arc<Mutex<Vec<Arc<LoroValue>>>>,
}

pub(crate) struct StrAllocResult {
    pub start: usize,
    pub end: usize,
    pub utf16_len: usize,
}

impl SharedArena {
    pub fn register_container(&self, id: &ContainerID) -> ContainerIdx {
        let mut container_id_to_idx = self.container_id_to_idx.lock().unwrap();
        if let Some(&idx) = container_id_to_idx.get(id) {
            return idx;
        }

        let mut container_idx_to_id = self.container_idx_to_id.lock().unwrap();
        let idx = container_idx_to_id.len();
        container_idx_to_id.push(id.clone());
        let ans = ContainerIdx::from_u32(idx as u32);
        container_id_to_idx.insert(id.clone(), ans);
        ans
    }

    pub fn id_to_idx(&self, id: &ContainerID) -> Option<ContainerIdx> {
        self.container_id_to_idx.lock().unwrap().get(id).copied()
    }

    pub fn idx_to_id(&self, id: ContainerIdx) -> Option<ContainerID> {
        let lock = self.container_idx_to_id.lock().unwrap();
        lock.get(id.to_u32() as usize).cloned()
    }

    /// return utf16 len
    pub fn alloc_str(&self, str: &str) -> StrAllocResult {
        let mut text_lock = self.text.lock().unwrap();
        let start = text_lock.len();
        let utf16_len = count_utf16_chars(str.as_bytes());
        text_lock.push_slice(str.as_bytes());
        self.text_utf16_len
            .fetch_add(utf16_len, std::sync::atomic::Ordering::SeqCst);
        StrAllocResult {
            start,
            end: text_lock.len(),
            utf16_len,
        }
    }

    pub fn utf16_len(&self) -> usize {
        self.text_utf16_len
            .load(std::sync::atomic::Ordering::SeqCst)
    }

    pub fn alloc_value(&self, value: LoroValue) -> usize {
        let mut values_lock = self.values.lock().unwrap();
        values_lock.push(Arc::new(value));
        values_lock.len() - 1
    }

    pub fn alloc_values(&self, values: impl Iterator<Item = LoroValue>) -> std::ops::Range<usize> {
        let mut values_lock = self.values.lock().unwrap();
        let start = values_lock.len();
        for value in values {
            values_lock.push(Arc::new(value));
        }

        start..values_lock.len()
    }

    pub fn set_parent(&self, child: ContainerIdx, parent: Option<ContainerIdx>) {
        self.parents.lock().unwrap().insert(child, parent);
    }

    pub fn get_parent(&self, child: ContainerIdx) -> Option<ContainerIdx> {
        self.parents.lock().unwrap().get(&child).copied().flatten()
    }

    pub fn slice_bytes(&self, range: std::ops::Range<usize>) -> BytesSlice {
        self.text.lock().unwrap().slice(range)
    }

    pub fn get_value(&self, idx: usize) -> Option<Arc<LoroValue>> {
        self.values.lock().unwrap().get(idx).cloned()
    }

    pub fn convert_single_op(
        &mut self,
        container: &ContainerID,
        counter: Counter,
        content: RemoteContent,
    ) -> Op {
        let container = self.register_container(container);
        match content {
            crate::op::RemoteContent::Map(map) => {
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
            crate::op::RemoteContent::List(list) => match list {
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
                },
                ListOp::Delete(span) => Op {
                    counter,
                    container,
                    content: crate::op::InnerContent::List(InnerListOp::Delete(span)),
                },
            },
        }
    }
}

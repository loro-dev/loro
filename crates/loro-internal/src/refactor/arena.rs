use std::sync::Arc;

use append_only_bytes::{AppendOnlyBytes, BytesSlice};
use im::Vector;

use crate::{
    container::{registry::ContainerIdx, ContainerID},
    text::utf16::count_utf16_chars,
    LoroValue,
};

/// This is shared between [OpLog] and [AppState].
/// It only takes O(1) to have a readonly view cloned.
/// It makes ownership problem easier.
///
#[derive(Clone, Default)]
pub(super) struct SharedArena {
    container_idx_to_id: Vector<ContainerID>,
    container_id_to_idx: im::HashMap<ContainerID, ContainerIdx>,
    /// The parent of each container.
    parents: im::HashMap<ContainerIdx, Option<ContainerIdx>>,
    text: AppendOnlyBytes,
    text_utf16_len: usize,
    values: Vector<Arc<LoroValue>>,
}

#[derive(Clone)]
pub(super) struct ReadonlyArena {
    container_idx_to_id: Vector<ContainerID>,
    container_id_to_idx: im::HashMap<ContainerID, ContainerIdx>,
    /// The parent of each container.
    parents: im::HashMap<ContainerIdx, Option<ContainerIdx>>,
    bytes: BytesSlice,
    text_utf16_len: usize,
    values: Vector<Arc<LoroValue>>,
}

pub(crate) struct StrAllocResult {
    pub start: usize,
    pub end: usize,
    pub utf16_len: usize,
}

impl SharedArena {
    pub fn register_container(&mut self, id: &ContainerID) -> ContainerIdx {
        if let Some(&idx) = self.container_id_to_idx.get(id) {
            return idx;
        }

        let idx = self.container_idx_to_id.len();
        self.container_idx_to_id.push_back(id.clone());
        let ans = ContainerIdx::from_u32(idx as u32);
        self.container_id_to_idx.insert(id.clone(), ans);
        ans
    }

    pub fn id_to_idx(&self, id: &ContainerID) -> Option<ContainerIdx> {
        self.container_id_to_idx.get(id).copied()
    }

    pub fn idx_to_id(&self, id: ContainerIdx) -> Option<&ContainerID> {
        self.container_idx_to_id.get(id.to_u32() as usize)
    }

    /// return utf16 len
    pub fn alloc_str(&mut self, str: &str) -> StrAllocResult {
        let start = self.text.len();
        let utf16_len = count_utf16_chars(str.as_bytes());
        self.text.push_slice(str.as_bytes());
        self.text_utf16_len += utf16_len;
        StrAllocResult {
            start,
            end: self.text.len(),
            utf16_len,
        }
    }

    pub fn utf16_len(&self) -> usize {
        self.text_utf16_len
    }

    pub fn alloc_value(&mut self, value: LoroValue) -> usize {
        self.values.push_back(Arc::new(value));
        self.values.len() - 1
    }

    pub fn alloc_values(
        &mut self,
        values: impl Iterator<Item = LoroValue>,
    ) -> std::ops::Range<usize> {
        let start = self.values.len();
        for value in values {
            self.values.push_back(Arc::new(value));
        }

        start..self.values.len()
    }

    pub fn to_readonly(&self) -> ReadonlyArena {
        ReadonlyArena {
            container_idx_to_id: self.container_idx_to_id.clone(),
            container_id_to_idx: self.container_id_to_idx.clone(),
            parents: self.parents.clone(),
            bytes: self.text.slice(..),
            values: self.values.clone(),
            text_utf16_len: self.text_utf16_len,
        }
    }

    pub fn set_parent(&mut self, child: ContainerIdx, parent: Option<ContainerIdx>) {
        self.parents.insert(child, parent);
    }

    pub fn get_parent(&self, child: ContainerIdx) -> Option<ContainerIdx> {
        self.parents.get(&child).copied().flatten()
    }

    pub fn slice_bytes(&self, range: std::ops::Range<usize>) -> &[u8] {
        &self.text[range]
    }

    pub fn get_value(&self, idx: usize) -> Option<&Arc<LoroValue>> {
        self.values.get(idx)
    }
}

impl ReadonlyArena {
    pub fn id_to_idx(&self, id: &ContainerID) -> Option<ContainerIdx> {
        self.container_id_to_idx.get(id).copied()
    }

    pub fn idx_to_id(&self, id: ContainerIdx) -> Option<&ContainerID> {
        self.container_idx_to_id.get(id.to_u32() as usize)
    }

    pub fn slice_bytes(&self, range: std::ops::Range<usize>) -> &[u8] {
        &self.bytes[range]
    }

    pub fn get_value(&self, idx: usize) -> Option<&Arc<LoroValue>> {
        self.values.get(idx)
    }

    pub fn get_parent(&self, child: ContainerIdx) -> Option<ContainerIdx> {
        self.parents.get(&child).copied().flatten()
    }
}

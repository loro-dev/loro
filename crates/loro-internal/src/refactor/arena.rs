use append_only_bytes::{AppendOnlyBytes, BytesSlice};
use im::Vector;

use crate::{
    container::{registry::ContainerIdx, ContainerID},
    LoroValue,
};

/// This is shared between [OpLog] and [AppState].
/// It uses a immutable data structure inside so that we have O(1) clone time.
/// It can make sharing data between threads easier.
///
#[derive(Clone, Default)]
pub(super) struct SharedArena {
    container_idx_to_id: Vector<ContainerID>,
    container_id_to_idx: im::HashMap<ContainerID, ContainerIdx>,
    /// The parent of each container.
    parents: im::HashMap<ContainerIdx, Option<ContainerIdx>>,
    bytes: AppendOnlyBytes,
    values: Vector<LoroValue>,
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

    pub fn alloc_bytes(&mut self, bytes: &[u8]) -> BytesSlice {
        let start = self.bytes.len();
        self.bytes.push_slice(bytes);
        self.bytes.slice(start..self.bytes.len())
    }

    pub fn alloc_value(&mut self, value: LoroValue) -> usize {
        self.values.push_back(value);
        self.values.len() - 1
    }

    pub fn alloc_values(&mut self, values: impl Iterator<Item = LoroValue>) -> (usize, usize) {
        let start = self.values.len();
        for value in values {
            self.values.push_back(value);
        }

        (start, self.values.len())
    }
}

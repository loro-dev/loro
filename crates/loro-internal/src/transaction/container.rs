use crate::{container::registry::ContainerIdx, ContainerType};


pub struct TransactionalContainer {
    pub idx: ContainerIdx,
    pub type_: ContainerType,
}

impl TransactionalContainer {
    pub fn new(idx: ContainerIdx, type_: ContainerType) -> Self {
        Self { idx, type_ }
    }
}

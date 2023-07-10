use crate::container::{registry::ContainerIdx, ContainerID};

use super::txn::Transaction;

pub struct Text {
    container_id: ContainerID,
}

impl From<ContainerID> for Text {
    fn from(container_id: ContainerID) -> Self {
        Self { container_id }
    }
}

impl Text {
    pub fn insert(&self, txn: &mut Transaction, pos: usize, s: &str) {}
}

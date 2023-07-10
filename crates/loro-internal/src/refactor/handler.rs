use crate::container::registry::ContainerIdx;

use super::txn::Transaction;

pub struct Text {
    container_idx: ContainerIdx,
}

impl Text {
    pub fn insert(&self, txn: &Transaction, pos: usize, s: &str) {}
}

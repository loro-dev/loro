use std::borrow::Cow;

use crate::container::{
    list::list_op::{DeleteSpan, ListOp},
    registry::ContainerIdx,
    text::text_content::ListSlice,
};

use super::txn::Transaction;

pub struct Text {
    container_idx: ContainerIdx,
}

impl From<ContainerIdx> for Text {
    fn from(container_idx: ContainerIdx) -> Self {
        Self { container_idx }
    }
}

impl Text {
    pub fn insert(&self, txn: &mut Transaction, pos: usize, s: &str) {
        txn.apply_local_op(
            self.container_idx,
            crate::op::RawOpContent::List(crate::container::list::list_op::ListOp::Insert {
                slice: ListSlice::RawStr(Cow::Borrowed(s)),
                pos,
            }),
        );
    }

    pub fn delete(&self, txn: &mut Transaction, pos: usize, len: usize) {
        txn.apply_local_op(
            self.container_idx,
            crate::op::RawOpContent::List(ListOp::Delete(DeleteSpan {
                pos: pos as isize,
                len: len as isize,
            })),
        );
    }
}

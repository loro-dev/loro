use std::{borrow::Cow, sync::Arc};

use crate::{
    container::{
        list::list_op::{DeleteSpan, ListOp},
        registry::ContainerIdx,
        text::text_content::ListSlice,
    },
    LoroValue,
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
        if s.is_empty() {
            return;
        }

        txn.apply_local_op(
            self.container_idx,
            crate::op::RawOpContent::List(crate::container::list::list_op::ListOp::Insert {
                slice: ListSlice::RawStr(Cow::Borrowed(s)),
                pos,
            }),
        );
    }

    pub fn delete(&self, txn: &mut Transaction, pos: usize, len: usize) {
        if len == 0 {
            return;
        }

        txn.apply_local_op(
            self.container_idx,
            crate::op::RawOpContent::List(ListOp::Delete(DeleteSpan {
                pos: pos as isize,
                len: len as isize,
            })),
        );
    }

    pub fn get_value(&self, txn: &Transaction) -> LoroValue {
        LoroValue::String(
            txn.get_value_by_idx(self.container_idx)
                .into_string()
                .unwrap_or_else(|_| Arc::new(String::new())),
        )
    }
}

#[cfg(test)]
mod test {

    use crate::refactor::loro::LoroApp;

    #[test]
    fn test() {
        let loro = LoroApp::new();
        let mut txn = loro.txn().unwrap();
        let text = txn.get_text("hello").unwrap();
        text.insert(&mut txn, 0, "hello");
        assert_eq!(&**text.get_value(&txn).as_string().unwrap(), "hello");
        text.insert(&mut txn, 2, " kk ");
        assert_eq!(&**text.get_value(&txn).as_string().unwrap(), "he kk llo");
        txn.abort();
        let mut txn = loro.txn().unwrap();
        assert_eq!(&**text.get_value(&txn).as_string().unwrap(), "");
        text.insert(&mut txn, 0, "hi");
        txn.commit().unwrap();
        let txn = loro.txn().unwrap();
        assert_eq!(&**text.get_value(&txn).as_string().unwrap(), "hi");
    }

    #[test]
    fn import() {
        let loro = LoroApp::new();
        loro.set_peer_id(1);
        let loro2 = LoroApp::new();
        loro2.set_peer_id(2);

        let mut txn = loro.txn().unwrap();
        let text = txn.get_text("hello").unwrap();
        text.insert(&mut txn, 0, "hello");
        txn.commit().unwrap();
        let exported = loro.export_from(&Default::default());
        loro2.import(&exported).unwrap();
        let mut txn = loro2.txn().unwrap();
        let text = txn.get_text("hello").unwrap();
        assert_eq!(&**text.get_value(&txn).as_string().unwrap(), "hello");
        text.insert(&mut txn, 5, " world");
        assert_eq!(&**text.get_value(&txn).as_string().unwrap(), "hello world");
        txn.commit().unwrap();
        loro.import(&loro2.export_from(&Default::default()))
            .unwrap();
        let txn = loro.txn().unwrap();
        let text = txn.get_text("hello").unwrap();
        assert_eq!(&**text.get_value(&txn).as_string().unwrap(), "hello world");
    }
}

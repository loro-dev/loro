use super::{state::DocState, txn::Transaction};
use crate::container::{
    list::list_op::{DeleteSpan, ListOp},
    registry::ContainerIdx,
    text::text_content::ListSlice,
};
use loro_common::{ContainerID, ContainerType, LoroValue};
use std::{
    borrow::Cow,
    sync::{Mutex, Weak},
};

pub struct TextHandler {
    container_idx: ContainerIdx,
    state: Weak<Mutex<DocState>>,
}

pub struct MapHandler {
    container_idx: ContainerIdx,
    state: Weak<Mutex<DocState>>,
}

pub struct ListHandler {
    container_idx: ContainerIdx,
    state: Weak<Mutex<DocState>>,
}

impl TextHandler {
    pub fn new(idx: ContainerIdx, state: Weak<Mutex<DocState>>) -> Self {
        assert_eq!(idx.get_type(), ContainerType::Text);
        Self {
            container_idx: idx,
            state,
        }
    }

    pub fn get_value(&self) -> LoroValue {
        self.state
            .upgrade()
            .unwrap()
            .lock()
            .unwrap()
            .get_value_by_idx(self.container_idx)
    }

    pub fn id(&self) -> ContainerID {
        self.state
            .upgrade()
            .unwrap()
            .lock()
            .unwrap()
            .arena
            .idx_to_id(self.container_idx)
            .unwrap()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn len_utf16(&self) -> usize {
        self.state
            .upgrade()
            .unwrap()
            .lock()
            .unwrap()
            .with_state(self.container_idx, |state| {
                state.as_text_state().as_ref().unwrap().len_wchars()
            })
    }

    pub fn len_utf8(&self) -> usize {
        self.state
            .upgrade()
            .unwrap()
            .lock()
            .unwrap()
            .with_state(self.container_idx, |state| {
                state.as_text_state().as_ref().unwrap().len()
            })
    }
}

#[cfg(not(features = "wasm"))]
impl TextHandler {
    #[inline(always)]
    pub fn insert(&self, txn: &mut Transaction, pos: usize, s: &str) {
        self.insert_utf8(txn, pos, s)
    }

    #[inline(always)]
    pub fn delete(&self, txn: &mut Transaction, pos: usize, len: usize) {
        self.delete_utf8(txn, pos, len)
    }

    #[inline(always)]
    pub fn len(&self) -> usize {
        self.len_utf8()
    }

    pub fn insert_utf8(&self, txn: &mut Transaction, pos: usize, s: &str) {
        if s.is_empty() {
            return;
        }

        txn.apply_local_op(
            self.container_idx,
            crate::op::RawOpContent::List(crate::container::list::list_op::ListOp::Insert {
                slice: ListSlice::RawStr(Cow::Borrowed(s)),
                pos,
            }),
            None,
        );
    }

    pub fn delete_utf8(&self, txn: &mut Transaction, pos: usize, len: usize) {
        if len == 0 {
            return;
        }

        txn.apply_local_op(
            self.container_idx,
            crate::op::RawOpContent::List(ListOp::Delete(DeleteSpan {
                pos: pos as isize,
                len: len as isize,
            })),
            None,
        );
    }

    pub fn insert_utf16(&self, txn: &mut Transaction, pos: usize, s: &str) {
        if s.is_empty() {
            return;
        }

        let start =
            self.state
                .upgrade()
                .unwrap()
                .lock()
                .unwrap()
                .with_state(self.container_idx, |state| {
                    let text_state = &state.as_text_state();
                    let text = text_state.as_ref().unwrap();
                    text.utf16_to_utf8(pos)
                });

        txn.apply_local_op(
            self.container_idx,
            crate::op::RawOpContent::List(crate::container::list::list_op::ListOp::Insert {
                slice: ListSlice::RawStr(Cow::Borrowed(s)),
                pos: start,
            }),
            None,
        );
    }

    pub fn delete_utf16(&self, txn: &mut Transaction, pos: usize, del: usize) {
        if del == 0 {
            return;
        }

        let (start, end) =
            self.state
                .upgrade()
                .unwrap()
                .lock()
                .unwrap()
                .with_state(self.container_idx, |state| {
                    let text_state = &state.as_text_state();
                    let text = text_state.as_ref().unwrap();
                    (text.utf16_to_utf8(pos), text.utf16_to_utf8(pos + del))
                });
        txn.apply_local_op(
            self.container_idx,
            crate::op::RawOpContent::List(ListOp::Delete(DeleteSpan {
                pos: start as isize,
                len: (end - start) as isize,
            })),
            None,
        );
    }
}

#[cfg(features = "wasm")]
impl TextHandler {
    #[inline(always)]
    pub fn len(&self) -> usize {
        self.len_utf16()
    }

    #[inline(always)]
    pub fn delete(&self, txn: &mut Transaction, pos: usize, del: usize) {
        self.delete_utf16(txn, pos, del)
    }

    #[inline(always)]
    pub fn insert(&self, txn: &mut Transaction, pos: usize, s: &str) {
        self.insert_utf16(txn, pos, s)
    }

    pub fn len_utf16(&self) -> usize {
        self.state
            .upgrade()
            .unwrap()
            .lock()
            .unwrap()
            .with_state(self.container_idx, |state| {
                state.as_text_state().as_ref().unwrap().len_wchars()
            })
    }

    pub fn insert_utf16(&self, txn: &mut Transaction, pos: usize, s: &str) {
        if s.is_empty() {
            return;
        }

        let start =
            self.state
                .upgrade()
                .unwrap()
                .lock()
                .unwrap()
                .with_state(self.container_idx, |state| {
                    let text_state = &state.as_text_state();
                    let text = text_state.as_ref().unwrap();
                    text.utf16_to_utf8(pos)
                });

        txn.apply_local_op(
            self.container_idx,
            crate::op::RawOpContent::List(crate::container::list::list_op::ListOp::Insert {
                slice: ListSlice::RawStr(Cow::Borrowed(s)),
                pos: start,
            }),
            Some(EventHint::Utf16 { pos, len: 0 }),
        );
    }

    pub fn delete_utf16(&self, txn: &mut Transaction, pos: usize, del: usize) {
        if del == 0 {
            return;
        }

        let (start, end) =
            self.state
                .upgrade()
                .unwrap()
                .lock()
                .unwrap()
                .with_state(self.container_idx, |state| {
                    let text_state = &state.as_text_state();
                    let text = text_state.as_ref().unwrap();
                    (text.utf16_to_utf8(pos), text.utf16_to_utf8(pos + del))
                });
        txn.apply_local_op(
            self.container_idx,
            crate::op::RawOpContent::List(ListOp::Delete(DeleteSpan {
                pos: start as isize,
                len: (end - start) as isize,
            })),
            Some(EventHint::Utf16 { pos, len: del }),
        );
    }
}

impl ListHandler {
    pub fn new(idx: ContainerIdx, state: Weak<Mutex<DocState>>) -> Self {
        assert_eq!(idx.get_type(), ContainerType::List);
        Self {
            container_idx: idx,
            state,
        }
    }

    pub fn insert(&self, txn: &mut Transaction, pos: usize, v: LoroValue) {
        if let Some(container) = v.as_container() {
            self.insert_container(txn, pos, container.container_type());
            return;
        }

        txn.apply_local_op(
            self.container_idx,
            crate::op::RawOpContent::List(crate::container::list::list_op::ListOp::Insert {
                slice: ListSlice::RawData(Cow::Owned(vec![v])),
                pos,
            }),
            None,
        );
    }

    pub fn insert_container(
        &self,
        txn: &mut Transaction,
        pos: usize,
        c_type: ContainerType,
    ) -> ContainerIdx {
        let id = txn.next_id();
        let container_id = ContainerID::new_normal(id, c_type);
        let child_idx = txn.arena.register_container(&container_id);
        txn.arena.set_parent(child_idx, Some(self.container_idx));
        let v = LoroValue::Container(container_id);
        txn.apply_local_op(
            self.container_idx,
            crate::op::RawOpContent::List(crate::container::list::list_op::ListOp::Insert {
                slice: ListSlice::RawData(Cow::Owned(vec![v])),
                pos,
            }),
            None,
        );
        child_idx
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
            None,
        );
    }

    pub(crate) fn len(&self) -> usize {
        self.state
            .upgrade()
            .unwrap()
            .lock()
            .unwrap()
            .with_state(self.container_idx, |state| {
                state.as_list_state().as_ref().unwrap().len()
            })
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn get_value(&self) -> LoroValue {
        self.state
            .upgrade()
            .unwrap()
            .lock()
            .unwrap()
            .get_value_by_idx(self.container_idx)
    }

    pub fn id(&self) -> ContainerID {
        self.state
            .upgrade()
            .unwrap()
            .lock()
            .unwrap()
            .arena
            .idx_to_id(self.container_idx)
            .unwrap()
    }
}

impl MapHandler {
    pub fn new(idx: ContainerIdx, state: Weak<Mutex<DocState>>) -> Self {
        assert_eq!(idx.get_type(), ContainerType::Map);
        Self {
            container_idx: idx,
            state,
        }
    }

    pub fn insert(&self, txn: &mut Transaction, key: &str, value: LoroValue) {
        if let Some(value) = value.as_container() {
            self.insert_container(txn, key, value.container_type());
            return;
        }

        txn.apply_local_op(
            self.container_idx,
            crate::op::RawOpContent::Map(crate::container::map::MapSet {
                key: key.into(),
                value,
            }),
            None,
        );
    }

    pub fn insert_container(
        &self,
        txn: &mut Transaction,
        key: &str,
        c_type: ContainerType,
    ) -> ContainerIdx {
        let id = txn.next_id();
        let container_id = ContainerID::new_normal(id, c_type);
        let child_idx = txn.arena.register_container(&container_id);
        txn.arena.set_parent(child_idx, Some(self.container_idx));
        txn.apply_local_op(
            self.container_idx,
            crate::op::RawOpContent::Map(crate::container::map::MapSet {
                key: key.into(),
                value: LoroValue::Container(container_id),
            }),
            None,
        );
        child_idx
    }

    pub fn delete(&self, txn: &mut Transaction, key: &str) {
        txn.apply_local_op(
            self.container_idx,
            crate::op::RawOpContent::Map(crate::container::map::MapSet {
                key: key.into(),
                // TODO: use another special value to delete?
                value: LoroValue::Null,
            }),
            None,
        );
    }

    pub fn get_value(&self) -> LoroValue {
        self.state
            .upgrade()
            .unwrap()
            .lock()
            .unwrap()
            .get_value_by_idx(self.container_idx)
    }

    pub fn id(&self) -> ContainerID {
        self.state
            .upgrade()
            .unwrap()
            .lock()
            .unwrap()
            .arena
            .idx_to_id(self.container_idx)
            .unwrap()
    }
}

#[cfg(test)]
mod test {

    use crate::refactor::loro::LoroDoc;

    #[test]
    fn test() {
        let loro = LoroDoc::new();
        let mut txn = loro.txn().unwrap();
        let text = txn.get_text("hello");
        text.insert(&mut txn, 0, "hello");
        assert_eq!(&**text.get_value().as_string().unwrap(), "hello");
        text.insert(&mut txn, 2, " kk ");
        assert_eq!(&**text.get_value().as_string().unwrap(), "he kk llo");
        txn.abort();
        let mut txn = loro.txn().unwrap();
        assert_eq!(&**text.get_value().as_string().unwrap(), "");
        text.insert(&mut txn, 0, "hi");
        txn.commit().unwrap();
        assert_eq!(&**text.get_value().as_string().unwrap(), "hi");
    }

    #[test]
    fn import() {
        let loro = LoroDoc::new();
        loro.set_peer_id(1);
        let loro2 = LoroDoc::new();
        loro2.set_peer_id(2);

        let mut txn = loro.txn().unwrap();
        let text = txn.get_text("hello");
        text.insert(&mut txn, 0, "hello");
        txn.commit().unwrap();
        let exported = loro.export_from(&Default::default());
        loro2.import(&exported).unwrap();
        let mut txn = loro2.txn().unwrap();
        let text = txn.get_text("hello");
        assert_eq!(&**text.get_value().as_string().unwrap(), "hello");
        text.insert(&mut txn, 5, " world");
        assert_eq!(&**text.get_value().as_string().unwrap(), "hello world");
        txn.commit().unwrap();
        loro.import(&loro2.export_from(&Default::default()))
            .unwrap();
        let txn = loro.txn().unwrap();
        let text = txn.get_text("hello");
        assert_eq!(&**text.get_value().as_string().unwrap(), "hello world");
    }
}

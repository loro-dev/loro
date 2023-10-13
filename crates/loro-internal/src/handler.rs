use super::{state::DocState, txn::Transaction};
use crate::{
    container::{
        idx::ContainerIdx,
        list::list_op::{DeleteSpan, ListOp},
        richtext::TextStyleInfoFlag,
    },
    delta::MapValue,
    op::ListSlice,
};
use enum_as_inner::EnumAsInner;
use loro_common::{ContainerID, ContainerType, LoroResult, LoroValue};
use std::{
    borrow::Cow,
    sync::{Mutex, Weak},
};

#[derive(Clone)]
pub struct TextHandler {
    container_idx: ContainerIdx,
    state: Weak<Mutex<DocState>>,
}

impl std::fmt::Debug for TextHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RichtextHandler")
    }
}

#[derive(Clone)]
pub struct MapHandler {
    container_idx: ContainerIdx,
    state: Weak<Mutex<DocState>>,
}

impl std::fmt::Debug for MapHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("MapHandler")
    }
}

#[derive(Clone)]
pub struct ListHandler {
    container_idx: ContainerIdx,
    state: Weak<Mutex<DocState>>,
}

impl std::fmt::Debug for ListHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ListHandler")
    }
}

#[derive(Clone, EnumAsInner, Debug)]
pub enum Handler {
    Text(TextHandler),
    Map(MapHandler),
    List(ListHandler),
}

impl Handler {
    pub fn container_idx(&self) -> ContainerIdx {
        match self {
            Self::Map(x) => x.container_idx,
            Self::List(x) => x.container_idx,
            Self::Text(x) => x.container_idx,
        }
    }

    pub fn c_type(&self) -> ContainerType {
        match self {
            Self::Map(_) => ContainerType::Map,
            Self::List(_) => ContainerType::List,
            Self::Text(_) => ContainerType::Text,
        }
    }
}

impl Handler {
    fn new(value: ContainerIdx, state: Weak<Mutex<DocState>>) -> Self {
        match value.get_type() {
            ContainerType::Map => Self::Map(MapHandler::new(value, state)),
            ContainerType::List => Self::List(ListHandler::new(value, state)),
            ContainerType::Text => Self::Text(TextHandler::new(value, state)),
        }
    }
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

    pub fn get_richtext_value(&self) -> LoroValue {
        self.state
            .upgrade()
            .unwrap()
            .lock()
            .unwrap()
            .with_state(self.container_idx, |state| {
                state.as_richtext_state().unwrap().get_richtext_value()
            })
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
        self.len_unicode() == 0
    }

    pub fn len_utf8(&self) -> usize {
        self.state
            .upgrade()
            .unwrap()
            .lock()
            .unwrap()
            .with_state(self.container_idx, |state| {
                state.as_richtext_state().unwrap().len_utf8()
            })
    }

    pub fn len_utf16(&self) -> usize {
        self.state
            .upgrade()
            .unwrap()
            .lock()
            .unwrap()
            .with_state(self.container_idx, |state| {
                state.as_richtext_state().unwrap().len_utf16()
            })
    }

    pub fn len_unicode(&self) -> usize {
        self.state
            .upgrade()
            .unwrap()
            .lock()
            .unwrap()
            .with_state(self.container_idx, |state| {
                state.as_richtext_state().unwrap().len_unicode()
            })
    }

    pub fn insert_utf16(&self, txn: &mut Transaction, pos: usize, s: &str) -> LoroResult<()> {
        let entity_index =
            self.state
                .upgrade()
                .unwrap()
                .lock()
                .unwrap()
                .with_state(self.container_idx, |state| {
                    state
                        .as_richtext_state()
                        .unwrap()
                        .get_entity_index_for_text_insert_pos(pos)
                });

        txn.apply_local_op(
            self.container_idx,
            crate::op::RawOpContent::List(crate::container::list::list_op::ListOp::Insert {
                slice: ListSlice::RawStr {
                    str: Cow::Borrowed(s),
                    unicode_len: s.chars().count(),
                },
                pos: entity_index,
            }),
            None,
            &self.state,
        )
    }

    pub fn insert(&self, txn: &mut Transaction, pos: usize, s: &str) -> LoroResult<()> {
        let entity_index =
            self.state
                .upgrade()
                .unwrap()
                .lock()
                .unwrap()
                .with_state(self.container_idx, |state| {
                    state
                        .as_richtext_state()
                        .unwrap()
                        .get_entity_index_for_text_insert_pos(pos)
                });

        txn.apply_local_op(
            self.container_idx,
            crate::op::RawOpContent::List(crate::container::list::list_op::ListOp::Insert {
                slice: ListSlice::RawStr {
                    str: Cow::Borrowed(s),
                    unicode_len: s.chars().count(),
                },
                pos: entity_index,
            }),
            None,
            &self.state,
        )
    }

    pub fn delete(&self, txn: &mut Transaction, pos: usize, len: usize) -> LoroResult<()> {
        let ranges =
            self.state
                .upgrade()
                .unwrap()
                .lock()
                .unwrap()
                .with_state(self.container_idx, |state| {
                    state
                        .as_richtext_state()
                        .unwrap()
                        .get_text_entity_ranges_in_unicode_range(pos, len)
                });

        debug_log::debug_dbg!(&ranges, pos, len);
        for range in ranges.iter().rev() {
            txn.apply_local_op(
                self.container_idx,
                crate::op::RawOpContent::List(ListOp::Delete(DeleteSpan {
                    pos: range.start as isize,
                    signed_len: (range.end - range.start) as isize,
                })),
                None,
                &self.state,
            )?;
        }

        Ok(())
    }

    pub fn delete_utf16(&self, txn: &mut Transaction, pos: usize, len: usize) -> LoroResult<()> {
        let ranges =
            self.state
                .upgrade()
                .unwrap()
                .lock()
                .unwrap()
                .with_state(self.container_idx, |state| {
                    state
                        .as_richtext_state()
                        .unwrap()
                        .get_text_entity_ranges_in_utf16_range(pos, len)
                });

        debug_log::debug_dbg!(&ranges, pos, len);
        for range in ranges.iter().rev() {
            txn.apply_local_op(
                self.container_idx,
                crate::op::RawOpContent::List(ListOp::Delete(DeleteSpan {
                    pos: range.start as isize,
                    signed_len: (range.end - range.start) as isize,
                })),
                None,
                &self.state,
            )?;
        }

        Ok(())
    }

    pub fn mark(
        &self,
        txn: &mut Transaction,
        start: usize,
        end: usize,
        key: &str,
        flag: TextStyleInfoFlag,
    ) -> LoroResult<()> {
        if start >= end {
            return Err(loro_common::LoroError::ArgErr(
                "Start must be less than end".to_string().into_boxed_str(),
            ));
        }

        let (entity_start, entity_end) =
            self.state
                .upgrade()
                .unwrap()
                .lock()
                .unwrap()
                .with_state(self.container_idx, |state| {
                    (
                        state
                            .as_richtext_state()
                            .unwrap()
                            .get_entity_index_for_text_insert_pos(start),
                        state
                            .as_richtext_state()
                            .unwrap()
                            .get_entity_index_for_text_insert_pos(end),
                    )
                });

        txn.apply_local_op(
            self.container_idx,
            crate::op::RawOpContent::List(ListOp::StyleStart {
                start: entity_start as u32,
                end: entity_end as u32,
                key: key.into(),
                info: flag,
            }),
            None,
            &self.state,
        )?;

        txn.apply_local_op(
            self.container_idx,
            crate::op::RawOpContent::List(ListOp::StyleEnd),
            None,
            &self.state,
        )?;

        Ok(())
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

    pub fn insert(&self, txn: &mut Transaction, pos: usize, v: LoroValue) -> LoroResult<()> {
        if let Some(container) = v.as_container() {
            self.insert_container(txn, pos, container.container_type())?;
            return Ok(());
        }

        txn.apply_local_op(
            self.container_idx,
            crate::op::RawOpContent::List(crate::container::list::list_op::ListOp::Insert {
                slice: ListSlice::RawData(Cow::Owned(vec![v])),
                pos,
            }),
            None,
            &self.state,
        )
    }

    pub fn push(&self, txn: &mut Transaction, v: LoroValue) -> LoroResult<()> {
        let pos = self.len();
        self.insert(txn, pos, v)
    }

    pub fn insert_container(
        &self,
        txn: &mut Transaction,
        pos: usize,
        c_type: ContainerType,
    ) -> LoroResult<Handler> {
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
            &self.state,
        )?;
        Ok(Handler::new(child_idx, self.state.clone()))
    }

    pub fn delete(&self, txn: &mut Transaction, pos: usize, len: usize) -> LoroResult<()> {
        if len == 0 {
            return Ok(());
        }

        txn.apply_local_op(
            self.container_idx,
            crate::op::RawOpContent::List(ListOp::Delete(DeleteSpan {
                pos: pos as isize,
                signed_len: len as isize,
            })),
            None,
            &self.state,
        )
    }

    pub fn get_child_handler(&self, index: usize) -> Handler {
        let mutex = &self.state.upgrade().unwrap();
        let state = mutex.lock().unwrap();
        let container_id = state.with_state(self.container_idx, |state| {
            state
                .as_list_state()
                .as_ref()
                .unwrap()
                .get(index)
                .unwrap()
                .as_container()
                .unwrap()
                .clone()
        });
        let idx = state.arena.register_container(&container_id);
        Handler::new(idx, self.state.clone())
    }

    pub fn len(&self) -> usize {
        self.state
            .upgrade()
            .unwrap()
            .lock()
            .unwrap()
            .with_state(self.container_idx, |state| {
                state.as_list_state().as_ref().unwrap().len()
            })
    }

    pub fn is_empty(&self) -> bool {
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

    pub fn get_deep_value(&self) -> LoroValue {
        self.state
            .upgrade()
            .unwrap()
            .lock()
            .unwrap()
            .get_container_deep_value(self.container_idx)
    }

    pub fn get_deep_value_with_id(&self) -> LoroValue {
        self.state
            .upgrade()
            .unwrap()
            .lock()
            .unwrap()
            .get_container_deep_value_with_id(self.container_idx, None)
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

    pub fn get(&self, index: usize) -> Option<LoroValue> {
        self.state
            .upgrade()
            .unwrap()
            .lock()
            .unwrap()
            .with_state(self.container_idx, |state| {
                let a = state.as_list_state().unwrap();
                a.get(index).cloned()
            })
    }

    pub fn for_each<I>(&self, mut f: I)
    where
        I: FnMut(&LoroValue),
    {
        self.state
            .upgrade()
            .unwrap()
            .lock()
            .unwrap()
            .with_state(self.container_idx, |state| {
                let a = state.as_list_state().unwrap();
                for v in a.iter() {
                    f(v);
                }
            })
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

    pub fn insert(&self, txn: &mut Transaction, key: &str, value: LoroValue) -> LoroResult<()> {
        if let Some(value) = value.as_container() {
            self.insert_container(txn, key, value.container_type())?;
            return Ok(());
        }

        if self.get(key).map(|x| x == value).unwrap_or(false) {
            // skip if the value is already set
            return Ok(());
        }

        txn.apply_local_op(
            self.container_idx,
            crate::op::RawOpContent::Map(crate::container::map::MapSet {
                key: key.into(),
                value: Some(value),
            }),
            None,
            &self.state,
        )
    }

    pub fn insert_container(
        &self,
        txn: &mut Transaction,
        key: &str,
        c_type: ContainerType,
    ) -> LoroResult<Handler> {
        let id = txn.next_id();
        let container_id = ContainerID::new_normal(id, c_type);
        let child_idx = txn.arena.register_container(&container_id);
        txn.arena.set_parent(child_idx, Some(self.container_idx));
        txn.apply_local_op(
            self.container_idx,
            crate::op::RawOpContent::Map(crate::container::map::MapSet {
                key: key.into(),
                value: Some(LoroValue::Container(container_id)),
            }),
            None,
            &self.state,
        )?;

        Ok(Handler::new(child_idx, self.state.clone()))
    }

    pub fn delete(&self, txn: &mut Transaction, key: &str) -> LoroResult<()> {
        txn.apply_local_op(
            self.container_idx,
            crate::op::RawOpContent::Map(crate::container::map::MapSet {
                key: key.into(),
                value: None,
            }),
            None,
            &self.state,
        )
    }

    pub fn for_each<I>(&self, mut f: I)
    where
        I: FnMut(&str, &MapValue),
    {
        self.state
            .upgrade()
            .unwrap()
            .lock()
            .unwrap()
            .with_state(self.container_idx, |state| {
                let a = state.as_map_state().unwrap();
                for (k, v) in a.iter() {
                    f(k, v);
                }
            })
    }

    pub fn get_value(&self) -> LoroValue {
        self.state
            .upgrade()
            .unwrap()
            .lock()
            .unwrap()
            .get_value_by_idx(self.container_idx)
    }

    pub fn get_child_handler(&self, key: &str) -> Handler {
        let mutex = &self.state.upgrade().unwrap();
        let state = mutex.lock().unwrap();
        let container_id = state.with_state(self.container_idx, |state| {
            state
                .as_map_state()
                .as_ref()
                .unwrap()
                .get(key)
                .unwrap()
                .as_container()
                .unwrap()
                .clone()
        });
        let idx = state.arena.register_container(&container_id);
        Handler::new(idx, self.state.clone())
    }

    pub fn get_deep_value(&self) -> LoroValue {
        self.state
            .upgrade()
            .unwrap()
            .lock()
            .unwrap()
            .get_container_deep_value(self.container_idx)
    }

    pub fn get_deep_value_with_id(&self) -> LoroValue {
        self.state
            .upgrade()
            .unwrap()
            .lock()
            .unwrap()
            .get_container_deep_value_with_id(self.container_idx, None)
    }

    pub fn get(&self, key: &str) -> Option<LoroValue> {
        self.state
            .upgrade()
            .unwrap()
            .lock()
            .unwrap()
            .with_state(self.container_idx, |state| {
                let a = state.as_map_state().unwrap();
                a.get(key).cloned()
            })
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

    pub fn len(&self) -> usize {
        self.state
            .upgrade()
            .unwrap()
            .lock()
            .unwrap()
            .with_state(self.container_idx, |state| {
                state.as_map_state().as_ref().unwrap().len()
            })
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod test {
    use std::ops::Deref;

    use crate::container::richtext::TextStyleInfoFlag;
    use crate::loro::LoroDoc;
    use crate::version::Frontiers;
    use crate::ToJson;
    use loro_common::ID;

    #[test]
    fn test() {
        let loro = LoroDoc::new();
        let mut txn = loro.txn().unwrap();
        let text = txn.get_text("hello");
        text.insert(&mut txn, 0, "hello").unwrap();
        assert_eq!(&**text.get_value().as_string().unwrap(), "hello");
        text.insert(&mut txn, 2, " kk ").unwrap();
        assert_eq!(&**text.get_value().as_string().unwrap(), "he kk llo");
        txn.abort();
        let mut txn = loro.txn().unwrap();
        assert_eq!(&**text.get_value().as_string().unwrap(), "");
        text.insert(&mut txn, 0, "hi").unwrap();
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
        text.insert(&mut txn, 0, "hello").unwrap();
        txn.commit().unwrap();
        let exported = loro.export_from(&Default::default());
        loro2.import(&exported).unwrap();
        let mut txn = loro2.txn().unwrap();
        let text = txn.get_text("hello");
        assert_eq!(&**text.get_value().as_string().unwrap(), "hello");
        text.insert(&mut txn, 5, " world").unwrap();
        assert_eq!(&**text.get_value().as_string().unwrap(), "hello world");
        txn.commit().unwrap();
        loro.import(&loro2.export_from(&Default::default()))
            .unwrap();
        let txn = loro.txn().unwrap();
        let text = txn.get_text("hello");
        assert_eq!(&**text.get_value().as_string().unwrap(), "hello world");
    }

    #[test]
    fn richtext_handler() {
        let mut loro = LoroDoc::new();
        loro.set_peer_id(1);
        let loro2 = LoroDoc::new();
        loro2.set_peer_id(2);

        let mut txn = loro.txn().unwrap();
        let text = txn.get_text("hello");
        text.insert(&mut txn, 0, "hello").unwrap();
        txn.commit().unwrap();
        let exported = loro.export_from(&Default::default());

        loro2.import(&exported).unwrap();
        let mut txn = loro2.txn().unwrap();
        let text = txn.get_text("hello");
        assert_eq!(&**text.get_value().as_string().unwrap(), "hello");
        text.insert(&mut txn, 5, " world").unwrap();
        assert_eq!(&**text.get_value().as_string().unwrap(), "hello world");
        txn.commit().unwrap();

        loro.import(&loro2.export_from(&Default::default()))
            .unwrap();
        let txn = loro.txn().unwrap();
        let text = txn.get_text("hello");
        assert_eq!(&**text.get_value().as_string().unwrap(), "hello world");
        txn.commit().unwrap();

        // test checkout
        loro.checkout(&Frontiers::from_id(ID::new(2, 1))).unwrap();
        assert_eq!(&**text.get_value().as_string().unwrap(), "hello w");
    }

    #[test]
    fn richtext_handler_concurrent() {
        let loro = LoroDoc::new();
        let mut txn = loro.txn().unwrap();
        let handler = loro.get_text("richtext");
        handler.insert(&mut txn, 0, "hello").unwrap();
        txn.commit().unwrap();
        for i in 0..100 {
            let new_loro = LoroDoc::new();
            new_loro
                .import(&loro.export_from(&Default::default()))
                .unwrap();
            let mut txn = new_loro.txn().unwrap();
            let handler = new_loro.get_text("richtext");
            handler.insert(&mut txn, i % 5, &i.to_string()).unwrap();
            txn.commit().unwrap();
            loro.import(&new_loro.export_from(&loro.oplog_vv()))
                .unwrap();
        }
    }

    #[test]
    fn richtext_handler_mark() {
        let loro = LoroDoc::new();
        let mut txn = loro.txn().unwrap();
        let handler = loro.get_text("richtext");
        handler.insert(&mut txn, 0, "hello world").unwrap();
        handler
            .mark(&mut txn, 0, 5, "bold".into(), TextStyleInfoFlag::BOLD)
            .unwrap();
        txn.commit().unwrap();

        // assert has bold
        let value = handler.get_richtext_value();
        assert_eq!(value[0]["insert"], "hello".into());
        let meta = value[0]["attributes"].as_map().unwrap();
        assert_eq!(meta.len(), 1);
        meta.get("bold").unwrap();

        let loro2 = LoroDoc::new();
        loro2
            .import(&loro.export_from(&Default::default()))
            .unwrap();
        let handler2 = loro2.get_text("richtext");
        assert_eq!(
            handler2.get_value().as_string().unwrap().deref(),
            "hello world"
        );

        // assert has bold
        let value = handler2.get_richtext_value();
        dbg!(&value);
        assert_eq!(value[0]["insert"], "hello".into());
        let meta = value[0]["attributes"].as_map().unwrap();
        assert_eq!(meta.len(), 1);
        meta.get("bold").unwrap();

        // insert after bold should be bold
        {
            loro2
                .with_txn(|txn| handler2.insert(txn, 5, " new"))
                .unwrap();

            let value = handler2.get_richtext_value();
            assert_eq!(
                value.to_json_value(),
                serde_json::json!([
                    {"insert": "hello new", "attributes": {"bold": true}},
                    {"insert": " world"}
                ])
            );
        }
    }
}

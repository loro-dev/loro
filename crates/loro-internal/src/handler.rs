use super::{state::DocState, txn::Transaction};
use crate::{
    arena::SharedArena,
    container::{
        idx::ContainerIdx,
        list::list_op::{DeleteSpan, DeleteSpanWithId, ListOp},
        richtext::richtext_state::PosType,
        tree::tree_op::TreeOp,
    },
    delta::{DeltaItem, StyleMeta, TreeDiffItem, TreeExternalDiff},
    op::ListSlice,
    state::{State, TreeParentId},
    txn::EventHint,
    utils::{string_slice::StringSlice, utf16::count_utf16_len},
};
use enum_as_inner::EnumAsInner;
use fxhash::FxHashMap;
use loro_common::{
    ContainerID, ContainerType, InternalString, LoroError, LoroResult, LoroTreeError, LoroValue,
    TreeID,
};
use serde::{Deserialize, Serialize};
use std::{
    borrow::Cow,
    ops::Deref,
    sync::{Mutex, Weak},
};

#[derive(Debug, Clone, EnumAsInner, Deserialize, Serialize, PartialEq)]
#[serde(untagged)]
pub enum TextDelta {
    Retain {
        retain: usize,
        attributes: Option<FxHashMap<String, LoroValue>>,
    },
    Insert {
        insert: String,
        attributes: Option<FxHashMap<String, LoroValue>>,
    },
    Delete {
        delete: usize,
    },
}

impl From<&DeltaItem<StringSlice, StyleMeta>> for TextDelta {
    fn from(value: &DeltaItem<StringSlice, StyleMeta>) -> Self {
        match value {
            crate::delta::DeltaItem::Retain { retain, attributes } => TextDelta::Retain {
                retain: *retain,
                attributes: attributes.to_option_map(),
            },
            crate::delta::DeltaItem::Insert { insert, attributes } => TextDelta::Insert {
                insert: insert.to_string(),
                attributes: attributes.to_option_map(),
            },
            crate::delta::DeltaItem::Delete {
                delete,
                attributes: _,
            } => TextDelta::Delete { delete: *delete },
        }
    }
}

pub trait HandlerTrait {
    fn inner(&self) -> &BasicHandler;

    fn get_value(&self) -> LoroValue {
        self.inner()
            .state
            .upgrade()
            .unwrap()
            .lock()
            .unwrap()
            .get_value_by_idx(self.inner().container_idx)
    }

    fn get_deep_value(&self) -> LoroValue {
        self.inner()
            .state
            .upgrade()
            .unwrap()
            .lock()
            .unwrap()
            .get_container_deep_value(self.inner().container_idx)
    }

    fn idx(&self) -> ContainerIdx {
        self.inner().container_idx
    }

    fn id(&self) -> &ContainerID {
        &self.inner().id
    }

    fn with_state<R>(&self, f: impl FnOnce(&mut State) -> R) -> R {
        let state = self.inner().state.upgrade().unwrap();
        let mut guard = state.lock().unwrap();
        guard.with_state_mut(self.idx(), f)
    }

    fn with_txn<R>(&self, f: impl FnOnce(&mut Transaction) -> LoroResult<R>) -> LoroResult<R> {
        with_txn(&self.inner().txn, f)
    }

    fn parent(&self) -> Option<Handler> {
        self.inner().get_parent()
    }
}

fn create_handler(handler: &impl HandlerTrait, id: ContainerID) -> Handler {
    Handler::new(
        id,
        handler.inner().arena.clone(),
        handler.inner().txn.clone(),
        handler.inner().state.clone(),
    )
}

/// Flatten attributes that allow overlap
#[derive(Clone)]
pub struct BasicHandler {
    id: ContainerID,
    arena: SharedArena,
    container_idx: ContainerIdx,
    txn: Weak<Mutex<Option<Transaction>>>,
    state: Weak<Mutex<DocState>>,
}

impl BasicHandler {
    #[inline]
    fn with_doc_state<R>(&self, f: impl FnOnce(&mut DocState) -> R) -> R {
        let state = self.state.upgrade().unwrap();
        let mut guard = state.lock().unwrap();
        f(&mut guard)
    }

    fn with_txn(
        &self,
        f: impl FnOnce(&mut Transaction) -> Result<(), LoroError>,
    ) -> Result<(), LoroError> {
        with_txn(&self.txn, f)
    }

    fn get_parent(&self) -> Option<Handler> {
        let parent_idx = self.arena.get_parent(self.container_idx)?;
        let parent_id = self.arena.get_container_id(parent_idx).unwrap();
        {
            let arena = self.arena.clone();
            let txn = self.txn.clone();
            let state = self.state.clone();
            let kind = parent_id.container_type();
            let handler = BasicHandler {
                container_idx: parent_idx,
                id: parent_id,
                txn,
                arena,
                state,
            };

            Some(match kind {
                ContainerType::Map => Handler::Map(MapHandler { inner: handler }),
                ContainerType::List => Handler::List(ListHandler { inner: handler }),
                ContainerType::Tree => Handler::Tree(TreeHandler { inner: handler }),
                ContainerType::Text => Handler::Text(TextHandler { inner: handler }),
            })
        }
    }
}

/// Flatten attributes that allow overlap
#[derive(Clone)]
pub struct TextHandler {
    inner: BasicHandler,
}

impl HandlerTrait for TextHandler {
    fn inner(&self) -> &BasicHandler {
        &self.inner
    }
}

impl std::fmt::Debug for TextHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "TextHandler {}", self.inner.id)
    }
}

#[derive(Clone)]
pub struct MapHandler {
    inner: BasicHandler,
}

impl HandlerTrait for MapHandler {
    fn inner(&self) -> &BasicHandler {
        &self.inner
    }
}

impl std::fmt::Debug for MapHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "MapHandler {}", self.inner.id)
    }
}

#[derive(Clone)]
pub struct ListHandler {
    inner: BasicHandler,
}

impl std::fmt::Debug for ListHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ListHandler {}", self.inner.id)
    }
}

impl HandlerTrait for ListHandler {
    fn inner(&self) -> &BasicHandler {
        &self.inner
    }
}

///
#[derive(Clone)]
pub struct TreeHandler {
    inner: BasicHandler,
}

impl HandlerTrait for TreeHandler {
    fn inner(&self) -> &BasicHandler {
        &self.inner
    }
}

impl std::fmt::Debug for TreeHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "TreeHandler {}", self.inner.id)
    }
}

#[derive(Clone, EnumAsInner, Debug)]
pub enum Handler {
    Text(TextHandler),
    Map(MapHandler),
    List(ListHandler),
    Tree(TreeHandler),
}

impl Handler {
    pub(crate) fn new(
        id: ContainerID,
        arena: SharedArena,
        txn: Weak<Mutex<Option<Transaction>>>,
        state: Weak<Mutex<DocState>>,
    ) -> Self {
        let kind = id.container_type();
        let handler = BasicHandler {
            container_idx: arena.register_container(&id),
            id,
            txn,
            arena,
            state,
        };

        match kind {
            ContainerType::Map => Self::Map(MapHandler { inner: handler }),
            ContainerType::List => Self::List(ListHandler { inner: handler }),
            ContainerType::Tree => Self::Tree(TreeHandler { inner: handler }),
            ContainerType::Text => Self::Text(TextHandler { inner: handler }),
        }
    }

    pub fn id(&self) -> &ContainerID {
        match self {
            Self::Map(x) => &x.inner.id,
            Self::List(x) => &x.inner.id,
            Self::Text(x) => &x.inner.id,
            Self::Tree(x) => &x.inner.id,
        }
    }

    pub fn container_idx(&self) -> ContainerIdx {
        match self {
            Self::Map(x) => x.inner.container_idx,
            Self::List(x) => x.inner.container_idx,
            Self::Text(x) => x.inner.container_idx,
            Self::Tree(x) => x.inner.container_idx,
        }
    }

    pub fn c_type(&self) -> ContainerType {
        match self {
            Self::Map(_) => ContainerType::Map,
            Self::List(_) => ContainerType::List,
            Self::Text(_) => ContainerType::Text,
            Self::Tree(_) => ContainerType::Tree,
        }
    }
}

#[derive(Clone, EnumAsInner, Debug)]
pub enum ValueOrHandler {
    Value(LoroValue),
    Handler(Handler),
}

impl ValueOrHandler {
    pub(crate) fn from_value(
        value: LoroValue,
        arena: &SharedArena,
        txn: &Weak<Mutex<Option<Transaction>>>,
        state: &Weak<Mutex<DocState>>,
    ) -> Self {
        if let LoroValue::Container(c) = value {
            ValueOrHandler::Handler(Handler::new(c, arena.clone(), txn.clone(), state.clone()))
        } else {
            ValueOrHandler::Value(value)
        }
    }
}

impl TextHandler {
    pub fn get_richtext_value(&self) -> LoroValue {
        self.with_state(|state| state.as_richtext_state_mut().unwrap().get_richtext_value())
    }

    pub fn is_empty(&self) -> bool {
        self.len_unicode() == 0
    }

    pub fn len_utf8(&self) -> usize {
        self.with_state(|state| state.as_richtext_state_mut().unwrap().len_utf8())
    }

    pub fn len_utf16(&self) -> usize {
        self.with_state(|state| state.as_richtext_state_mut().unwrap().len_utf16())
    }

    pub fn len_unicode(&self) -> usize {
        self.with_state(|state| state.as_richtext_state_mut().unwrap().len_unicode())
    }

    /// if `wasm` feature is enabled, it is a UTF-16 length
    /// otherwise, it is a Unicode length
    pub fn len_event(&self) -> usize {
        self.with_state(|state| {
            if cfg!(feature = "wasm") {
                state.as_richtext_state_mut().unwrap().len_utf16()
            } else {
                state.as_richtext_state_mut().unwrap().len_unicode()
            }
        })
    }

    pub fn diagnose(&self) {
        todo!();

        // self.inner.diagnose();
    }

    /// `pos` is a Event Index:
    ///
    /// - if feature="wasm", pos is a UTF-16 index
    /// - if feature!="wasm", pos is a Unicode index
    ///
    /// This method requires auto_commit to be enabled.
    pub fn insert(&self, pos: usize, s: &str) -> LoroResult<()> {
        self.inner.with_txn(|txn| self.insert_with_txn(txn, pos, s))
    }

    /// `pos` is a Event Index:
    ///
    /// - if feature="wasm", pos is a UTF-16 index
    /// - if feature!="wasm", pos is a Unicode index
    pub fn insert_with_txn(&self, txn: &mut Transaction, pos: usize, s: &str) -> LoroResult<()> {
        self.insert_with_txn_and_attr(txn, pos, s, None)?;
        Ok(())
    }

    /// If attr is specified, it will be used as the attribute of the inserted text.
    /// It will override the existing attribute of the text.
    fn insert_with_txn_and_attr(
        &self,
        txn: &mut Transaction,
        pos: usize,
        s: &str,
        attr: Option<&FxHashMap<String, LoroValue>>,
    ) -> Result<Vec<(InternalString, LoroValue)>, LoroError> {
        if s.is_empty() {
            return Ok(Vec::new());
        }

        if pos > self.len_event() {
            return Err(LoroError::OutOfBound {
                pos,
                len: self.len_event(),
            });
        }

        let (entity_index, styles) = self.with_state(|state| {
            let richtext_state = state.as_richtext_state_mut().unwrap();
            let pos = richtext_state.get_entity_index_for_text_insert(pos);
            let styles = richtext_state.get_styles_at_entity_index(pos);
            (pos, styles)
        });

        let mut override_styles = Vec::new();
        if let Some(attr) = attr {
            // current styles
            let map: FxHashMap<_, _> = styles.iter().map(|x| (x.0.clone(), x.1.data)).collect();
            for (key, style) in map.iter() {
                match attr.get(key.deref()) {
                    Some(v) if v == style => {}
                    new_style_value => {
                        // need to override
                        let new_style_value = new_style_value.cloned().unwrap_or(LoroValue::Null);
                        override_styles.push((key.clone(), new_style_value));
                    }
                }
            }

            for (key, style) in attr.iter() {
                let key = key.as_str().into();
                if !map.contains_key(&key) {
                    override_styles.push((key, style.clone()));
                }
            }
        }

        let unicode_len = s.chars().count();
        let event_len = if cfg!(feature = "wasm") {
            count_utf16_len(s.as_bytes())
        } else {
            unicode_len
        };

        txn.apply_local_op(
            self.inner.container_idx,
            crate::op::RawOpContent::List(crate::container::list::list_op::ListOp::Insert {
                slice: ListSlice::RawStr {
                    str: Cow::Borrowed(s),
                    unicode_len,
                },
                pos: entity_index,
            }),
            EventHint::InsertText {
                pos: pos as u32,
                styles,
                unicode_len: unicode_len as u32,
                event_len: event_len as u32,
            },
            &self.inner.state,
        )?;

        Ok(override_styles)
    }

    /// `pos` is a Event Index:
    ///
    /// - if feature="wasm", pos is a UTF-16 index
    /// - if feature!="wasm", pos is a Unicode index
    ///
    /// This method requires auto_commit to be enabled.
    pub fn delete(&self, pos: usize, len: usize) -> LoroResult<()> {
        self.inner
            .with_txn(|txn| self.delete_with_txn(txn, pos, len))
    }

    /// `pos` is a Event Index:
    ///
    /// - if feature="wasm", pos is a UTF-16 index
    /// - if feature!="wasm", pos is a Unicode index
    pub fn delete_with_txn(&self, txn: &mut Transaction, pos: usize, len: usize) -> LoroResult<()> {
        if len == 0 {
            return Ok(());
        }

        if pos + len > self.len_event() {
            return Err(LoroError::OutOfBound {
                pos: pos + len,
                len: self.len_event(),
            });
        }

        let s = tracing::span!(tracing::Level::INFO, "delete pos={} len={}", pos, len);
        let _e = s.enter();
        let ranges = self.with_state(|state| {
            let richtext_state = state.as_richtext_state_mut().unwrap();
            richtext_state.get_text_entity_ranges_in_event_index_range(pos, len)
        });

        debug_assert_eq!(ranges.iter().map(|x| x.event_len).sum::<usize>(), len);
        let mut event_end = (pos + len) as isize;
        for range in ranges.iter().rev() {
            let event_start = event_end - range.event_len as isize;
            txn.apply_local_op(
                self.inner.container_idx,
                crate::op::RawOpContent::List(ListOp::Delete(DeleteSpanWithId::new(
                    range.id_start,
                    range.entity_start as isize,
                    range.entity_len() as isize,
                ))),
                EventHint::DeleteText {
                    span: DeleteSpan {
                        pos: event_start,
                        signed_len: range.event_len as isize,
                    },
                    unicode_len: range.entity_len(),
                },
                &self.inner.state,
            )?;
            event_end = event_start;
        }

        Ok(())
    }

    /// `start` and `end` are [Event Index]s:
    ///
    /// - if feature="wasm", pos is a UTF-16 index
    /// - if feature!="wasm", pos is a Unicode index
    ///
    /// This method requires auto_commit to be enabled.
    pub fn mark(
        &self,
        start: usize,
        end: usize,
        key: impl Into<InternalString>,
        value: LoroValue,
    ) -> LoroResult<()> {
        self.inner
            .with_txn(|txn| self.mark_with_txn(txn, start, end, key, value, false))
    }

    /// `start` and `end` are [Event Index]s:
    ///
    /// - if feature="wasm", pos is a UTF-16 index
    /// - if feature!="wasm", pos is a Unicode index
    ///
    /// This method requires auto_commit to be enabled.
    pub fn unmark(
        &self,
        start: usize,
        end: usize,
        key: impl Into<InternalString>,
    ) -> LoroResult<()> {
        self.inner
            .with_txn(|txn| self.mark_with_txn(txn, start, end, key, LoroValue::Null, true))
    }

    /// `start` and `end` are [Event Index]s:
    ///
    /// - if feature="wasm", pos is a UTF-16 index
    /// - if feature!="wasm", pos is a Unicode index
    pub fn mark_with_txn(
        &self,
        txn: &mut Transaction,
        start: usize,
        end: usize,
        key: impl Into<InternalString>,
        value: LoroValue,
        is_delete: bool,
    ) -> LoroResult<()> {
        if start >= end {
            return Err(loro_common::LoroError::ArgErr(
                "Start must be less than end".to_string().into_boxed_str(),
            ));
        }

        let len = self.len_event();
        if end > len {
            return Err(LoroError::OutOfBound { pos: end, len });
        }

        let key: InternalString = key.into();

        let mutex = &self.inner.state.upgrade().unwrap();
        let mut doc_state = mutex.lock().unwrap();
        let (entity_range, skip) = doc_state.with_state_mut(self.inner.container_idx, |state| {
            let (entity_range, styles) = state
                .as_richtext_state_mut()
                .unwrap()
                .get_entity_range_and_styles_at_range(start..end, PosType::Event);

            let skip = match styles {
                Some(styles) if styles.has_key_value(&key, &value) => {
                    // already has the same style, skip
                    true
                }
                _ => false,
            };
            (entity_range, skip)
        });

        if skip {
            return Ok(());
        }

        let entity_start = entity_range.start;
        let entity_end = entity_range.end;
        let style_config = doc_state.config.text_style_config.try_read().unwrap();
        let flag = if is_delete {
            style_config
                .get_style_flag_for_unmark(&key)
                .ok_or_else(|| LoroError::StyleConfigMissing(key.clone()))?
        } else {
            style_config
                .get_style_flag(&key)
                .ok_or_else(|| LoroError::StyleConfigMissing(key.clone()))?
        };

        drop(style_config);
        drop(doc_state);
        txn.apply_local_op(
            self.inner.container_idx,
            crate::op::RawOpContent::List(ListOp::StyleStart {
                start: entity_start as u32,
                end: entity_end as u32,
                key: key.clone(),
                value: value.clone(),
                info: flag,
            }),
            EventHint::Mark {
                start: start as u32,
                end: end as u32,
                style: crate::container::richtext::Style { key, data: value },
            },
            &self.inner.state,
        )?;

        txn.apply_local_op(
            self.inner.container_idx,
            crate::op::RawOpContent::List(ListOp::StyleEnd),
            EventHint::MarkEnd,
            &self.inner.state,
        )?;

        Ok(())
    }

    pub fn check(&self) {
        self.with_state(|state| {
            state
                .as_richtext_state_mut()
                .unwrap()
                .check_consistency_between_content_and_style_ranges()
        })
    }

    pub fn apply_delta(&self, delta: &[TextDelta]) -> LoroResult<()> {
        self.inner
            .with_txn(|txn| self.apply_delta_with_txn(txn, delta))
    }

    pub fn apply_delta_with_txn(
        &self,
        txn: &mut Transaction,
        delta: &[TextDelta],
    ) -> LoroResult<()> {
        let mut index = 0;
        let mut marks = Vec::new();
        for d in delta {
            match d {
                TextDelta::Insert { insert, attributes } => {
                    let end = index + event_len(insert.as_str());
                    let override_styles = self.insert_with_txn_and_attr(
                        txn,
                        index,
                        insert.as_str(),
                        Some(attributes.as_ref().unwrap_or(&Default::default())),
                    )?;

                    for (key, value) in override_styles {
                        marks.push((index, end, key, value));
                    }

                    index = end;
                }
                TextDelta::Delete { delete } => {
                    self.delete_with_txn(txn, index, *delete)?;
                }
                TextDelta::Retain { attributes, retain } => {
                    let end = index + *retain;
                    match attributes {
                        Some(attr) if !attr.is_empty() => {
                            for (key, value) in attr {
                                marks.push((index, end, key.deref().into(), value.clone()));
                            }
                        }
                        _ => {}
                    }
                    index = end;
                }
            }
        }

        let mut len = self.len_event();
        for (start, end, key, value) in marks {
            if start >= len {
                self.insert_with_txn(txn, len, &"\n".repeat(start - len + 1))?;
                len = start;
            }

            self.mark_with_txn(txn, start, end, key.deref(), value, false)?;
        }

        Ok(())
    }

    #[allow(clippy::inherent_to_string)]
    pub fn to_string(&self) -> String {
        self.with_state(|s| s.as_richtext_state_mut().unwrap().to_string_mut())
    }
}

fn event_len(s: &str) -> usize {
    if cfg!(feature = "wasm") {
        count_utf16_len(s.as_bytes())
    } else {
        s.chars().count()
    }
}

impl ListHandler {
    pub fn insert(&self, pos: usize, v: impl Into<LoroValue>) -> LoroResult<()> {
        self.inner
            .with_txn(|txn| self.insert_with_txn(txn, pos, v.into()))
    }

    pub fn insert_with_txn(
        &self,
        txn: &mut Transaction,
        pos: usize,
        v: LoroValue,
    ) -> LoroResult<()> {
        if pos > self.len() {
            return Err(LoroError::OutOfBound {
                pos,
                len: self.len(),
            });
        }

        if let Some(container) = v.as_container() {
            self.insert_container_with_txn(txn, pos, container.container_type())?;
            return Ok(());
        }

        txn.apply_local_op(
            self.inner.container_idx,
            crate::op::RawOpContent::List(crate::container::list::list_op::ListOp::Insert {
                slice: ListSlice::RawData(Cow::Owned(vec![v.clone()])),
                pos,
            }),
            EventHint::InsertList { len: 1 },
            &self.inner.state,
        )
    }

    pub fn push(&self, v: LoroValue) -> LoroResult<()> {
        with_txn(&self.inner.txn, |txn| self.push_with_txn(txn, v))
    }

    pub fn push_with_txn(&self, txn: &mut Transaction, v: LoroValue) -> LoroResult<()> {
        let pos = self.len();
        self.insert_with_txn(txn, pos, v)
    }

    pub fn pop(&self) -> LoroResult<Option<LoroValue>> {
        with_txn(&self.inner.txn, |txn| self.pop_with_txn(txn))
    }

    pub fn pop_with_txn(&self, txn: &mut Transaction) -> LoroResult<Option<LoroValue>> {
        let len = self.len();
        if len == 0 {
            return Ok(None);
        }

        let v = self.get(len - 1);
        self.delete_with_txn(txn, len - 1, 1)?;
        Ok(v)
    }

    pub fn insert_container(&self, pos: usize, c_type: ContainerType) -> LoroResult<Handler> {
        with_txn(&self.inner.txn, |txn| {
            self.insert_container_with_txn(txn, pos, c_type)
        })
    }

    pub fn insert_container_with_txn(
        &self,
        txn: &mut Transaction,
        pos: usize,
        c_type: ContainerType,
    ) -> LoroResult<Handler> {
        if pos > self.len() {
            return Err(LoroError::OutOfBound {
                pos,
                len: self.len(),
            });
        }

        let id = txn.next_id();
        let container_id = ContainerID::new_normal(id, c_type);
        let v = LoroValue::Container(container_id.clone());
        txn.apply_local_op(
            self.inner.container_idx,
            crate::op::RawOpContent::List(crate::container::list::list_op::ListOp::Insert {
                slice: ListSlice::RawData(Cow::Owned(vec![v.clone()])),
                pos,
            }),
            EventHint::InsertList { len: 1 },
            &self.inner.state,
        )?;
        Ok(create_handler(self, container_id))
    }

    pub fn delete(&self, pos: usize, len: usize) -> LoroResult<()> {
        with_txn(&self.inner.txn, |txn| self.delete_with_txn(txn, pos, len))
    }

    pub fn delete_with_txn(&self, txn: &mut Transaction, pos: usize, len: usize) -> LoroResult<()> {
        if len == 0 {
            return Ok(());
        }

        if pos + len > self.len() {
            return Err(LoroError::OutOfBound {
                pos: pos + len,
                len: self.len(),
            });
        }

        let ids: Vec<_> = self.with_state(|state| {
            let list = state.as_list_state().unwrap();
            (pos..pos + len)
                .map(|i| list.get_id_at(i).unwrap())
                .collect()
        });

        for id in ids.into_iter() {
            txn.apply_local_op(
                self.inner.container_idx,
                crate::op::RawOpContent::List(ListOp::Delete(DeleteSpanWithId::new(
                    id.id(),
                    pos as isize,
                    1,
                ))),
                EventHint::DeleteList(DeleteSpan::new(pos as isize, 1)),
                &self.inner.state,
            )?;
        }

        Ok(())
    }

    pub fn get_child_handler(&self, index: usize) -> Handler {
        let container_id = self.with_state(|state| {
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
        create_handler(self, container_id)
    }

    pub fn len(&self) -> usize {
        self.with_state(|state| state.as_list_state().as_ref().unwrap().len())
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn get_deep_value_with_id(&self) -> LoroValue {
        self.inner.with_doc_state(|state| {
            state.get_container_deep_value_with_id(self.inner.container_idx, None)
        })
    }

    pub fn get(&self, index: usize) -> Option<LoroValue> {
        self.with_state(|state| {
            let a = state.as_list_state().unwrap();
            a.get(index).cloned()
        })
    }

    /// Get value at given index, if it's a container, return a handler to the container
    pub fn get_(&self, index: usize) -> Option<ValueOrHandler> {
        self.inner.with_doc_state(|doc_state| {
            doc_state.with_state(self.inner.container_idx, |state| {
                let a = state.as_list_state().unwrap();
                match a.get(index) {
                    Some(v) => {
                        if let LoroValue::Container(id) = v {
                            Some(ValueOrHandler::Handler(create_handler(self, id.clone())))
                        } else {
                            Some(ValueOrHandler::Value(v.clone()))
                        }
                    }
                    None => None,
                }
            })
        })
    }

    pub fn for_each<I>(&self, mut f: I)
    where
        I: FnMut(ValueOrHandler),
    {
        self.inner.with_doc_state(|doc_state| {
            doc_state.with_state(self.inner.container_idx, |state| {
                let a = state.as_list_state().unwrap();
                for v in a.iter() {
                    match v {
                        LoroValue::Container(c) => {
                            f(ValueOrHandler::Handler(create_handler(self, c.clone())));
                        }
                        value => {
                            f(ValueOrHandler::Value(value.clone()));
                        }
                    }
                }
            })
        })
    }
}

impl MapHandler {
    pub fn insert(&self, key: &str, value: impl Into<LoroValue>) -> LoroResult<()> {
        with_txn(&self.inner.txn, |txn| {
            self.insert_with_txn(txn, key, value.into())
        })
    }

    pub fn insert_with_txn(
        &self,
        txn: &mut Transaction,
        key: &str,
        value: LoroValue,
    ) -> LoroResult<()> {
        if let Some(value) = value.as_container() {
            self.insert_container_with_txn(txn, key, value.container_type())?;
            return Ok(());
        }

        if self.get(key).map(|x| x == value).unwrap_or(false) {
            // skip if the value is already set
            return Ok(());
        }

        txn.apply_local_op(
            self.inner.container_idx,
            crate::op::RawOpContent::Map(crate::container::map::MapSet {
                key: key.into(),
                value: Some(value.clone()),
            }),
            EventHint::Map {
                key: key.into(),
                value: Some(value.clone()),
            },
            &self.inner.state,
        )
    }

    pub fn insert_container(&self, key: &str, c_type: ContainerType) -> LoroResult<Handler> {
        with_txn(&self.inner.txn, |txn| {
            self.insert_container_with_txn(txn, key, c_type)
        })
    }

    pub fn insert_container_with_txn(
        &self,
        txn: &mut Transaction,
        key: &str,
        c_type: ContainerType,
    ) -> LoroResult<Handler> {
        let id = txn.next_id();
        let container_id = ContainerID::new_normal(id, c_type);
        txn.apply_local_op(
            self.inner.container_idx,
            crate::op::RawOpContent::Map(crate::container::map::MapSet {
                key: key.into(),
                value: Some(LoroValue::Container(container_id.clone())),
            }),
            EventHint::Map {
                key: key.into(),
                value: Some(LoroValue::Container(container_id.clone())),
            },
            &self.inner.state,
        )?;

        Ok(create_handler(self, container_id))
    }

    pub fn delete(&self, key: &str) -> LoroResult<()> {
        with_txn(&self.inner.txn, |txn| self.delete_with_txn(txn, key))
    }

    pub fn delete_with_txn(&self, txn: &mut Transaction, key: &str) -> LoroResult<()> {
        txn.apply_local_op(
            self.inner.container_idx,
            crate::op::RawOpContent::Map(crate::container::map::MapSet {
                key: key.into(),
                value: None,
            }),
            EventHint::Map {
                key: key.into(),
                value: None,
            },
            &self.inner.state,
        )
    }

    pub fn for_each<I>(&self, mut f: I)
    where
        I: FnMut(&str, ValueOrHandler),
    {
        let mutex = &self.inner.state.upgrade().unwrap();
        let mut doc_state = mutex.lock().unwrap();
        doc_state.with_state(self.inner.container_idx, |state| {
            let a = state.as_map_state().unwrap();
            for (k, v) in a.iter() {
                match &v.value {
                    Some(v) => match v {
                        LoroValue::Container(c) => {
                            f(k, ValueOrHandler::Handler(create_handler(self, c.clone())))
                        }
                        value => f(k, ValueOrHandler::Value(value.clone())),
                    },
                    None => {}
                }
            }
        })
    }

    pub fn get_child_handler(&self, key: &str) -> Handler {
        let container_id = self.with_state(|state| {
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
        create_handler(self, container_id)
    }

    pub fn get_deep_value_with_id(&self) -> LoroValue {
        self.inner.with_doc_state(|state| {
            state.get_container_deep_value_with_id(self.inner.container_idx, None)
        })
    }

    pub fn get(&self, key: &str) -> Option<LoroValue> {
        self.with_state(|state| {
            let a = state.as_map_state().unwrap();
            a.get(key).cloned()
        })
    }

    /// Get the value at given key, if value is a container, return a handler to the container
    pub fn get_(&self, key: &str) -> Option<ValueOrHandler> {
        self.with_state(|state| {
            let a = state.as_map_state().unwrap();
            let value = a.get(key);
            match value {
                Some(LoroValue::Container(container_id)) => Some(ValueOrHandler::Handler(
                    create_handler(self, container_id.clone()),
                )),
                Some(value) => Some(ValueOrHandler::Value(value.clone())),
                None => None,
            }
        })
    }

    pub fn len(&self) -> usize {
        self.with_state(|state| state.as_map_state().as_ref().unwrap().len())
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl TreeHandler {
    pub fn delete(&self, target: TreeID) -> LoroResult<()> {
        with_txn(&self.inner.txn, |txn| self.delete_with_txn(txn, target))
    }

    pub fn delete_with_txn(&self, txn: &mut Transaction, target: TreeID) -> LoroResult<()> {
        txn.apply_local_op(
            self.inner.container_idx,
            crate::op::RawOpContent::Tree(TreeOp {
                target,
                parent: Some(TreeID::delete_root()),
            }),
            EventHint::Tree(TreeDiffItem {
                target,
                action: TreeExternalDiff::Delete,
            }),
            &self.inner.state,
        )
    }

    pub fn create<T: Into<Option<TreeID>>>(&self, parent: T) -> LoroResult<TreeID> {
        with_txn(&self.inner.txn, |txn| self.create_with_txn(txn, parent))
    }

    pub fn create_with_txn<T: Into<Option<TreeID>>>(
        &self,
        txn: &mut Transaction,
        parent: T,
    ) -> LoroResult<TreeID> {
        let parent: Option<TreeID> = parent.into();
        let tree_id = TreeID::from_id(txn.next_id());
        let event_hint = TreeDiffItem {
            target: tree_id,
            action: TreeExternalDiff::Create(parent),
        };
        txn.apply_local_op(
            self.inner.container_idx,
            crate::op::RawOpContent::Tree(TreeOp {
                target: tree_id,
                parent,
            }),
            EventHint::Tree(event_hint),
            &self.inner.state,
        )?;
        Ok(tree_id)
    }

    pub fn mov<T: Into<Option<TreeID>>>(&self, target: TreeID, parent: T) -> LoroResult<()> {
        with_txn(&self.inner.txn, |txn| {
            self.mov_with_txn(txn, target, parent)
        })
    }

    pub fn mov_with_txn<T: Into<Option<TreeID>>>(
        &self,
        txn: &mut Transaction,
        target: TreeID,
        parent: T,
    ) -> LoroResult<()> {
        let parent = parent.into();
        txn.apply_local_op(
            self.inner.container_idx,
            crate::op::RawOpContent::Tree(TreeOp { target, parent }),
            EventHint::Tree(TreeDiffItem {
                target,
                action: TreeExternalDiff::Move(parent),
            }),
            &self.inner.state,
        )
    }

    pub fn get_meta(&self, target: TreeID) -> LoroResult<MapHandler> {
        if !self.contains(target) {
            return Err(LoroTreeError::TreeNodeNotExist(target).into());
        }
        let map_container_id = target.associated_meta_container();
        let handler = create_handler(self, map_container_id);
        Ok(handler.into_map().unwrap())
    }

    /// Get the parent of the node, if the node is deleted or does not exist, return None
    pub fn get_node_parent(&self, target: TreeID) -> Option<Option<TreeID>> {
        self.with_state(|state| {
            let a = state.as_tree_state().unwrap();
            a.parent(target).map(|p| match p {
                TreeParentId::None => None,
                TreeParentId::Node(parent_id) => Some(parent_id),
                _ => unreachable!(),
            })
        })
    }

    pub fn children(&self, target: TreeID) -> Vec<TreeID> {
        self.with_state(|state| {
            let a = state.as_tree_state().unwrap();
            a.as_ref()
                .get_children(&TreeParentId::Node(target))
                .into_iter()
                .collect()
        })
    }

    pub fn contains(&self, target: TreeID) -> bool {
        self.with_state(|state| {
            let a = state.as_tree_state().unwrap();
            a.contains(target)
        })
    }

    pub fn nodes(&self) -> Vec<TreeID> {
        self.with_state(|state| {
            let a = state.as_tree_state().unwrap();
            a.nodes()
        })
    }

    #[cfg(feature = "test_utils")]
    pub fn max_counter(&self) -> i32 {
        self.with_state(|state| {
            let a = state.as_tree_state().unwrap();
            a.max_counter()
        })
    }

    #[cfg(feature = "test_utils")]
    pub fn next_tree_id(&self) -> TreeID {
        with_txn(&self.inner.txn, |txn| Ok(TreeID::from_id(txn.next_id()))).unwrap()
    }
}

#[inline(always)]
fn with_txn<R>(
    txn: &Weak<Mutex<Option<Transaction>>>,
    f: impl FnOnce(&mut Transaction) -> LoroResult<R>,
) -> LoroResult<R> {
    let mutex = &txn.upgrade().unwrap();
    let mut txn = mutex.try_lock().unwrap();
    match &mut *txn {
        Some(t) => f(t),
        None => Err(LoroError::AutoCommitNotStarted),
    }
}

#[cfg(test)]
mod test {

    use crate::loro::LoroDoc;
    use crate::version::Frontiers;
    use crate::{fx_map, ToJson};
    use loro_common::ID;
    use serde_json::json;

    use super::{HandlerTrait, TextDelta};

    #[test]
    fn import() {
        let loro = LoroDoc::new();
        loro.set_peer_id(1).unwrap();
        let loro2 = LoroDoc::new();
        loro2.set_peer_id(2).unwrap();

        let mut txn = loro.txn().unwrap();
        let text = txn.get_text("hello");
        text.insert_with_txn(&mut txn, 0, "hello").unwrap();
        txn.commit().unwrap();
        let exported = loro.export_from(&Default::default());
        loro2.import(&exported).unwrap();
        let mut txn = loro2.txn().unwrap();
        let text = txn.get_text("hello");
        assert_eq!(&**text.get_value().as_string().unwrap(), "hello");
        text.insert_with_txn(&mut txn, 5, " world").unwrap();
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
        let loro = LoroDoc::new();
        loro.set_peer_id(1).unwrap();
        let loro2 = LoroDoc::new();
        loro2.set_peer_id(2).unwrap();

        let mut txn = loro.txn().unwrap();
        let text = txn.get_text("hello");
        text.insert_with_txn(&mut txn, 0, "hello").unwrap();
        txn.commit().unwrap();
        let exported = loro.export_from(&Default::default());

        loro2.import(&exported).unwrap();
        let mut txn = loro2.txn().unwrap();
        let text = txn.get_text("hello");
        assert_eq!(&**text.get_value().as_string().unwrap(), "hello");
        text.insert_with_txn(&mut txn, 5, " world").unwrap();
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
        handler.insert_with_txn(&mut txn, 0, "hello").unwrap();
        txn.commit().unwrap();
        for i in 0..100 {
            let new_loro = LoroDoc::new();
            new_loro
                .import(&loro.export_from(&Default::default()))
                .unwrap();
            let mut txn = new_loro.txn().unwrap();
            let handler = new_loro.get_text("richtext");
            handler
                .insert_with_txn(&mut txn, i % 5, &i.to_string())
                .unwrap();
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
        handler.insert_with_txn(&mut txn, 0, "hello world").unwrap();
        handler
            .mark_with_txn(&mut txn, 0, 5, "bold", true.into(), false)
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
        assert_eq!(&**handler2.get_value().as_string().unwrap(), "hello world");

        // assert has bold
        let value = handler2.get_richtext_value();
        assert_eq!(value[0]["insert"], "hello".into());
        let meta = value[0]["attributes"].as_map().unwrap();
        assert_eq!(meta.len(), 1);
        meta.get("bold").unwrap();

        // insert after bold should be bold
        {
            loro2
                .with_txn(|txn| handler2.insert_with_txn(txn, 5, " new"))
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

    #[test]
    fn richtext_snapshot() {
        let loro = LoroDoc::new();
        let mut txn = loro.txn().unwrap();
        let handler = loro.get_text("richtext");
        handler.insert_with_txn(&mut txn, 0, "hello world").unwrap();
        handler
            .mark_with_txn(&mut txn, 0, 5, "bold", true.into(), false)
            .unwrap();
        txn.commit().unwrap();

        let loro2 = LoroDoc::new();
        loro2.import(&loro.export_snapshot()).unwrap();
        let handler2 = loro2.get_text("richtext");
        assert_eq!(
            handler2.get_richtext_value().to_json_value(),
            serde_json::json!([
                {"insert": "hello", "attributes": {"bold": true}},
                {"insert": " world"}
            ])
        );
    }

    #[test]
    fn tree_meta() {
        let loro = LoroDoc::new();
        loro.set_peer_id(1).unwrap();
        let tree = loro.get_tree("root");
        let id = loro
            .with_txn(|txn| tree.create_with_txn(txn, None))
            .unwrap();
        loro.with_txn(|txn| {
            let meta = tree.get_meta(id)?;
            meta.insert_with_txn(txn, "a", 123.into())
        })
        .unwrap();
        let meta = loro
            .with_txn(|_| {
                let meta = tree.get_meta(id)?;
                Ok(meta.get("a").unwrap())
            })
            .unwrap();
        assert_eq!(meta, 123.into());
        assert_eq!(
            r#"[{"parent":null,"meta":{"a":123},"id":"0@1"}]"#,
            tree.get_deep_value().to_json()
        );
        let bytes = loro.export_snapshot();
        let loro2 = LoroDoc::new();
        loro2.import(&bytes).unwrap();
    }

    #[test]
    fn tree_meta_event() {
        use std::sync::Arc;
        let loro = LoroDoc::new();
        let tree = loro.get_tree("root");
        let text = loro.get_text("text");
        loro.with_txn(|txn| {
            let id = tree.create_with_txn(txn, None)?;
            let meta = tree.get_meta(id)?;
            meta.insert_with_txn(txn, "a", 1.into())?;
            text.insert_with_txn(txn, 0, "abc")?;
            let _id2 = tree.create_with_txn(txn, None)?;
            meta.insert_with_txn(txn, "b", 2.into())?;
            Ok(id)
        })
        .unwrap();

        let loro2 = LoroDoc::new();
        loro2.subscribe_root(Arc::new(|e| {
            println!("{} {:?} ", e.event_meta.local, e.event_meta.diff)
        }));
        loro2.import(&loro.export_from(&loro2.oplog_vv())).unwrap();
        assert_eq!(loro.get_deep_value(), loro2.get_deep_value());
    }

    #[test]
    fn richtext_apply_delta() {
        let loro = LoroDoc::new_auto_commit();
        let text = loro.get_text("text");
        text.apply_delta(&[TextDelta::Insert {
            insert: "Hello World!".into(),
            attributes: None,
        }])
        .unwrap();
        dbg!(text.get_richtext_value());
        text.apply_delta(&[
            TextDelta::Retain {
                retain: 6,
                attributes: Some(fx_map!("italic".into() => loro_common::LoroValue::Bool(true))),
            },
            TextDelta::Insert {
                insert: "New ".into(),
                attributes: Some(fx_map!("bold".into() => loro_common::LoroValue::Bool(true))),
            },
        ])
        .unwrap();
        dbg!(text.get_richtext_value());
        loro.commit_then_renew();
        assert_eq!(
            text.get_richtext_value().to_json_value(),
            json!([
                {"insert": "Hello ", "attributes": {"italic": true}},
                {"insert": "New ", "attributes": {"bold": true}},
                {"insert": "World!"}

            ])
        )
    }
}

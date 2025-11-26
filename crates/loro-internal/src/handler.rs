use super::{state::DocState, txn::Transaction};
use crate::sync::Mutex;
use crate::{
    container::{
        idx::ContainerIdx,
        list::list_op::{DeleteSpan, DeleteSpanWithId, ListOp},
        richtext::{richtext_state::PosType, RichtextState, StyleOp, TextStyleInfoFlag},
    },
    cursor::{Cursor, Side},
    delta::{DeltaItem, Meta, StyleMeta, TreeExternalDiff},
    diff::{diff, diff_impl::UpdateTimeoutError, OperateProxy},
    event::{Diff, TextDiff, TextDiffItem, TextMeta},
    op::ListSlice,
    state::{IndexType, State, TreeParentId},
    txn::EventHint,
    utils::{string_slice::StringSlice, utf16::count_utf16_len},
    LoroDoc, LoroDocInner,
};
use append_only_bytes::BytesSlice;
use enum_as_inner::EnumAsInner;
use generic_btree::rle::HasLength;
use loro_common::{
    ContainerID, ContainerType, IdFull, InternalString, LoroError, LoroResult, LoroValue, PeerID,
    TreeID, ID,
};
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};
use std::{borrow::Cow, cmp::Reverse, collections::BinaryHeap, fmt::Debug, ops::Deref, sync::Arc};
use tracing::{error, instrument};

pub use crate::diff::diff_impl::UpdateOptions;
pub use tree::TreeHandler;
mod movable_list_apply_delta;
mod tree;

const INSERT_CONTAINER_VALUE_ARG_ERROR: &str =
    "Cannot insert a LoroValue::Container directly. To create child container, use insert_container";

mod text_update;

pub trait HandlerTrait: Clone + Sized {
    fn is_attached(&self) -> bool;
    fn attached_handler(&self) -> Option<&BasicHandler>;
    fn get_value(&self) -> LoroValue;
    fn get_deep_value(&self) -> LoroValue;
    fn kind(&self) -> ContainerType;
    fn to_handler(&self) -> Handler;
    fn from_handler(h: Handler) -> Option<Self>;
    fn doc(&self) -> Option<LoroDoc>;
    /// This method returns an attached handler.
    fn attach(
        &self,
        txn: &mut Transaction,
        parent: &BasicHandler,
        self_id: ContainerID,
    ) -> LoroResult<Self>;
    /// If a detached container is attached, this method will return its corresponding attached handler.
    fn get_attached(&self) -> Option<Self>;

    fn parent(&self) -> Option<Handler> {
        self.attached_handler().and_then(|x| x.parent())
    }

    fn idx(&self) -> ContainerIdx {
        self.attached_handler()
            .map(|x| x.container_idx)
            .unwrap_or_else(|| ContainerIdx::from_index_and_type(u32::MAX, self.kind()))
    }

    fn id(&self) -> ContainerID {
        self.attached_handler()
            .map(|x| x.id.clone())
            .unwrap_or_else(|| ContainerID::new_normal(ID::NONE_ID, self.kind()))
    }

    fn with_state<R>(&self, f: impl FnOnce(&mut State) -> LoroResult<R>) -> LoroResult<R> {
        let inner = self
            .attached_handler()
            .ok_or(LoroError::MisuseDetachedContainer {
                method: "with_state",
            })?;
        let state = inner.doc.state.clone();
        let mut guard = state.lock().unwrap();
        guard.with_state_mut(inner.container_idx, f)
    }
}

fn create_handler(inner: &BasicHandler, id: ContainerID) -> Handler {
    Handler::new_attached(id, inner.doc.clone())
}

/// Flatten attributes that allow overlap
#[derive(Clone, Debug)]
pub struct BasicHandler {
    id: ContainerID,
    container_idx: ContainerIdx,
    doc: LoroDoc,
}

struct DetachedInner<T> {
    value: T,
    /// If the handler attached later, this field will be filled.
    attached: Option<BasicHandler>,
}

impl<T> DetachedInner<T> {
    fn new(v: T) -> Self {
        Self {
            value: v,
            attached: None,
        }
    }
}

enum MaybeDetached<T> {
    Detached(Arc<Mutex<DetachedInner<T>>>),
    Attached(BasicHandler),
}

impl<T> Clone for MaybeDetached<T> {
    fn clone(&self) -> Self {
        match self {
            MaybeDetached::Detached(a) => MaybeDetached::Detached(Arc::clone(a)),
            MaybeDetached::Attached(a) => MaybeDetached::Attached(a.clone()),
        }
    }
}

impl<T> MaybeDetached<T> {
    fn new_detached(v: T) -> Self {
        MaybeDetached::Detached(Arc::new(Mutex::new(DetachedInner::new(v))))
    }

    fn is_attached(&self) -> bool {
        match self {
            MaybeDetached::Detached(_) => false,
            MaybeDetached::Attached(_) => true,
        }
    }

    fn attached_handler(&self) -> Option<&BasicHandler> {
        match self {
            MaybeDetached::Detached(_) => None,
            MaybeDetached::Attached(a) => Some(a),
        }
    }

    fn try_attached_state(&self) -> LoroResult<&BasicHandler> {
        match self {
            MaybeDetached::Detached(_) => Err(LoroError::MisuseDetachedContainer {
                method: "inner_state",
            }),
            MaybeDetached::Attached(a) => Ok(a),
        }
    }
}

impl<T> From<BasicHandler> for MaybeDetached<T> {
    fn from(a: BasicHandler) -> Self {
        MaybeDetached::Attached(a)
    }
}

impl BasicHandler {
    pub(crate) fn doc(&self) -> LoroDoc {
        self.doc.clone()
    }

    #[inline]
    fn with_doc_state<R>(&self, f: impl FnOnce(&mut DocState) -> R) -> R {
        let state = self.doc.state.clone();
        let mut guard = state.lock().unwrap();
        f(&mut guard)
    }

    fn with_txn<R>(
        &self,
        f: impl FnOnce(&mut Transaction) -> Result<R, LoroError>,
    ) -> Result<R, LoroError> {
        with_txn(&self.doc, f)
    }

    fn get_parent(&self) -> Option<Handler> {
        let parent_idx = self.doc.arena.get_parent(self.container_idx)?;
        let parent_id = self.doc.arena.get_container_id(parent_idx).unwrap();
        {
            let kind = parent_id.container_type();
            let handler = BasicHandler {
                container_idx: parent_idx,
                id: parent_id,
                doc: self.doc.clone(),
            };

            Some(match kind {
                ContainerType::Map => Handler::Map(MapHandler {
                    inner: handler.into(),
                }),
                ContainerType::List => Handler::List(ListHandler {
                    inner: handler.into(),
                }),
                ContainerType::Tree => Handler::Tree(TreeHandler {
                    inner: handler.into(),
                }),
                ContainerType::Text => Handler::Text(TextHandler {
                    inner: handler.into(),
                }),
                ContainerType::MovableList => Handler::MovableList(MovableListHandler {
                    inner: handler.into(),
                }),
                #[cfg(feature = "counter")]
                ContainerType::Counter => Handler::Counter(counter::CounterHandler {
                    inner: handler.into(),
                }),
                ContainerType::Unknown(_) => unreachable!(),
            })
        }
    }

    pub fn get_value(&self) -> LoroValue {
        self.doc
            .state
            .lock()
            .unwrap()
            .get_value_by_idx(self.container_idx)
    }

    pub fn get_deep_value(&self) -> LoroValue {
        self.doc
            .state
            .lock()
            .unwrap()
            .get_container_deep_value(self.container_idx)
    }

    fn with_state<R>(&self, f: impl FnOnce(&mut State) -> R) -> R {
        let mut guard = self.doc.state.lock().unwrap();
        guard.with_state_mut(self.container_idx, f)
    }

    pub fn parent(&self) -> Option<Handler> {
        self.get_parent()
    }

    fn is_deleted(&self) -> bool {
        self.doc
            .state
            .lock()
            .unwrap()
            .is_deleted(self.container_idx)
    }
}

/// Flatten attributes that allow overlap
#[derive(Clone)]
pub struct TextHandler {
    inner: MaybeDetached<RichtextState>,
}

impl HandlerTrait for TextHandler {
    fn to_handler(&self) -> Handler {
        Handler::Text(self.clone())
    }

    fn attach(
        &self,
        txn: &mut Transaction,
        parent: &BasicHandler,
        self_id: ContainerID,
    ) -> LoroResult<Self> {
        match &self.inner {
            MaybeDetached::Detached(t) => {
                let mut t = t.lock().unwrap();
                let inner = create_handler(parent, self_id);
                let text = inner.into_text().unwrap();
                let mut delta: Vec<TextDelta> = Vec::new();
                for span in t.value.iter() {
                    delta.push(TextDelta::Insert {
                        insert: span.text.to_string(),
                        attributes: span.attributes.to_option_map(),
                    });
                }

                text.apply_delta_with_txn(txn, &delta)?;
                t.attached = text.attached_handler().cloned();
                Ok(text)
            }
            MaybeDetached::Attached(a) => {
                let new_inner = create_handler(a, self_id);
                let ans = new_inner.into_text().unwrap();

                let delta = self.get_delta();
                ans.apply_delta_with_txn(txn, &delta)?;
                Ok(ans)
            }
        }
    }

    fn attached_handler(&self) -> Option<&BasicHandler> {
        self.inner.attached_handler()
    }

    fn get_value(&self) -> LoroValue {
        match &self.inner {
            MaybeDetached::Detached(t) => {
                let t = t.lock().unwrap();
                LoroValue::String((t.value.to_string()).into())
            }
            MaybeDetached::Attached(a) => a.get_value(),
        }
    }

    fn get_deep_value(&self) -> LoroValue {
        self.get_value()
    }

    fn is_attached(&self) -> bool {
        matches!(&self.inner, MaybeDetached::Attached(..))
    }

    fn kind(&self) -> ContainerType {
        ContainerType::Text
    }

    fn get_attached(&self) -> Option<Self> {
        match &self.inner {
            MaybeDetached::Detached(d) => d.lock().unwrap().attached.clone().map(|x| Self {
                inner: MaybeDetached::Attached(x),
            }),
            MaybeDetached::Attached(_a) => Some(self.clone()),
        }
    }

    fn from_handler(h: Handler) -> Option<Self> {
        match h {
            Handler::Text(x) => Some(x),
            _ => None,
        }
    }

    fn doc(&self) -> Option<LoroDoc> {
        match &self.inner {
            MaybeDetached::Detached(_) => None,
            MaybeDetached::Attached(a) => Some(a.doc()),
        }
    }
}

impl std::fmt::Debug for TextHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.inner {
            MaybeDetached::Detached(_) => {
                write!(f, "TextHandler(Unattached)")
            }
            MaybeDetached::Attached(a) => {
                write!(f, "TextHandler({:?})", &a.id)
            }
        }
    }
}

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

impl TextDelta {
    pub fn from_text_diff<'a>(diff: impl Iterator<Item = &'a TextDiffItem>) -> Vec<TextDelta> {
        let mut ans = Vec::with_capacity(diff.size_hint().0);
        for iter in diff {
            match iter {
                loro_delta::DeltaItem::Retain { len, attr } => {
                    ans.push(TextDelta::Retain {
                        retain: *len,
                        attributes: if attr.0.is_empty() {
                            None
                        } else {
                            Some(attr.0.clone())
                        },
                    });
                }
                loro_delta::DeltaItem::Replace {
                    value,
                    attr,
                    delete,
                } => {
                    if value.rle_len() > 0 {
                        ans.push(TextDelta::Insert {
                            insert: value.to_string(),
                            attributes: if attr.0.is_empty() {
                                None
                            } else {
                                Some(attr.0.clone())
                            },
                        });
                    }
                    if *delete > 0 {
                        ans.push(TextDelta::Delete { delete: *delete });
                    }
                }
            }
        }

        ans
    }

    pub fn into_text_diff(vec: impl Iterator<Item = Self>) -> TextDiff {
        let mut delta = TextDiff::new();
        for item in vec {
            match item {
                TextDelta::Retain { retain, attributes } => {
                    delta.push_retain(retain, TextMeta(attributes.unwrap_or_default().clone()));
                }
                TextDelta::Insert { insert, attributes } => {
                    delta.push_insert(
                        StringSlice::from(insert.as_str()),
                        TextMeta(attributes.unwrap_or_default()),
                    );
                }
                TextDelta::Delete { delete } => {
                    delta.push_delete(delete);
                }
            }
        }

        delta
    }
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

#[derive(Clone)]
pub struct MapHandler {
    inner: MaybeDetached<FxHashMap<String, ValueOrHandler>>,
}

impl HandlerTrait for MapHandler {
    fn is_attached(&self) -> bool {
        matches!(&self.inner, MaybeDetached::Attached(..))
    }

    fn attached_handler(&self) -> Option<&BasicHandler> {
        match &self.inner {
            MaybeDetached::Detached(_) => None,
            MaybeDetached::Attached(a) => Some(a),
        }
    }

    fn get_value(&self) -> LoroValue {
        match &self.inner {
            MaybeDetached::Detached(m) => {
                let m = m.lock().unwrap();
                let mut map = FxHashMap::default();
                for (k, v) in m.value.iter() {
                    map.insert(k.to_string(), v.to_value());
                }
                LoroValue::Map(map.into())
            }
            MaybeDetached::Attached(a) => a.get_value(),
        }
    }

    fn get_deep_value(&self) -> LoroValue {
        match &self.inner {
            MaybeDetached::Detached(m) => {
                let m = m.lock().unwrap();
                let mut map = FxHashMap::default();
                for (k, v) in m.value.iter() {
                    map.insert(k.to_string(), v.to_deep_value());
                }
                LoroValue::Map(map.into())
            }
            MaybeDetached::Attached(a) => a.get_deep_value(),
        }
    }

    fn kind(&self) -> ContainerType {
        ContainerType::Map
    }

    fn to_handler(&self) -> Handler {
        Handler::Map(self.clone())
    }

    fn attach(
        &self,
        txn: &mut Transaction,
        parent: &BasicHandler,
        self_id: ContainerID,
    ) -> LoroResult<Self> {
        match &self.inner {
            MaybeDetached::Detached(m) => {
                let mut m = m.lock().unwrap();
                let inner = create_handler(parent, self_id);
                let map = inner.into_map().unwrap();
                for (k, v) in m.value.iter() {
                    match v {
                        ValueOrHandler::Value(v) => {
                            map.insert_with_txn(txn, k, v.clone())?;
                        }
                        ValueOrHandler::Handler(h) => {
                            map.insert_container_with_txn(txn, k, h.clone())?;
                        }
                    }
                }
                m.attached = map.attached_handler().cloned();
                Ok(map)
            }
            MaybeDetached::Attached(a) => {
                let new_inner = create_handler(a, self_id);
                let ans = new_inner.into_map().unwrap();

                for (k, v) in self.get_value().into_map().unwrap().iter() {
                    if let LoroValue::Container(id) = v {
                        ans.insert_container_with_txn(txn, k, create_handler(a, id.clone()))?;
                    } else {
                        ans.insert_with_txn(txn, k, v.clone())?;
                    }
                }

                Ok(ans)
            }
        }
    }

    fn get_attached(&self) -> Option<Self> {
        match &self.inner {
            MaybeDetached::Detached(d) => d.lock().unwrap().attached.clone().map(|x| Self {
                inner: MaybeDetached::Attached(x),
            }),
            MaybeDetached::Attached(_a) => Some(self.clone()),
        }
    }

    fn from_handler(h: Handler) -> Option<Self> {
        match h {
            Handler::Map(x) => Some(x),
            _ => None,
        }
    }

    fn doc(&self) -> Option<LoroDoc> {
        match &self.inner {
            MaybeDetached::Detached(_) => None,
            MaybeDetached::Attached(a) => Some(a.doc()),
        }
    }
}

impl std::fmt::Debug for MapHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.inner {
            MaybeDetached::Detached(_) => write!(f, "MapHandler Detached"),
            MaybeDetached::Attached(a) => write!(f, "MapHandler {}", a.id),
        }
    }
}

#[derive(Clone)]
pub struct ListHandler {
    inner: MaybeDetached<Vec<ValueOrHandler>>,
}

#[derive(Clone)]
pub struct MovableListHandler {
    inner: MaybeDetached<Vec<ValueOrHandler>>,
}

impl HandlerTrait for MovableListHandler {
    fn is_attached(&self) -> bool {
        matches!(&self.inner, MaybeDetached::Attached(..))
    }

    fn attached_handler(&self) -> Option<&BasicHandler> {
        match &self.inner {
            MaybeDetached::Detached(_) => None,
            MaybeDetached::Attached(a) => Some(a),
        }
    }

    fn get_value(&self) -> LoroValue {
        match &self.inner {
            MaybeDetached::Detached(a) => {
                let a = a.lock().unwrap();
                LoroValue::List(a.value.iter().map(|v| v.to_value()).collect())
            }
            MaybeDetached::Attached(a) => a.get_value(),
        }
    }

    fn get_deep_value(&self) -> LoroValue {
        match &self.inner {
            MaybeDetached::Detached(a) => {
                let a = a.lock().unwrap();
                LoroValue::List(a.value.iter().map(|v| v.to_deep_value()).collect())
            }
            MaybeDetached::Attached(a) => a.get_deep_value(),
        }
    }

    fn kind(&self) -> ContainerType {
        ContainerType::MovableList
    }

    fn to_handler(&self) -> Handler {
        Handler::MovableList(self.clone())
    }

    fn from_handler(h: Handler) -> Option<Self> {
        match h {
            Handler::MovableList(x) => Some(x),
            _ => None,
        }
    }

    fn attach(
        &self,
        txn: &mut Transaction,
        parent: &BasicHandler,
        self_id: ContainerID,
    ) -> LoroResult<Self> {
        match &self.inner {
            MaybeDetached::Detached(l) => {
                let mut l = l.lock().unwrap();
                let inner = create_handler(parent, self_id);
                let list = inner.into_movable_list().unwrap();
                for (index, v) in l.value.iter().enumerate() {
                    match v {
                        ValueOrHandler::Value(v) => {
                            list.insert_with_txn(txn, index, v.clone())?;
                        }
                        ValueOrHandler::Handler(h) => {
                            list.insert_container_with_txn(txn, index, h.clone())?;
                        }
                    }
                }
                l.attached = list.attached_handler().cloned();
                Ok(list)
            }
            MaybeDetached::Attached(a) => {
                let new_inner = create_handler(a, self_id);
                let ans = new_inner.into_movable_list().unwrap();

                for (i, v) in self.get_value().into_list().unwrap().iter().enumerate() {
                    if let LoroValue::Container(id) = v {
                        ans.insert_container_with_txn(txn, i, create_handler(a, id.clone()))?;
                    } else {
                        ans.insert_with_txn(txn, i, v.clone())?;
                    }
                }

                Ok(ans)
            }
        }
    }

    fn get_attached(&self) -> Option<Self> {
        match &self.inner {
            MaybeDetached::Detached(d) => d.lock().unwrap().attached.clone().map(|x| Self {
                inner: MaybeDetached::Attached(x),
            }),
            MaybeDetached::Attached(_a) => Some(self.clone()),
        }
    }

    fn doc(&self) -> Option<LoroDoc> {
        match &self.inner {
            MaybeDetached::Detached(_) => None,
            MaybeDetached::Attached(a) => Some(a.doc()),
        }
    }
}

impl std::fmt::Debug for MovableListHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "MovableListHandler {}", self.id())
    }
}

impl std::fmt::Debug for ListHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.inner {
            MaybeDetached::Detached(_) => write!(f, "ListHandler Detached"),
            MaybeDetached::Attached(a) => write!(f, "ListHandler {}", a.id),
        }
    }
}

impl HandlerTrait for ListHandler {
    fn is_attached(&self) -> bool {
        self.inner.is_attached()
    }

    fn attached_handler(&self) -> Option<&BasicHandler> {
        self.inner.attached_handler()
    }

    fn get_value(&self) -> LoroValue {
        match &self.inner {
            MaybeDetached::Detached(a) => {
                let a = a.lock().unwrap();
                LoroValue::List(a.value.iter().map(|v| v.to_value()).collect())
            }
            MaybeDetached::Attached(a) => a.get_value(),
        }
    }

    fn get_deep_value(&self) -> LoroValue {
        match &self.inner {
            MaybeDetached::Detached(a) => {
                let a = a.lock().unwrap();
                LoroValue::List(a.value.iter().map(|v| v.to_deep_value()).collect())
            }
            MaybeDetached::Attached(a) => a.get_deep_value(),
        }
    }

    fn kind(&self) -> ContainerType {
        ContainerType::List
    }

    fn to_handler(&self) -> Handler {
        Handler::List(self.clone())
    }

    fn attach(
        &self,
        txn: &mut Transaction,
        parent: &BasicHandler,
        self_id: ContainerID,
    ) -> LoroResult<Self> {
        match &self.inner {
            MaybeDetached::Detached(l) => {
                let mut l = l.lock().unwrap();
                let inner = create_handler(parent, self_id);
                let list = inner.into_list().unwrap();
                for (index, v) in l.value.iter().enumerate() {
                    match v {
                        ValueOrHandler::Value(v) => {
                            list.insert_with_txn(txn, index, v.clone())?;
                        }
                        ValueOrHandler::Handler(h) => {
                            list.insert_container_with_txn(txn, index, h.clone())?;
                        }
                    }
                }
                l.attached = list.attached_handler().cloned();
                Ok(list)
            }
            MaybeDetached::Attached(a) => {
                let new_inner = create_handler(a, self_id);
                let ans = new_inner.into_list().unwrap();

                for (i, v) in self.get_value().into_list().unwrap().iter().enumerate() {
                    if let LoroValue::Container(id) = v {
                        ans.insert_container_with_txn(txn, i, create_handler(a, id.clone()))?;
                    } else {
                        ans.insert_with_txn(txn, i, v.clone())?;
                    }
                }

                Ok(ans)
            }
        }
    }

    fn get_attached(&self) -> Option<Self> {
        match &self.inner {
            MaybeDetached::Detached(d) => d.lock().unwrap().attached.clone().map(|x| Self {
                inner: MaybeDetached::Attached(x),
            }),
            MaybeDetached::Attached(_a) => Some(self.clone()),
        }
    }

    fn from_handler(h: Handler) -> Option<Self> {
        match h {
            Handler::List(x) => Some(x),
            _ => None,
        }
    }

    fn doc(&self) -> Option<LoroDoc> {
        match &self.inner {
            MaybeDetached::Detached(_) => None,
            MaybeDetached::Attached(a) => Some(a.doc()),
        }
    }
}

#[derive(Clone)]
pub struct UnknownHandler {
    inner: BasicHandler,
}

impl Debug for UnknownHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "UnknownHandler")
    }
}

impl UnknownHandler {
    pub fn is_deleted(&self) -> bool {
        self.inner.is_deleted()
    }
}

impl HandlerTrait for UnknownHandler {
    fn is_attached(&self) -> bool {
        true
    }

    fn attached_handler(&self) -> Option<&BasicHandler> {
        Some(&self.inner)
    }

    fn get_value(&self) -> LoroValue {
        todo!()
    }

    fn get_deep_value(&self) -> LoroValue {
        todo!()
    }

    fn kind(&self) -> ContainerType {
        self.inner.id.container_type()
    }

    fn to_handler(&self) -> Handler {
        Handler::Unknown(self.clone())
    }

    fn from_handler(h: Handler) -> Option<Self> {
        match h {
            Handler::Unknown(x) => Some(x),
            _ => None,
        }
    }

    fn attach(
        &self,
        _txn: &mut Transaction,
        _parent: &BasicHandler,
        self_id: ContainerID,
    ) -> LoroResult<Self> {
        let new_inner = create_handler(&self.inner, self_id);
        let ans = new_inner.into_unknown().unwrap();
        Ok(ans)
    }

    fn get_attached(&self) -> Option<Self> {
        Some(self.clone())
    }

    fn doc(&self) -> Option<LoroDoc> {
        Some(self.inner.doc())
    }
}

#[derive(Clone, EnumAsInner, Debug)]
pub enum Handler {
    Text(TextHandler),
    Map(MapHandler),
    List(ListHandler),
    MovableList(MovableListHandler),
    Tree(TreeHandler),
    #[cfg(feature = "counter")]
    Counter(counter::CounterHandler),
    Unknown(UnknownHandler),
}

impl HandlerTrait for Handler {
    fn is_attached(&self) -> bool {
        match self {
            Self::Text(x) => x.is_attached(),
            Self::Map(x) => x.is_attached(),
            Self::List(x) => x.is_attached(),
            Self::Tree(x) => x.is_attached(),
            Self::MovableList(x) => x.is_attached(),
            #[cfg(feature = "counter")]
            Self::Counter(x) => x.is_attached(),
            Self::Unknown(x) => x.is_attached(),
        }
    }

    fn attached_handler(&self) -> Option<&BasicHandler> {
        match self {
            Self::Text(x) => x.attached_handler(),
            Self::Map(x) => x.attached_handler(),
            Self::List(x) => x.attached_handler(),
            Self::MovableList(x) => x.attached_handler(),
            Self::Tree(x) => x.attached_handler(),
            #[cfg(feature = "counter")]
            Self::Counter(x) => x.attached_handler(),
            Self::Unknown(x) => x.attached_handler(),
        }
    }

    fn get_value(&self) -> LoroValue {
        match self {
            Self::Text(x) => x.get_value(),
            Self::Map(x) => x.get_value(),
            Self::List(x) => x.get_value(),
            Self::MovableList(x) => x.get_value(),
            Self::Tree(x) => x.get_value(),
            #[cfg(feature = "counter")]
            Self::Counter(x) => x.get_value(),
            Self::Unknown(x) => x.get_value(),
        }
    }

    fn get_deep_value(&self) -> LoroValue {
        match self {
            Self::Text(x) => x.get_deep_value(),
            Self::Map(x) => x.get_deep_value(),
            Self::List(x) => x.get_deep_value(),
            Self::MovableList(x) => x.get_deep_value(),
            Self::Tree(x) => x.get_deep_value(),
            #[cfg(feature = "counter")]
            Self::Counter(x) => x.get_deep_value(),
            Self::Unknown(x) => x.get_deep_value(),
        }
    }

    fn kind(&self) -> ContainerType {
        match self {
            Self::Text(x) => x.kind(),
            Self::Map(x) => x.kind(),
            Self::List(x) => x.kind(),
            Self::MovableList(x) => x.kind(),
            Self::Tree(x) => x.kind(),
            #[cfg(feature = "counter")]
            Self::Counter(x) => x.kind(),
            Self::Unknown(x) => x.kind(),
        }
    }

    fn to_handler(&self) -> Handler {
        match self {
            Self::Text(x) => x.to_handler(),
            Self::Map(x) => x.to_handler(),
            Self::List(x) => x.to_handler(),
            Self::MovableList(x) => x.to_handler(),
            Self::Tree(x) => x.to_handler(),
            #[cfg(feature = "counter")]
            Self::Counter(x) => x.to_handler(),
            Self::Unknown(x) => x.to_handler(),
        }
    }

    fn attach(
        &self,
        txn: &mut Transaction,
        parent: &BasicHandler,
        self_id: ContainerID,
    ) -> LoroResult<Self> {
        match self {
            Self::Text(x) => Ok(Handler::Text(x.attach(txn, parent, self_id)?)),
            Self::Map(x) => Ok(Handler::Map(x.attach(txn, parent, self_id)?)),
            Self::List(x) => Ok(Handler::List(x.attach(txn, parent, self_id)?)),
            Self::MovableList(x) => Ok(Handler::MovableList(x.attach(txn, parent, self_id)?)),
            Self::Tree(x) => Ok(Handler::Tree(x.attach(txn, parent, self_id)?)),
            #[cfg(feature = "counter")]
            Self::Counter(x) => Ok(Handler::Counter(x.attach(txn, parent, self_id)?)),
            Self::Unknown(x) => Ok(Handler::Unknown(x.attach(txn, parent, self_id)?)),
        }
    }

    fn get_attached(&self) -> Option<Self> {
        match self {
            Self::Text(x) => x.get_attached().map(Handler::Text),
            Self::Map(x) => x.get_attached().map(Handler::Map),
            Self::List(x) => x.get_attached().map(Handler::List),
            Self::MovableList(x) => x.get_attached().map(Handler::MovableList),
            Self::Tree(x) => x.get_attached().map(Handler::Tree),
            #[cfg(feature = "counter")]
            Self::Counter(x) => x.get_attached().map(Handler::Counter),
            Self::Unknown(x) => x.get_attached().map(Handler::Unknown),
        }
    }

    fn from_handler(h: Handler) -> Option<Self> {
        Some(h)
    }

    fn doc(&self) -> Option<LoroDoc> {
        match self {
            Self::Text(x) => x.doc(),
            Self::Map(x) => x.doc(),
            Self::List(x) => x.doc(),
            Self::MovableList(x) => x.doc(),
            Self::Tree(x) => x.doc(),
            #[cfg(feature = "counter")]
            Self::Counter(x) => x.doc(),
            Self::Unknown(x) => x.doc(),
        }
    }
}

impl Handler {
    pub(crate) fn new_attached(id: ContainerID, doc: LoroDoc) -> Self {
        let kind = id.container_type();
        let handler = BasicHandler {
            container_idx: doc.arena.register_container(&id),
            id,
            doc,
        };

        match kind {
            ContainerType::Map => Self::Map(MapHandler {
                inner: handler.into(),
            }),
            ContainerType::List => Self::List(ListHandler {
                inner: handler.into(),
            }),
            ContainerType::Tree => Self::Tree(TreeHandler {
                inner: handler.into(),
            }),
            ContainerType::Text => Self::Text(TextHandler {
                inner: handler.into(),
            }),
            ContainerType::MovableList => Self::MovableList(MovableListHandler {
                inner: handler.into(),
            }),
            #[cfg(feature = "counter")]
            ContainerType::Counter => Self::Counter(counter::CounterHandler {
                inner: handler.into(),
            }),
            ContainerType::Unknown(_) => Self::Unknown(UnknownHandler { inner: handler }),
        }
    }

    #[allow(unused)]
    pub(crate) fn new_unattached(kind: ContainerType) -> Self {
        match kind {
            ContainerType::Text => Self::Text(TextHandler::new_detached()),
            ContainerType::Map => Self::Map(MapHandler::new_detached()),
            ContainerType::List => Self::List(ListHandler::new_detached()),
            ContainerType::Tree => Self::Tree(TreeHandler::new_detached()),
            ContainerType::MovableList => Self::MovableList(MovableListHandler::new_detached()),
            #[cfg(feature = "counter")]
            ContainerType::Counter => Self::Counter(counter::CounterHandler::new_detached()),
            ContainerType::Unknown(_) => unreachable!(),
        }
    }

    pub fn id(&self) -> ContainerID {
        match self {
            Self::Map(x) => x.id(),
            Self::List(x) => x.id(),
            Self::Text(x) => x.id(),
            Self::Tree(x) => x.id(),
            Self::MovableList(x) => x.id(),
            #[cfg(feature = "counter")]
            Self::Counter(x) => x.id(),
            Self::Unknown(x) => x.id(),
        }
    }

    pub(crate) fn container_idx(&self) -> ContainerIdx {
        match self {
            Self::Map(x) => x.idx(),
            Self::List(x) => x.idx(),
            Self::Text(x) => x.idx(),
            Self::Tree(x) => x.idx(),
            Self::MovableList(x) => x.idx(),
            #[cfg(feature = "counter")]
            Self::Counter(x) => x.idx(),
            Self::Unknown(x) => x.idx(),
        }
    }

    pub fn c_type(&self) -> ContainerType {
        match self {
            Self::Map(_) => ContainerType::Map,
            Self::List(_) => ContainerType::List,
            Self::Text(_) => ContainerType::Text,
            Self::Tree(_) => ContainerType::Tree,
            Self::MovableList(_) => ContainerType::MovableList,
            #[cfg(feature = "counter")]
            Self::Counter(_) => ContainerType::Counter,
            Self::Unknown(x) => x.id().container_type(),
        }
    }

    fn get_deep_value(&self) -> LoroValue {
        match self {
            Self::Map(x) => x.get_deep_value(),
            Self::List(x) => x.get_deep_value(),
            Self::MovableList(x) => x.get_deep_value(),
            Self::Text(x) => x.get_deep_value(),
            Self::Tree(x) => x.get_deep_value(),
            #[cfg(feature = "counter")]
            Self::Counter(x) => x.get_deep_value(),
            Self::Unknown(x) => x.get_deep_value(),
        }
    }

    pub(crate) fn apply_diff(
        &self,
        diff: Diff,
        container_remap: &mut FxHashMap<ContainerID, ContainerID>,
    ) -> LoroResult<()> {
        // In this method we will not clone the values of the containers if
        // they are remapped. It's the caller's duty to do so
        let on_container_remap = &mut |old_id, new_id| {
            container_remap.insert(old_id, new_id);
        };
        match self {
            Self::Map(x) => {
                let diff = diff.into_map().unwrap();
                for (key, value) in diff.updated.into_iter() {
                    match value.value {
                        Some(ValueOrHandler::Handler(h)) => {
                            let old_id = h.id();
                            let new_h = x.insert_container(
                                &key,
                                Handler::new_unattached(old_id.container_type()),
                            )?;
                            let new_id = new_h.id();
                            on_container_remap(old_id, new_id);
                        }
                        Some(ValueOrHandler::Value(LoroValue::Container(old_id))) => {
                            let new_h = x.insert_container(
                                &key,
                                Handler::new_unattached(old_id.container_type()),
                            )?;
                            let new_id = new_h.id();
                            on_container_remap(old_id, new_id);
                        }
                        Some(ValueOrHandler::Value(v)) => {
                            x.insert_without_skipping(&key, v)?;
                        }
                        None => {
                            x.delete(&key)?;
                        }
                    }
                }
            }
            Self::Text(x) => {
                let delta = diff.into_text().unwrap();
                x.apply_delta(&TextDelta::from_text_diff(delta.iter()))?;
            }
            Self::List(x) => {
                let delta = diff.into_list().unwrap();
                x.apply_delta(delta, on_container_remap)?;
            }
            Self::MovableList(x) => {
                let delta = diff.into_list().unwrap();
                x.apply_delta(delta, container_remap)?;
            }
            Self::Tree(x) => {
                fn remap_tree_id(
                    id: &mut TreeID,
                    container_remap: &FxHashMap<ContainerID, ContainerID>,
                ) {
                    let mut remapped = false;
                    let mut map_id = id.associated_meta_container();
                    while let Some(rid) = container_remap.get(&map_id) {
                        remapped = true;
                        map_id = rid.clone();
                    }
                    if remapped {
                        *id = TreeID::new(
                            *map_id.as_normal().unwrap().0,
                            *map_id.as_normal().unwrap().1,
                        )
                    }
                }
                for diff in diff.into_tree().unwrap().diff {
                    let mut target = diff.target;
                    match diff.action {
                        TreeExternalDiff::Create {
                            mut parent,
                            index: _,
                            position,
                        } => {
                            if let TreeParentId::Node(p) = &mut parent {
                                remap_tree_id(p, container_remap)
                            }
                            remap_tree_id(&mut target, container_remap);
                            if !x.is_node_unexist(&target) && !x.is_node_deleted(&target)? {
                                // 1@0 is the parent of 2@1
                                // ┌────┐    ┌───────────────┐
                                // │xxxx│◀───│Move 2@1 to 0@0◀┐
                                // └────┘    └───────────────┘│
                                // ┌───────┐                  │ ┌────────┐
                                // │Del 1@0│◀─────────────────┴─│Meta 2@1│ ◀───  undo 2 ops redo 2 ops
                                // └───────┘                    └────────┘
                                //
                                // When we undo the delete operation, we should not create a new tree node and its child.
                                // However, the concurrent operation has moved the child to another parent. It's still alive.
                                // So when we redo the delete operation, we should check if the target is still alive.
                                // If it's alive, we should move it back instead of creating new one.
                                x.move_at_with_target_for_apply_diff(parent, position, target)?;
                            } else {
                                let new_target = x.__internal__next_tree_id();
                                if x.create_at_with_target_for_apply_diff(
                                    parent, position, new_target,
                                )? {
                                    container_remap.insert(
                                        target.associated_meta_container(),
                                        new_target.associated_meta_container(),
                                    );
                                }
                            }
                        }
                        TreeExternalDiff::Move {
                            mut parent,
                            index: _,
                            position,
                            old_parent: _,
                            old_index: _,
                        } => {
                            if let TreeParentId::Node(p) = &mut parent {
                                remap_tree_id(p, container_remap)
                            }
                            remap_tree_id(&mut target, container_remap);
                            // determine if the target is deleted
                            if x.is_node_unexist(&target) || x.is_node_deleted(&target).unwrap() {
                                // create the target node, we should use the new target id
                                let new_target = x.__internal__next_tree_id();
                                if x.create_at_with_target_for_apply_diff(
                                    parent, position, new_target,
                                )? {
                                    container_remap.insert(
                                        target.associated_meta_container(),
                                        new_target.associated_meta_container(),
                                    );
                                }
                            } else {
                                x.move_at_with_target_for_apply_diff(parent, position, target)?;
                            }
                        }
                        TreeExternalDiff::Delete { .. } => {
                            remap_tree_id(&mut target, container_remap);
                            if !x.is_node_deleted(&target).unwrap() {
                                x.delete(target)?;
                            }
                        }
                    }
                }
            }
            #[cfg(feature = "counter")]
            Self::Counter(x) => {
                let delta = diff.into_counter().unwrap();
                x.increment(delta)?;
            }
            Self::Unknown(_) => {
                // do nothing
            }
        }

        Ok(())
    }

    pub fn clear(&self) -> LoroResult<()> {
        match self {
            Handler::Text(text_handler) => text_handler.clear(),
            Handler::Map(map_handler) => map_handler.clear(),
            Handler::List(list_handler) => list_handler.clear(),
            Handler::MovableList(movable_list_handler) => movable_list_handler.clear(),
            Handler::Tree(tree_handler) => tree_handler.clear(),
            #[cfg(feature = "counter")]
            Handler::Counter(counter_handler) => counter_handler.clear(),
            Handler::Unknown(_unknown_handler) => Ok(()),
        }
    }
}

#[derive(Clone, EnumAsInner, Debug)]
pub enum ValueOrHandler {
    Value(LoroValue),
    Handler(Handler),
}

impl ValueOrHandler {
    pub(crate) fn from_value(value: LoroValue, doc: &Arc<LoroDocInner>) -> Self {
        if let LoroValue::Container(c) = value {
            ValueOrHandler::Handler(Handler::new_attached(c, LoroDoc::from_inner(doc.clone())))
        } else {
            ValueOrHandler::Value(value)
        }
    }

    pub(crate) fn to_value(&self) -> LoroValue {
        match self {
            Self::Value(v) => v.clone(),
            Self::Handler(h) => LoroValue::Container(h.id().clone()),
        }
    }

    pub(crate) fn to_deep_value(&self) -> LoroValue {
        match self {
            Self::Value(v) => v.clone(),
            Self::Handler(h) => h.get_deep_value(),
        }
    }
}

impl From<LoroValue> for ValueOrHandler {
    fn from(value: LoroValue) -> Self {
        ValueOrHandler::Value(value)
    }
}

impl TextHandler {
    /// Create a new container that is detached from the document.
    ///
    /// The edits on a detached container will not be persisted.
    /// To attach the container to the document, please insert it into an attached container.
    pub fn new_detached() -> Self {
        Self {
            inner: MaybeDetached::new_detached(RichtextState::default()),
        }
    }

    /// Get the version id of the richtext
    ///
    /// This can be used to detect whether the richtext is changed
    pub fn version_id(&self) -> Option<usize> {
        match &self.inner {
            MaybeDetached::Detached(_) => None,
            MaybeDetached::Attached(a) => {
                Some(a.with_state(|state| state.as_richtext_state_mut().unwrap().get_version_id()))
            }
        }
    }

    pub fn get_richtext_value(&self) -> LoroValue {
        match &self.inner {
            MaybeDetached::Detached(t) => {
                let t = t.lock().unwrap();
                t.value.get_richtext_value()
            }
            MaybeDetached::Attached(a) => {
                a.with_state(|state| state.as_richtext_state_mut().unwrap().get_richtext_value())
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        match &self.inner {
            MaybeDetached::Detached(t) => t.lock().unwrap().value.is_empty(),
            MaybeDetached::Attached(a) => {
                a.with_state(|state| state.as_richtext_state_mut().unwrap().is_empty())
            }
        }
    }

    pub fn len_utf8(&self) -> usize {
        match &self.inner {
            MaybeDetached::Detached(t) => {
                let t = t.lock().unwrap();
                t.value.len_utf8()
            }
            MaybeDetached::Attached(a) => {
                a.with_state(|state| state.as_richtext_state_mut().unwrap().len_utf8())
            }
        }
    }

    pub fn len_utf16(&self) -> usize {
        match &self.inner {
            MaybeDetached::Detached(t) => {
                let t = t.lock().unwrap();
                t.value.len_utf16()
            }
            MaybeDetached::Attached(a) => {
                a.with_state(|state| state.as_richtext_state_mut().unwrap().len_utf16())
            }
        }
    }

    pub fn len_unicode(&self) -> usize {
        match &self.inner {
            MaybeDetached::Detached(t) => {
                let t = t.lock().unwrap();
                t.value.len_unicode()
            }
            MaybeDetached::Attached(a) => {
                a.with_state(|state| state.as_richtext_state_mut().unwrap().len_unicode())
            }
        }
    }

    /// if `wasm` feature is enabled, it is a UTF-16 length
    /// otherwise, it is a Unicode length
    pub fn len_event(&self) -> usize {
        if cfg!(feature = "wasm") {
            self.len_utf16()
        } else {
            self.len_unicode()
        }
    }

    fn len(&self, pos_type: PosType) -> usize {
        match &self.inner {
            MaybeDetached::Detached(t) => t.lock().unwrap().value.len(pos_type),
            MaybeDetached::Attached(a) => {
                a.with_state(|state| state.as_richtext_state_mut().unwrap().len(pos_type))
            }
        }
    }

    pub fn diagnose(&self) {
        match &self.inner {
            MaybeDetached::Detached(t) => {
                let t = t.lock().unwrap();
                t.value.diagnose();
            }
            MaybeDetached::Attached(a) => {
                a.with_state(|state| state.as_richtext_state_mut().unwrap().diagnose());
            }
        }
    }

    pub fn iter(&self, mut callback: impl FnMut(&str) -> bool) {
        match &self.inner {
            MaybeDetached::Detached(t) => {
                let t = t.lock().unwrap();
                for span in t.value.iter() {
                    if !callback(span.text.as_str()) {
                        return;
                    }
                }
            }
            MaybeDetached::Attached(a) => {
                a.with_state(|state| {
                    state.as_richtext_state_mut().unwrap().iter(callback);
                });
            }
        }
    }

    /// Get a character at `pos` in the coordinate system specified by `pos_type`.
    pub fn char_at(&self, pos: usize, pos_type: PosType) -> LoroResult<char> {
        let len = self.len(pos_type);
        if pos >= len {
            return Err(LoroError::OutOfBound {
                pos,
                len,
                info: format!("Position: {}:{}", file!(), line!()).into_boxed_str(),
            });
        }
        if let Ok(c) = match &self.inner {
            MaybeDetached::Detached(t) => {
                let t = t.lock().unwrap();
                let event_pos = match pos_type {
                    PosType::Event => pos,
                    _ => t.value.index_to_event_index(pos, pos_type),
                };
                t.value.get_char_by_event_index(event_pos)
            }
            MaybeDetached::Attached(a) => a.with_state(|state| {
                let state = state.as_richtext_state_mut().unwrap();
                let event_pos = match pos_type {
                    PosType::Event => pos,
                    _ => state.index_to_event_index(pos, pos_type),
                };
                state.get_char_by_event_index(event_pos)
            }),
        } {
            Ok(c)
        } else {
            Err(LoroError::OutOfBound {
                pos,
                len,
                info: format!("Position: {}:{}", file!(), line!()).into_boxed_str(),
            })
        }
    }

    /// `start_index` and `end_index` are Event Index:
    ///
    /// - if feature="wasm", pos is a UTF-16 index
    /// - if feature!="wasm", pos is a Unicode index
    ///
    pub fn slice(
        &self,
        start_index: usize,
        end_index: usize,
        pos_type: PosType,
    ) -> LoroResult<String> {
        self.slice_with_pos_type(start_index, end_index, pos_type)
    }

    pub fn slice_utf16(&self, start_index: usize, end_index: usize) -> LoroResult<String> {
        self.slice(start_index, end_index, PosType::Utf16)
    }

    fn slice_with_pos_type(
        &self,
        start_index: usize,
        end_index: usize,
        pos_type: PosType,
    ) -> LoroResult<String> {
        if end_index < start_index {
            return Err(LoroError::EndIndexLessThanStartIndex {
                start: start_index,
                end: end_index,
            });
        }
        if start_index == end_index {
            return Ok(String::new());
        }

        let info = || format!("Position: {}:{}", file!(), line!()).into_boxed_str();
        match &self.inner {
            MaybeDetached::Detached(t) => {
                let t = t.lock().unwrap();
                let len = t.value.len(pos_type);
                if end_index > len {
                    return Err(LoroError::OutOfBound {
                        pos: end_index,
                        len,
                        info: info(),
                    });
                }
                let (start, end) = match pos_type {
                    PosType::Event => (start_index, end_index),
                    _ => (
                        t.value.index_to_event_index(start_index, pos_type),
                        t.value.index_to_event_index(end_index, pos_type),
                    ),
                };
                t.value.get_text_slice_by_event_index(start, end - start)
            }
            MaybeDetached::Attached(a) => a.with_state(|state| {
                let state = state.as_richtext_state_mut().unwrap();
                let len = state.len(pos_type);
                if end_index > len {
                    return Err(LoroError::OutOfBound {
                        pos: end_index,
                        len,
                        info: info(),
                    });
                }
                let (start, end) = match pos_type {
                    PosType::Event => (start_index, end_index),
                    _ => (
                        state.index_to_event_index(start_index, pos_type),
                        state.index_to_event_index(end_index, pos_type),
                    ),
                };
                state.get_text_slice_by_event_index(start, end - start)
            }),
        }
    }

    pub fn slice_delta(
        &self,
        start_index: usize,
        end_index: usize,
        pos_type: PosType,
    ) -> LoroResult<Vec<TextDelta>> {
        match &self.inner {
            MaybeDetached::Detached(t) => {
                let t = t.lock().unwrap();
                let ans = t.value.slice_delta(start_index, end_index, pos_type)?;
                Ok(ans
                    .into_iter()
                    .map(|(s, a)| TextDelta::Insert {
                        insert: s,
                        attributes: a.to_option_map(),
                    })
                    .collect())
            }
            MaybeDetached::Attached(a) => a.with_state(|state| {
                let ans = state.as_richtext_state_mut().unwrap().slice_delta(
                    start_index,
                    end_index,
                    pos_type,
                )?;
                Ok(ans
                    .into_iter()
                    .map(|(s, a)| TextDelta::Insert {
                        insert: s,
                        attributes: a.to_option_map(),
                    })
                    .collect())
            }),
        }
    }

    /// `pos` is a Event Index:
    ///
    /// - if feature="wasm", pos is a UTF-16 index
    /// - if feature!="wasm", pos is a Unicode index
    ///
    /// This method requires auto_commit to be enabled.
    pub fn splice(&self, pos: usize, len: usize, s: &str, pos_type: PosType) -> LoroResult<String> {
        let x = self.slice(pos, pos + len, pos_type)?;
        self.delete(pos, len, pos_type)?;
        self.insert(pos, s, pos_type)?;
        Ok(x)
    }

    pub fn splice_utf8(&self, pos: usize, len: usize, s: &str) -> LoroResult<()> {
        // let x = self.slice(pos, pos + len)?;
        self.delete_utf8(pos, len)?;
        self.insert_utf8(pos, s)?;
        Ok(())
    }

    pub fn splice_utf16(&self, pos: usize, len: usize, s: &str) -> LoroResult<()> {
        self.delete(pos, len, PosType::Utf16)?;
        self.insert(pos, s, PosType::Utf16)?;
        Ok(())
    }

    /// Insert text at `pos` using the given `pos_type` coordinate system.
    ///
    /// This method requires auto_commit to be enabled.
    pub fn insert(&self, pos: usize, s: &str, pos_type: PosType) -> LoroResult<()> {
        match &self.inner {
            MaybeDetached::Detached(t) => {
                let mut t = t.lock().unwrap();
                let (index, _) = t
                    .value
                    .get_entity_index_for_text_insert(pos, pos_type)
                    .unwrap();
                t.value.insert_at_entity_index(
                    index,
                    BytesSlice::from_bytes(s.as_bytes()),
                    IdFull::NONE_ID,
                );
                Ok(())
            }
            MaybeDetached::Attached(a) => {
                a.with_txn(|txn| self.insert_with_txn(txn, pos, s, pos_type))
            }
        }
    }

    pub fn insert_utf8(&self, pos: usize, s: &str) -> LoroResult<()> {
        self.insert(pos, s, PosType::Bytes)
    }

    pub fn insert_utf16(&self, pos: usize, s: &str) -> LoroResult<()> {
        self.insert(pos, s, PosType::Utf16)
    }

    pub fn insert_unicode(&self, pos: usize, s: &str) -> LoroResult<()> {
        self.insert(pos, s, PosType::Unicode)
    }

    /// Insert text within an existing transaction using the provided `pos_type`.
    pub fn insert_with_txn(
        &self,
        txn: &mut Transaction,
        pos: usize,
        s: &str,
        pos_type: PosType,
    ) -> LoroResult<()> {
        self.insert_with_txn_and_attr(txn, pos, s, None, pos_type)?;
        Ok(())
    }

    pub fn insert_with_txn_utf8(
        &self,
        txn: &mut Transaction,
        pos: usize,
        s: &str,
    ) -> LoroResult<()> {
        self.insert_with_txn(txn, pos, s, PosType::Bytes)
    }

    /// Delete a span using the coordinate system described by `pos_type`.
    ///
    /// This method requires auto_commit to be enabled.
    pub fn delete(&self, pos: usize, len: usize, pos_type: PosType) -> LoroResult<()> {
        match &self.inner {
            MaybeDetached::Detached(t) => {
                let mut t = t.lock().unwrap();
                let ranges = t.value.get_text_entity_ranges(pos, len, pos_type)?;
                for range in ranges.iter().rev() {
                    t.value
                        .drain_by_entity_index(range.entity_start, range.entity_len(), None);
                }
                Ok(())
            }
            MaybeDetached::Attached(a) => {
                a.with_txn(|txn| self.delete_with_txn(txn, pos, len, pos_type))
            }
        }
    }

    pub fn delete_utf8(&self, pos: usize, len: usize) -> LoroResult<()> {
        self.delete(pos, len, PosType::Bytes)
    }

    pub fn delete_utf16(&self, pos: usize, len: usize) -> LoroResult<()> {
        self.delete(pos, len, PosType::Utf16)
    }

    pub fn delete_unicode(&self, pos: usize, len: usize) -> LoroResult<()> {
        self.delete(pos, len, PosType::Unicode)
    }

    /// If attr is specified, it will be used as the attribute of the inserted text.
    /// It will override the existing attribute of the text.
    fn insert_with_txn_and_attr(
        &self,
        txn: &mut Transaction,
        pos: usize,
        s: &str,
        attr: Option<&FxHashMap<String, LoroValue>>,
        pos_type: PosType,
    ) -> Result<Vec<(InternalString, LoroValue)>, LoroError> {
        if s.is_empty() {
            return Ok(Vec::new());
        }

        match pos_type {
            PosType::Event => {
                if pos > self.len_event() {
                    return Err(LoroError::OutOfBound {
                        pos,
                        len: self.len_event(),
                        info: format!("Position: {}:{}", file!(), line!()).into_boxed_str(),
                    });
                }
            }
            PosType::Bytes => {
                if pos > self.len_utf8() {
                    return Err(LoroError::OutOfBound {
                        pos,
                        len: self.len_utf8(),
                        info: format!("Position: {}:{}", file!(), line!()).into_boxed_str(),
                    });
                }
            }
            PosType::Unicode => {
                if pos > self.len_unicode() {
                    return Err(LoroError::OutOfBound {
                        pos,
                        len: self.len_unicode(),
                        info: format!("Position: {}:{}", file!(), line!()).into_boxed_str(),
                    });
                }
            }
            PosType::Entity => {}
            PosType::Utf16 => {
                if pos > self.len_utf16() {
                    return Err(LoroError::OutOfBound {
                        pos,
                        len: self.len_utf16(),
                        info: format!("Position: {}:{}", file!(), line!()).into_boxed_str(),
                    });
                }
            }
        }

        let inner = self.inner.try_attached_state()?;
        let (entity_index, event_index, styles) = inner.with_state(|state| {
            let richtext_state = state.as_richtext_state_mut().unwrap();
            let ret = richtext_state.get_entity_index_for_text_insert(pos, pos_type);
            let (entity_index, cursor) = match ret {
                Err(_) => match pos_type {
                    PosType::Bytes => {
                        return (
                            Err(LoroError::UTF8InUnicodeCodePoint { pos }),
                            0,
                            StyleMeta::empty(),
                        );
                    }
                    PosType::Utf16 | PosType::Event => {
                        return (
                            Err(LoroError::UTF16InUnicodeCodePoint { pos }),
                            0,
                            StyleMeta::empty(),
                        );
                    }
                    _ => unreachable!(),
                },
                Ok(x) => x,
            };
            let event_index = if let Some(cursor) = cursor {
                if pos_type == PosType::Event {
                    debug_assert_eq!(
                        richtext_state.get_event_index_by_cursor(cursor),
                        pos,
                        "pos={} cursor={:?} state={:#?}",
                        pos,
                        cursor,
                        &richtext_state
                    );
                    pos
                } else {
                    richtext_state.get_event_index_by_cursor(cursor)
                }
            } else {
                assert_eq!(entity_index, 0);
                0
            };
            let styles = richtext_state.get_styles_at_entity_index(entity_index);
            (Ok(entity_index), event_index, styles)
        });

        let entity_index = match entity_index {
            Err(x) => return Err(x),
            _ => entity_index.unwrap(),
        };

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
            inner.container_idx,
            crate::op::RawOpContent::List(crate::container::list::list_op::ListOp::Insert {
                slice: ListSlice::RawStr {
                    str: Cow::Borrowed(s),
                    unicode_len,
                },
                pos: entity_index,
            }),
            EventHint::InsertText {
                pos: event_index as u32,
                styles,
                unicode_len: unicode_len as u32,
                event_len: event_len as u32,
            },
            &inner.doc,
        )?;

        Ok(override_styles)
    }

    /// Delete text within a transaction using the specified `pos_type`.
    pub fn delete_with_txn(
        &self,
        txn: &mut Transaction,
        pos: usize,
        len: usize,
        pos_type: PosType,
    ) -> LoroResult<()> {
        self.delete_with_txn_inline(txn, pos, len, pos_type)
    }

    fn delete_with_txn_inline(
        &self,
        txn: &mut Transaction,
        pos: usize,
        len: usize,
        pos_type: PosType,
    ) -> LoroResult<()> {
        if len == 0 {
            return Ok(());
        }

        if pos + len > self.len(pos_type) {
            error!("pos={} len={} len_event={}", pos, len, self.len_event());
            return Err(LoroError::OutOfBound {
                pos: pos + len,
                len: self.len_event(),
                info: format!("Position: {}:{}", file!(), line!()).into_boxed_str(),
            });
        }

        let inner = self.inner.try_attached_state()?;
        let s = tracing::span!(tracing::Level::INFO, "delete", "pos={} len={}", pos, len);
        let _e = s.enter();
        let mut event_pos = 0;
        let mut event_len = 0;
        let ranges = inner.with_state(|state| {
            let richtext_state = state.as_richtext_state_mut().unwrap();
            event_pos = richtext_state.index_to_event_index(pos, pos_type);
            let event_end = richtext_state.index_to_event_index(pos + len, pos_type);
            event_len = event_end - event_pos;

            richtext_state.get_text_entity_ranges_in_event_index_range(event_pos, event_len)
        })?;

        //debug_assert_eq!(ranges.iter().map(|x| x.event_len).sum::<usize>(), len);
        let pos = event_pos as isize;
        let len = event_len as isize;
        let mut event_end = pos + len;
        for range in ranges.iter().rev() {
            let event_start = event_end - range.event_len as isize;
            txn.apply_local_op(
                inner.container_idx,
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
                &inner.doc,
            )?;
            event_end = event_start;
        }

        Ok(())
    }

    /// `start` and `end` are interpreted using `pos_type`.
    ///
    /// This method requires auto_commit to be enabled.
    pub fn mark(
        &self,
        start: usize,
        end: usize,
        key: impl Into<InternalString>,
        value: LoroValue,
        pos_type: PosType,
    ) -> LoroResult<()> {
        match &self.inner {
            MaybeDetached::Detached(t) => {
                let mut g = t.lock().unwrap();
                self.mark_for_detached(&mut g.value, key, &value, start, end, pos_type, false)
            }
            MaybeDetached::Attached(a) => {
                a.with_txn(|txn| self.mark_with_txn(txn, start, end, key, value, pos_type, false))
            }
        }
    }

    fn mark_for_detached(
        &self,
        state: &mut RichtextState,
        key: impl Into<InternalString>,
        value: &LoroValue,
        start: usize,
        end: usize,
        pos_type: PosType,
        is_delete: bool,
    ) -> Result<(), LoroError> {
        let key: InternalString = key.into();
        if start >= end {
            return Err(loro_common::LoroError::ArgErr(
                "Start must be less than end".to_string().into_boxed_str(),
            ));
        }

        let len = state.len(pos_type);
        if end > len {
            return Err(LoroError::OutOfBound {
                pos: end,
                len,
                info: format!("Position: {}:{}", file!(), line!()).into_boxed_str(),
            });
        }
        let (entity_range, styles) =
            state.get_entity_range_and_text_styles_at_range(start..end, pos_type);
        if let Some(styles) = styles {
            if styles.has_key_value(&key, value) {
                // already has the same style, skip
                return Ok(());
            }
        }

        let style_op = Arc::new(StyleOp {
            lamport: 0,
            peer: 0,
            cnt: 0,
            key,
            value: value.clone(),
            // TODO: describe this behavior in the document
            info: if is_delete {
                TextStyleInfoFlag::BOLD.to_delete()
            } else {
                TextStyleInfoFlag::BOLD
            },
        });
        state.mark_with_entity_index(entity_range, style_op);
        Ok(())
    }

    /// `start` and `end` are interpreted using `pos_type`.
    pub fn unmark(
        &self,
        start: usize,
        end: usize,
        key: impl Into<InternalString>,
        pos_type: PosType,
    ) -> LoroResult<()> {
        match &self.inner {
            MaybeDetached::Detached(t) => self.mark_for_detached(
                &mut t.lock().unwrap().value,
                key,
                &LoroValue::Null,
                start,
                end,
                pos_type,
                true,
            ),
            MaybeDetached::Attached(a) => a.with_txn(|txn| {
                self.mark_with_txn(txn, start, end, key, LoroValue::Null, pos_type, true)
            }),
        }
    }

    /// `start` and `end` are interpreted using `pos_type`.
    pub fn mark_with_txn(
        &self,
        txn: &mut Transaction,
        start: usize,
        end: usize,
        key: impl Into<InternalString>,
        value: LoroValue,
        pos_type: PosType,
        is_delete: bool,
    ) -> LoroResult<()> {
        if start >= end {
            return Err(loro_common::LoroError::ArgErr(
                "Start must be less than end".to_string().into_boxed_str(),
            ));
        }

        let inner = self.inner.try_attached_state()?;
        let key: InternalString = key.into();

        let mut doc_state = inner.doc.state.lock().unwrap();
        let len = doc_state.with_state_mut(inner.container_idx, |state| {
            state.as_richtext_state_mut().unwrap().len(pos_type)
        });

        if end > len {
            return Err(LoroError::OutOfBound {
                pos: end,
                len,
                info: format!("Position: {}:{}", file!(), line!()).into_boxed_str(),
            });
        }

        let (entity_range, skip, event_start, event_end) =
            doc_state.with_state_mut(inner.container_idx, |state| {
                let state = state.as_richtext_state_mut().unwrap();
                let event_start = state.index_to_event_index(start, pos_type);
                let event_end = state.index_to_event_index(end, pos_type);
                let (entity_range, styles) =
                    state.get_entity_range_and_styles_at_range(start..end, pos_type);

                let skip = match styles {
                    Some(styles) if styles.has_key_value(&key, &value) => {
                        // already has the same style, skip
                        true
                    }
                    _ => false,
                };

                (entity_range, skip, event_start, event_end)
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
            inner.container_idx,
            crate::op::RawOpContent::List(ListOp::StyleStart {
                start: entity_start as u32,
                end: entity_end as u32,
                key: key.clone(),
                value: value.clone(),
                info: flag,
            }),
            EventHint::Mark {
                start: event_start as u32,
                end: event_end as u32,
                style: crate::container::richtext::Style { key, data: value },
            },
            &inner.doc,
        )?;

        txn.apply_local_op(
            inner.container_idx,
            crate::op::RawOpContent::List(ListOp::StyleEnd),
            EventHint::MarkEnd,
            &inner.doc,
        )?;

        Ok(())
    }

    pub fn check(&self) {
        match &self.inner {
            MaybeDetached::Detached(t) => {
                let t = t.lock().unwrap();
                t.value.check_consistency_between_content_and_style_ranges();
            }
            MaybeDetached::Attached(a) => a.with_state(|state| {
                state
                    .as_richtext_state_mut()
                    .unwrap()
                    .check_consistency_between_content_and_style_ranges();
            }),
        }
    }

    pub fn apply_delta(&self, delta: &[TextDelta]) -> LoroResult<()> {
        match &self.inner {
            MaybeDetached::Detached(t) => {
                let _t = t.lock().unwrap();
                // TODO: implement
                Err(LoroError::NotImplemented(
                    "`apply_delta` on a detached text container",
                ))
            }
            MaybeDetached::Attached(a) => a.with_txn(|txn| self.apply_delta_with_txn(txn, delta)),
        }
    }

    pub fn apply_delta_with_txn(
        &self,
        txn: &mut Transaction,
        delta: &[TextDelta],
    ) -> LoroResult<()> {
        let mut index = 0;
        struct PendingMark {
            start: usize,
            end: usize,
            attributes: FxHashMap<InternalString, LoroValue>,
        }
        let mut marks: Vec<PendingMark> = Vec::new();
        for d in delta {
            match d {
                TextDelta::Insert { insert, attributes } => {
                    let end = index + event_len(insert.as_str());
                    let override_styles = self.insert_with_txn_and_attr(
                        txn,
                        index,
                        insert.as_str(),
                        Some(attributes.as_ref().unwrap_or(&Default::default())),
                        PosType::Event,
                    )?;

                    let mut pending_mark = PendingMark {
                        start: index,
                        end,
                        attributes: FxHashMap::default(),
                    };
                    for (key, value) in override_styles {
                        pending_mark.attributes.insert(key, value);
                    }
                    marks.push(pending_mark);
                    index = end;
                }
                TextDelta::Delete { delete } => {
                    self.delete_with_txn(txn, index, *delete, PosType::Event)?;
                }
                TextDelta::Retain { attributes, retain } => {
                    let end = index + *retain;
                    match attributes {
                        Some(attr) if !attr.is_empty() => {
                            let mut pending_mark = PendingMark {
                                start: index,
                                end,
                                attributes: FxHashMap::default(),
                            };
                            for (key, value) in attr {
                                pending_mark
                                    .attributes
                                    .insert(key.deref().into(), value.clone());
                            }
                            marks.push(pending_mark);
                        }
                        _ => {}
                    }
                    index = end;
                }
            }
        }

        let mut len = self.len_event();
        for pending_mark in marks {
            if pending_mark.start >= len {
                self.insert_with_txn(
                    txn,
                    len,
                    &"\n".repeat(pending_mark.start - len + 1),
                    PosType::Event,
                )?;
                len = pending_mark.start;
            }

            for (key, value) in pending_mark.attributes {
                self.mark_with_txn(
                    txn,
                    pending_mark.start,
                    pending_mark.end,
                    key.deref(),
                    value,
                    PosType::Event,
                    false,
                )?;
            }
        }

        Ok(())
    }

    pub fn update(&self, text: &str, options: UpdateOptions) -> Result<(), UpdateTimeoutError> {
        let old_str = self.to_string();
        let new = text.chars().map(|x| x as u32).collect::<Vec<u32>>();
        let old = old_str.chars().map(|x| x as u32).collect::<Vec<u32>>();
        diff(
            &mut OperateProxy::new(text_update::DiffHook::new(self, &new)),
            options,
            &old,
            &new,
        )?;
        Ok(())
    }

    pub fn update_by_line(
        &self,
        text: &str,
        options: UpdateOptions,
    ) -> Result<(), UpdateTimeoutError> {
        let hook = text_update::DiffHookForLine::new(self, text);
        let old_lines = hook.get_old_arr().to_vec();
        let new_lines = hook.get_new_arr().to_vec();
        diff(
            &mut OperateProxy::new(hook),
            options,
            &old_lines,
            &new_lines,
        )
    }

    #[allow(clippy::inherent_to_string)]
    pub fn to_string(&self) -> String {
        match &self.inner {
            MaybeDetached::Detached(t) => t.lock().unwrap().value.to_string(),
            MaybeDetached::Attached(a) => a.get_value().into_string().unwrap().unwrap(),
        }
    }

    pub fn get_cursor(&self, event_index: usize, side: Side) -> Option<Cursor> {
        self.get_cursor_internal(event_index, side, true)
    }

    /// Get the stable position representation for the target pos
    pub(crate) fn get_cursor_internal(
        &self,
        index: usize,
        side: Side,
        get_by_event_index: bool,
    ) -> Option<Cursor> {
        match &self.inner {
            MaybeDetached::Detached(_) => None,
            MaybeDetached::Attached(a) => {
                let (id, len, origin_pos) = a.with_state(|s| {
                    let s = s.as_richtext_state_mut().unwrap();
                    (
                        s.get_stable_position(index, get_by_event_index),
                        if get_by_event_index {
                            s.len_event()
                        } else {
                            s.len_unicode()
                        },
                        if get_by_event_index {
                            s.event_index_to_unicode_index(index)
                        } else {
                            index
                        },
                    )
                });

                if len == 0 {
                    return Some(Cursor {
                        id: None,
                        container: self.id(),
                        side: if side == Side::Middle {
                            Side::Left
                        } else {
                            side
                        },
                        origin_pos: 0,
                    });
                }

                if len <= index {
                    return Some(Cursor {
                        id: None,
                        container: self.id(),
                        side: Side::Right,
                        origin_pos: len,
                    });
                }

                let id = id?;
                Some(Cursor {
                    id: Some(id),
                    container: self.id(),
                    side,
                    origin_pos,
                })
            }
        }
    }

    pub(crate) fn convert_entity_index_to_event_index(&self, entity_index: usize) -> usize {
        match &self.inner {
            MaybeDetached::Detached(s) => s
                .lock()
                .unwrap()
                .value
                .entity_index_to_event_index(entity_index),
            MaybeDetached::Attached(a) => {
                let mut pos = 0;
                a.with_state(|s| {
                    let s = s.as_richtext_state_mut().unwrap();
                    pos = s.entity_index_to_event_index(entity_index);
                });
                pos
            }
        }
    }

    pub fn get_delta(&self) -> Vec<TextDelta> {
        match &self.inner {
            MaybeDetached::Detached(s) => {
                let mut delta = Vec::new();
                for span in s.lock().unwrap().value.iter() {
                    let next_attr = span.attributes.to_option_map();
                    match delta.last_mut() {
                        Some(TextDelta::Insert { insert, attributes })
                            if &next_attr == attributes =>
                        {
                            insert.push_str(span.text.as_str());
                            continue;
                        }
                        _ => {}
                    }
                    delta.push(TextDelta::Insert {
                        insert: span.text.as_str().to_string(),
                        attributes: next_attr,
                    })
                }
                delta
            }
            MaybeDetached::Attached(_a) => self
                .with_state(|state| {
                    let state = state.as_richtext_state_mut().unwrap();
                    Ok(state.get_delta())
                })
                .unwrap(),
        }
    }

    pub fn is_deleted(&self) -> bool {
        match &self.inner {
            MaybeDetached::Detached(_) => false,
            MaybeDetached::Attached(a) => a.is_deleted(),
        }
    }

    pub fn push_str(&self, s: &str) -> LoroResult<()> {
        self.insert_utf8(self.len_utf8(), s)
    }

    pub fn clear(&self) -> LoroResult<()> {
        match &self.inner {
            MaybeDetached::Detached(mutex) => {
                let mut t = mutex.lock().unwrap();
                let len = t.value.len_unicode();
                let ranges = t.value.get_text_entity_ranges(0, len, PosType::Unicode)?;
                for range in ranges.iter().rev() {
                    t.value
                        .drain_by_entity_index(range.entity_start, range.entity_len(), None);
                }
                Ok(())
            }
            MaybeDetached::Attached(a) => a.with_txn(|txn| {
                let len = a.with_state(|s| s.as_richtext_state_mut().unwrap().len_unicode());
                self.delete_with_txn_inline(txn, 0, len, PosType::Unicode)
            }),
        }
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
    /// Create a new container that is detached from the document.
    /// The edits on a detached container will not be persisted.
    /// To attach the container to the document, please insert it into an attached container.
    pub fn new_detached() -> Self {
        Self {
            inner: MaybeDetached::new_detached(Vec::new()),
        }
    }

    pub fn insert(&self, pos: usize, v: impl Into<LoroValue>) -> LoroResult<()> {
        match &self.inner {
            MaybeDetached::Detached(l) => {
                let mut list = l.lock().unwrap();
                list.value.insert(pos, ValueOrHandler::Value(v.into()));
                Ok(())
            }
            MaybeDetached::Attached(a) => {
                a.with_txn(|txn| self.insert_with_txn(txn, pos, v.into()))
            }
        }
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
                info: format!("Position: {}:{}", file!(), line!()).into_boxed_str(),
                len: self.len(),
            });
        }

        let inner = self.inner.try_attached_state()?;
        if let Some(_container) = v.as_container() {
            return Err(LoroError::ArgErr(
                INSERT_CONTAINER_VALUE_ARG_ERROR
                    .to_string()
                    .into_boxed_str(),
            ));
        }

        txn.apply_local_op(
            inner.container_idx,
            crate::op::RawOpContent::List(crate::container::list::list_op::ListOp::Insert {
                slice: ListSlice::RawData(Cow::Owned(vec![v.clone()])),
                pos,
            }),
            EventHint::InsertList { len: 1, pos },
            &inner.doc,
        )
    }

    pub fn push(&self, v: impl Into<LoroValue>) -> LoroResult<()> {
        match &self.inner {
            MaybeDetached::Detached(l) => {
                let mut list = l.lock().unwrap();
                list.value.push(ValueOrHandler::Value(v.into()));
                Ok(())
            }
            MaybeDetached::Attached(a) => a.with_txn(|txn| self.push_with_txn(txn, v.into())),
        }
    }

    pub fn push_with_txn(&self, txn: &mut Transaction, v: LoroValue) -> LoroResult<()> {
        let pos = self.len();
        self.insert_with_txn(txn, pos, v)
    }

    pub fn pop(&self) -> LoroResult<Option<LoroValue>> {
        match &self.inner {
            MaybeDetached::Detached(l) => {
                let mut list = l.lock().unwrap();
                Ok(list.value.pop().map(|v| v.to_value()))
            }
            MaybeDetached::Attached(a) => a.with_txn(|txn| self.pop_with_txn(txn)),
        }
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

    pub fn insert_container<H: HandlerTrait>(&self, pos: usize, child: H) -> LoroResult<H> {
        match &self.inner {
            MaybeDetached::Detached(l) => {
                let mut list = l.lock().unwrap();
                list.value
                    .insert(pos, ValueOrHandler::Handler(child.to_handler()));
                Ok(child)
            }
            MaybeDetached::Attached(a) => {
                a.with_txn(|txn| self.insert_container_with_txn(txn, pos, child))
            }
        }
    }

    pub fn push_container<H: HandlerTrait>(&self, child: H) -> LoroResult<H> {
        self.insert_container(self.len(), child)
    }

    pub fn insert_container_with_txn<H: HandlerTrait>(
        &self,
        txn: &mut Transaction,
        pos: usize,
        child: H,
    ) -> LoroResult<H> {
        if pos > self.len() {
            return Err(LoroError::OutOfBound {
                pos,
                info: format!("Position: {}:{}", file!(), line!()).into_boxed_str(),
                len: self.len(),
            });
        }

        let inner = self.inner.try_attached_state()?;
        let id = txn.next_id();
        let container_id = ContainerID::new_normal(id, child.kind());
        let v = LoroValue::Container(container_id.clone());
        txn.apply_local_op(
            inner.container_idx,
            crate::op::RawOpContent::List(crate::container::list::list_op::ListOp::Insert {
                slice: ListSlice::RawData(Cow::Owned(vec![v.clone()])),
                pos,
            }),
            EventHint::InsertList { len: 1, pos },
            &inner.doc,
        )?;
        let ans = child.attach(txn, inner, container_id)?;
        Ok(ans)
    }

    pub fn delete(&self, pos: usize, len: usize) -> LoroResult<()> {
        match &self.inner {
            MaybeDetached::Detached(l) => {
                let mut list = l.lock().unwrap();
                list.value.drain(pos..pos + len);
                Ok(())
            }
            MaybeDetached::Attached(a) => a.with_txn(|txn| self.delete_with_txn(txn, pos, len)),
        }
    }

    pub fn delete_with_txn(&self, txn: &mut Transaction, pos: usize, len: usize) -> LoroResult<()> {
        if len == 0 {
            return Ok(());
        }

        if pos + len > self.len() {
            return Err(LoroError::OutOfBound {
                pos: pos + len,
                info: format!("Position: {}:{}", file!(), line!()).into_boxed_str(),
                len: self.len(),
            });
        }

        let inner = self.inner.try_attached_state()?;
        let ids: Vec<_> = inner.with_state(|state| {
            let list = state.as_list_state().unwrap();
            (pos..pos + len)
                .map(|i| list.get_id_at(i).unwrap())
                .collect()
        });

        for id in ids.into_iter() {
            txn.apply_local_op(
                inner.container_idx,
                crate::op::RawOpContent::List(ListOp::Delete(DeleteSpanWithId::new(
                    id.id(),
                    pos as isize,
                    1,
                ))),
                EventHint::DeleteList(DeleteSpan::new(pos as isize, 1)),
                &inner.doc,
            )?;
        }

        Ok(())
    }

    pub fn get_child_handler(&self, index: usize) -> LoroResult<Handler> {
        match &self.inner {
            MaybeDetached::Detached(l) => {
                let list = l.lock().unwrap();
                let value = list.value.get(index).ok_or(LoroError::OutOfBound {
                    pos: index,
                    info: format!("Position: {}:{}", file!(), line!()).into_boxed_str(),
                    len: list.value.len(),
                })?;
                match value {
                    ValueOrHandler::Handler(h) => Ok(h.clone()),
                    _ => Err(LoroError::ArgErr(
                        format!(
                            "Expected container at index {}, but found {:?}",
                            index, value
                        )
                        .into_boxed_str(),
                    )),
                }
            }
            MaybeDetached::Attached(a) => {
                let Some(value) = a.with_state(|state| {
                    state.as_list_state().as_ref().unwrap().get(index).cloned()
                }) else {
                    return Err(LoroError::OutOfBound {
                        pos: index,
                        info: format!("Position: {}:{}", file!(), line!()).into_boxed_str(),
                        len: a.with_state(|state| state.as_list_state().unwrap().len()),
                    });
                };
                match value {
                    LoroValue::Container(id) => Ok(create_handler(a, id)),
                    _ => Err(LoroError::ArgErr(
                        format!(
                            "Expected container at index {}, but found {:?}",
                            index, value
                        )
                        .into_boxed_str(),
                    )),
                }
            }
        }
    }

    pub fn len(&self) -> usize {
        match &self.inner {
            MaybeDetached::Detached(l) => l.lock().unwrap().value.len(),
            MaybeDetached::Attached(a) => {
                a.with_state(|state| state.as_list_state().unwrap().len())
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn get_deep_value_with_id(&self) -> LoroResult<LoroValue> {
        let inner = self.inner.try_attached_state()?;
        Ok(inner.with_doc_state(|state| {
            state.get_container_deep_value_with_id(inner.container_idx, None)
        }))
    }

    pub fn get(&self, index: usize) -> Option<LoroValue> {
        match &self.inner {
            MaybeDetached::Detached(l) => l.lock().unwrap().value.get(index).map(|x| x.to_value()),
            MaybeDetached::Attached(a) => a.with_state(|state| {
                let a = state.as_list_state().unwrap();
                a.get(index).cloned()
            }),
        }
    }

    /// Get value at given index, if it's a container, return a handler to the container
    pub fn get_(&self, index: usize) -> Option<ValueOrHandler> {
        match &self.inner {
            MaybeDetached::Detached(l) => {
                let l = l.lock().unwrap();
                l.value.get(index).cloned()
            }
            MaybeDetached::Attached(inner) => {
                let value =
                    inner.with_state(|state| state.as_list_state().unwrap().get(index).cloned());
                match value {
                    Some(LoroValue::Container(container_id)) => Some(ValueOrHandler::Handler(
                        create_handler(inner, container_id.clone()),
                    )),
                    Some(value) => Some(ValueOrHandler::Value(value.clone())),
                    None => None,
                }
            }
        }
    }

    pub fn for_each<I>(&self, mut f: I)
    where
        I: FnMut(ValueOrHandler),
    {
        match &self.inner {
            MaybeDetached::Detached(l) => {
                let l = l.lock().unwrap();
                for v in l.value.iter() {
                    f(v.clone())
                }
            }
            MaybeDetached::Attached(inner) => {
                let mut temp = vec![];
                inner.with_state(|state| {
                    let a = state.as_list_state().unwrap();
                    for v in a.iter() {
                        match v {
                            LoroValue::Container(c) => {
                                temp.push(ValueOrHandler::Handler(create_handler(
                                    inner,
                                    c.clone(),
                                )));
                            }
                            value => {
                                temp.push(ValueOrHandler::Value(value.clone()));
                            }
                        }
                    }
                });
                for v in temp.into_iter() {
                    f(v);
                }
            }
        }
    }

    pub fn get_cursor(&self, pos: usize, side: Side) -> Option<Cursor> {
        match &self.inner {
            MaybeDetached::Detached(_) => None,
            MaybeDetached::Attached(a) => {
                let (id, len) = a.with_state(|s| {
                    let l = s.as_list_state().unwrap();
                    (l.get_id_at(pos), l.len())
                });

                if len == 0 {
                    return Some(Cursor {
                        id: None,
                        container: self.id(),
                        side: if side == Side::Middle {
                            Side::Left
                        } else {
                            side
                        },
                        origin_pos: 0,
                    });
                }

                if len <= pos {
                    return Some(Cursor {
                        id: None,
                        container: self.id(),
                        side: Side::Right,
                        origin_pos: len,
                    });
                }

                let id = id?;
                Some(Cursor {
                    id: Some(id.id()),
                    container: self.id(),
                    side,
                    origin_pos: pos,
                })
            }
        }
    }

    fn apply_delta(
        &self,
        delta: loro_delta::DeltaRope<
            loro_delta::array_vec::ArrayVec<ValueOrHandler, 8>,
            crate::event::ListDeltaMeta,
        >,
        on_container_remap: &mut dyn FnMut(ContainerID, ContainerID),
    ) -> LoroResult<()> {
        match &self.inner {
            MaybeDetached::Detached(_) => unimplemented!(),
            MaybeDetached::Attached(_) => {
                let mut index = 0;
                for item in delta.iter() {
                    match item {
                        loro_delta::DeltaItem::Retain { len, .. } => {
                            index += len;
                        }
                        loro_delta::DeltaItem::Replace { value, delete, .. } => {
                            if *delete > 0 {
                                self.delete(index, *delete)?;
                            }

                            for v in value.iter() {
                                match v {
                                    ValueOrHandler::Value(LoroValue::Container(old_id)) => {
                                        let new_h = self.insert_container(
                                            index,
                                            Handler::new_unattached(old_id.container_type()),
                                        )?;
                                        let new_id = new_h.id();
                                        on_container_remap(old_id.clone(), new_id);
                                    }
                                    ValueOrHandler::Handler(h) => {
                                        let old_id = h.id();
                                        let new_h = self.insert_container(
                                            index,
                                            Handler::new_unattached(old_id.container_type()),
                                        )?;
                                        let new_id = new_h.id();
                                        on_container_remap(old_id, new_id);
                                    }
                                    ValueOrHandler::Value(v) => {
                                        self.insert(index, v.clone())?;
                                    }
                                }

                                index += 1;
                            }
                        }
                    }
                }
            }
        }

        Ok(())
    }

    pub fn is_deleted(&self) -> bool {
        match &self.inner {
            MaybeDetached::Detached(_) => false,
            MaybeDetached::Attached(a) => a.is_deleted(),
        }
    }

    pub fn clear(&self) -> LoroResult<()> {
        match &self.inner {
            MaybeDetached::Detached(l) => {
                let mut l = l.lock().unwrap();
                l.value.clear();
                Ok(())
            }
            MaybeDetached::Attached(a) => a.with_txn(|txn| self.clear_with_txn(txn)),
        }
    }

    pub fn clear_with_txn(&self, txn: &mut Transaction) -> LoroResult<()> {
        self.delete_with_txn(txn, 0, self.len())
    }

    pub fn get_id_at(&self, pos: usize) -> Option<ID> {
        match &self.inner {
            MaybeDetached::Detached(_) => None,
            MaybeDetached::Attached(a) => a.with_state(|state| {
                state
                    .as_list_state()
                    .unwrap()
                    .get_id_at(pos)
                    .map(|x| x.id())
            }),
        }
    }
}

impl MovableListHandler {
    pub fn insert(&self, pos: usize, v: impl Into<LoroValue>) -> LoroResult<()> {
        match &self.inner {
            MaybeDetached::Detached(d) => {
                let mut d = d.lock().unwrap();
                if pos > d.value.len() {
                    return Err(LoroError::OutOfBound {
                        pos,
                        info: format!("Position: {}:{}", file!(), line!()).into_boxed_str(),
                        len: d.value.len(),
                    });
                }
                d.value.insert(pos, ValueOrHandler::Value(v.into()));
                Ok(())
            }
            MaybeDetached::Attached(a) => {
                a.with_txn(|txn| self.insert_with_txn(txn, pos, v.into()))
            }
        }
    }

    #[instrument(skip_all)]
    pub fn insert_with_txn(
        &self,
        txn: &mut Transaction,
        pos: usize,
        v: LoroValue,
    ) -> LoroResult<()> {
        if pos > self.len() {
            return Err(LoroError::OutOfBound {
                pos,
                info: format!("Position: {}:{}", file!(), line!()).into_boxed_str(),
                len: self.len(),
            });
        }

        if v.is_container() {
            return Err(LoroError::ArgErr(
                INSERT_CONTAINER_VALUE_ARG_ERROR
                    .to_string()
                    .into_boxed_str(),
            ));
        }

        let op_index = self.with_state(|state| {
            let list = state.as_movable_list_state().unwrap();
            Ok(list
                .convert_index(pos, IndexType::ForUser, IndexType::ForOp)
                .unwrap())
        })?;

        let inner = self.inner.try_attached_state()?;
        txn.apply_local_op(
            inner.container_idx,
            crate::op::RawOpContent::List(crate::container::list::list_op::ListOp::Insert {
                slice: ListSlice::RawData(Cow::Owned(vec![v.clone()])),
                pos: op_index,
            }),
            EventHint::InsertList { len: 1, pos },
            &inner.doc,
        )
    }

    #[inline]
    pub fn mov(&self, from: usize, to: usize) -> LoroResult<()> {
        match &self.inner {
            MaybeDetached::Detached(d) => {
                let mut d = d.lock().unwrap();
                if from >= d.value.len() {
                    return Err(LoroError::OutOfBound {
                        pos: from,
                        info: format!("Position: {}:{}", file!(), line!()).into_boxed_str(),
                        len: d.value.len(),
                    });
                }
                if to >= d.value.len() {
                    return Err(LoroError::OutOfBound {
                        pos: to,
                        info: format!("Position: {}:{}", file!(), line!()).into_boxed_str(),
                        len: d.value.len(),
                    });
                }
                let v = d.value.remove(from);
                d.value.insert(to, v);
                Ok(())
            }
            MaybeDetached::Attached(a) => a.with_txn(|txn| self.move_with_txn(txn, from, to)),
        }
    }

    /// Move element from `from` to `to`. After this op, elem will be at pos `to`.
    #[instrument(skip_all)]
    pub fn move_with_txn(&self, txn: &mut Transaction, from: usize, to: usize) -> LoroResult<()> {
        if from == to {
            return Ok(());
        }

        if from >= self.len() {
            return Err(LoroError::OutOfBound {
                pos: from,
                info: format!("Position: {}:{}", file!(), line!()).into_boxed_str(),
                len: self.len(),
            });
        }

        if to >= self.len() {
            return Err(LoroError::OutOfBound {
                pos: to,
                info: format!("Position: {}:{}", file!(), line!()).into_boxed_str(),
                len: self.len(),
            });
        }

        let (op_from, op_to, elem_id, value) = self.with_state(|state| {
            let list = state.as_movable_list_state().unwrap();
            let (elem_id, elem) = list
                .get_elem_at_given_pos(from, IndexType::ForUser)
                .unwrap();
            Ok((
                list.convert_index(from, IndexType::ForUser, IndexType::ForOp)
                    .unwrap(),
                list.convert_index(to, IndexType::ForUser, IndexType::ForOp)
                    .unwrap(),
                elem_id,
                elem.value().clone(),
            ))
        })?;

        let inner = self.inner.try_attached_state()?;
        txn.apply_local_op(
            inner.container_idx,
            crate::op::RawOpContent::List(crate::container::list::list_op::ListOp::Move {
                from: op_from as u32,
                to: op_to as u32,
                elem_id: elem_id.to_id(),
            }),
            EventHint::Move {
                value,
                from: from as u32,
                to: to as u32,
            },
            &inner.doc,
        )
    }

    pub fn push(&self, v: LoroValue) -> LoroResult<()> {
        match &self.inner {
            MaybeDetached::Detached(d) => {
                let mut d = d.lock().unwrap();
                d.value.push(v.into());
                Ok(())
            }
            MaybeDetached::Attached(a) => a.with_txn(|txn| self.push_with_txn(txn, v)),
        }
    }

    pub fn push_with_txn(&self, txn: &mut Transaction, v: LoroValue) -> LoroResult<()> {
        let pos = self.len();
        self.insert_with_txn(txn, pos, v)
    }

    pub fn pop_(&self) -> LoroResult<Option<ValueOrHandler>> {
        match &self.inner {
            MaybeDetached::Detached(d) => {
                let mut d = d.lock().unwrap();
                Ok(d.value.pop())
            }
            MaybeDetached::Attached(a) => {
                let last = self.len() - 1;
                let ans = self.get_(last);
                a.with_txn(|txn| self.pop_with_txn(txn))?;
                Ok(ans)
            }
        }
    }

    pub fn pop(&self) -> LoroResult<Option<LoroValue>> {
        match &self.inner {
            MaybeDetached::Detached(a) => {
                let mut a = a.lock().unwrap();
                Ok(a.value.pop().map(|x| x.to_value()))
            }
            MaybeDetached::Attached(a) => a.with_txn(|txn| self.pop_with_txn(txn)),
        }
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

    pub fn insert_container<H: HandlerTrait>(&self, pos: usize, child: H) -> LoroResult<H> {
        match &self.inner {
            MaybeDetached::Detached(d) => {
                let mut d = d.lock().unwrap();
                if pos > d.value.len() {
                    return Err(LoroError::OutOfBound {
                        pos,
                        info: format!("Position: {}:{}", file!(), line!()).into_boxed_str(),
                        len: d.value.len(),
                    });
                }
                d.value
                    .insert(pos, ValueOrHandler::Handler(child.to_handler()));
                Ok(child)
            }
            MaybeDetached::Attached(a) => {
                a.with_txn(|txn| self.insert_container_with_txn(txn, pos, child))
            }
        }
    }

    pub fn push_container<H: HandlerTrait>(&self, child: H) -> LoroResult<H> {
        self.insert_container(self.len(), child)
    }

    pub fn insert_container_with_txn<H: HandlerTrait>(
        &self,
        txn: &mut Transaction,
        pos: usize,
        child: H,
    ) -> LoroResult<H> {
        if pos > self.len() {
            return Err(LoroError::OutOfBound {
                pos,
                info: format!("Position: {}:{}", file!(), line!()).into_boxed_str(),
                len: self.len(),
            });
        }

        let op_index = self.with_state(|state| {
            let list = state.as_movable_list_state().unwrap();
            Ok(list
                .convert_index(pos, IndexType::ForUser, IndexType::ForOp)
                .unwrap())
        })?;

        let id = txn.next_id();
        let container_id = ContainerID::new_normal(id, child.kind());
        let v = LoroValue::Container(container_id.clone());
        let inner = self.inner.try_attached_state()?;
        txn.apply_local_op(
            inner.container_idx,
            crate::op::RawOpContent::List(crate::container::list::list_op::ListOp::Insert {
                slice: ListSlice::RawData(Cow::Owned(vec![v.clone()])),
                pos: op_index,
            }),
            EventHint::InsertList { len: 1, pos },
            &inner.doc,
        )?;
        child.attach(txn, inner, container_id)
    }

    pub fn set(&self, index: usize, value: impl Into<LoroValue>) -> LoroResult<()> {
        match &self.inner {
            MaybeDetached::Detached(d) => {
                let mut d = d.lock().unwrap();
                if index >= d.value.len() {
                    return Err(LoroError::OutOfBound {
                        pos: index,
                        info: format!("Position: {}:{}", file!(), line!()).into_boxed_str(),
                        len: d.value.len(),
                    });
                }
                d.value[index] = ValueOrHandler::Value(value.into());
                Ok(())
            }
            MaybeDetached::Attached(a) => {
                a.with_txn(|txn| self.set_with_txn(txn, index, value.into()))
            }
        }
    }

    pub fn set_with_txn(
        &self,
        txn: &mut Transaction,
        index: usize,
        value: LoroValue,
    ) -> LoroResult<()> {
        if index >= self.len() {
            return Err(LoroError::OutOfBound {
                pos: index,
                info: format!("Position: {}:{}", file!(), line!()).into_boxed_str(),
                len: self.len(),
            });
        }

        let inner = self.inner.try_attached_state()?;
        let Some(elem_id) = self.with_state(|state| {
            let list = state.as_movable_list_state().unwrap();
            Ok(list.get_elem_id_at(index, IndexType::ForUser))
        })?
        else {
            unreachable!()
        };

        let op = crate::op::RawOpContent::List(crate::container::list::list_op::ListOp::Set {
            elem_id: elem_id.to_id(),
            value: value.clone(),
        });

        let hint = EventHint::SetList { index, value };
        txn.apply_local_op(inner.container_idx, op, hint, &inner.doc)
    }

    pub fn set_container<H: HandlerTrait>(&self, pos: usize, child: H) -> LoroResult<H> {
        match &self.inner {
            MaybeDetached::Detached(d) => {
                let mut d = d.lock().unwrap();
                d.value[pos] = ValueOrHandler::Handler(child.to_handler());
                Ok(child)
            }
            MaybeDetached::Attached(a) => {
                a.with_txn(|txn| self.set_container_with_txn(txn, pos, child))
            }
        }
    }

    pub fn set_container_with_txn<H: HandlerTrait>(
        &self,
        txn: &mut Transaction,
        pos: usize,
        child: H,
    ) -> LoroResult<H> {
        let id = txn.next_id();
        let container_id = ContainerID::new_normal(id, child.kind());
        let v = LoroValue::Container(container_id.clone());
        let Some(elem_id) = self.with_state(|state| {
            let list = state.as_movable_list_state().unwrap();
            Ok(list.get_elem_id_at(pos, IndexType::ForUser))
        })?
        else {
            let len = self.len();
            if pos >= len {
                return Err(LoroError::OutOfBound {
                    pos,
                    len,
                    info: "".into(),
                });
            } else {
                unreachable!()
            }
        };
        let inner = self.inner.try_attached_state()?;
        txn.apply_local_op(
            inner.container_idx,
            crate::op::RawOpContent::List(crate::container::list::list_op::ListOp::Set {
                elem_id: elem_id.to_id(),
                value: v.clone(),
            }),
            EventHint::SetList {
                index: pos,
                value: v,
            },
            &inner.doc,
        )?;

        child.attach(txn, inner, container_id)
    }

    pub fn delete(&self, pos: usize, len: usize) -> LoroResult<()> {
        match &self.inner {
            MaybeDetached::Detached(d) => {
                let mut d = d.lock().unwrap();
                d.value.drain(pos..pos + len);
                Ok(())
            }
            MaybeDetached::Attached(a) => a.with_txn(|txn| self.delete_with_txn(txn, pos, len)),
        }
    }

    #[instrument(skip_all)]
    pub fn delete_with_txn(&self, txn: &mut Transaction, pos: usize, len: usize) -> LoroResult<()> {
        if len == 0 {
            return Ok(());
        }

        if pos + len > self.len() {
            return Err(LoroError::OutOfBound {
                pos: pos + len,
                info: format!("Position: {}:{}", file!(), line!()).into_boxed_str(),
                len: self.len(),
            });
        }

        let (ids, new_poses) = self.with_state(|state| {
            let list = state.as_movable_list_state().unwrap();
            let ids: Vec<_> = (pos..pos + len)
                .map(|i| list.get_list_id_at(i, IndexType::ForUser).unwrap())
                .collect();
            let poses: Vec<_> = (pos..pos + len)
                // need to -i because we delete the previous ones
                .map(|user_index| {
                    let op_index = list
                        .convert_index(user_index, IndexType::ForUser, IndexType::ForOp)
                        .unwrap();
                    assert!(op_index >= user_index);
                    op_index - (user_index - pos)
                })
                .collect();
            Ok((ids, poses))
        })?;

        loro_common::info!(?pos, ?len, ?ids, ?new_poses, "delete_with_txn");
        let user_pos = pos;
        let inner = self.inner.try_attached_state()?;
        for (id, op_pos) in ids.into_iter().zip(new_poses.into_iter()) {
            txn.apply_local_op(
                inner.container_idx,
                crate::op::RawOpContent::List(ListOp::Delete(DeleteSpanWithId::new(
                    id,
                    op_pos as isize,
                    1,
                ))),
                EventHint::DeleteList(DeleteSpan::new(user_pos as isize, 1)),
                &inner.doc,
            )?;
        }

        Ok(())
    }

    pub fn get_child_handler(&self, index: usize) -> LoroResult<Handler> {
        match &self.inner {
            MaybeDetached::Detached(l) => {
                let list = l.lock().unwrap();
                let value = list.value.get(index).ok_or(LoroError::OutOfBound {
                    pos: index,
                    info: format!("Position: {}:{}", file!(), line!()).into_boxed_str(),
                    len: list.value.len(),
                })?;
                match value {
                    ValueOrHandler::Handler(h) => Ok(h.clone()),
                    _ => Err(LoroError::ArgErr(
                        format!(
                            "Expected container at index {}, but found {:?}",
                            index, value
                        )
                        .into_boxed_str(),
                    )),
                }
            }
            MaybeDetached::Attached(a) => {
                let Some(value) = a.with_state(|state| {
                    state
                        .as_movable_list_state()
                        .as_ref()
                        .unwrap()
                        .get(index, IndexType::ForUser)
                        .cloned()
                }) else {
                    return Err(LoroError::OutOfBound {
                        pos: index,
                        info: format!("Position: {}:{}", file!(), line!()).into_boxed_str(),
                        len: a.with_state(|state| state.as_list_state().unwrap().len()),
                    });
                };
                match value {
                    LoroValue::Container(id) => Ok(create_handler(a, id)),
                    _ => Err(LoroError::ArgErr(
                        format!(
                            "Expected container at index {}, but found {:?}",
                            index, value
                        )
                        .into_boxed_str(),
                    )),
                }
            }
        }
    }

    pub fn len(&self) -> usize {
        match &self.inner {
            MaybeDetached::Detached(d) => {
                let d = d.lock().unwrap();
                d.value.len()
            }
            MaybeDetached::Attached(a) => {
                a.with_state(|state| state.as_movable_list_state().unwrap().len())
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn get_deep_value_with_id(&self) -> LoroValue {
        let inner = self.inner.try_attached_state().unwrap();
        inner
            .doc
            .state
            .lock()
            .unwrap()
            .get_container_deep_value_with_id(inner.container_idx, None)
    }

    pub fn get(&self, index: usize) -> Option<LoroValue> {
        match &self.inner {
            MaybeDetached::Detached(d) => {
                let d = d.lock().unwrap();
                d.value.get(index).map(|v| v.to_value())
            }
            MaybeDetached::Attached(a) => a.with_state(|state| {
                let a = state.as_movable_list_state().unwrap();
                a.get(index, IndexType::ForUser).cloned()
            }),
        }
    }

    /// Get value at given index, if it's a container, return a handler to the container
    pub fn get_(&self, index: usize) -> Option<ValueOrHandler> {
        match &self.inner {
            MaybeDetached::Detached(d) => {
                let d = d.lock().unwrap();
                d.value.get(index).cloned()
            }
            MaybeDetached::Attached(m) => m.with_state(|state| {
                let a = state.as_movable_list_state().unwrap();
                match a.get(index, IndexType::ForUser) {
                    Some(v) => {
                        if let LoroValue::Container(c) = v {
                            Some(ValueOrHandler::Handler(create_handler(m, c.clone())))
                        } else {
                            Some(ValueOrHandler::Value(v.clone()))
                        }
                    }
                    None => None,
                }
            }),
        }
    }

    pub fn for_each<I>(&self, mut f: I)
    where
        I: FnMut(ValueOrHandler),
    {
        match &self.inner {
            MaybeDetached::Detached(d) => {
                let d = d.lock().unwrap();
                for v in d.value.iter() {
                    f(v.clone());
                }
            }
            MaybeDetached::Attached(m) => {
                let mut temp = vec![];
                m.with_state(|state| {
                    let a = state.as_movable_list_state().unwrap();
                    for v in a.iter() {
                        match v {
                            LoroValue::Container(c) => {
                                temp.push(ValueOrHandler::Handler(create_handler(m, c.clone())));
                            }
                            value => {
                                temp.push(ValueOrHandler::Value(value.clone()));
                            }
                        }
                    }
                });

                for v in temp.into_iter() {
                    f(v);
                }
            }
        }
    }

    pub fn log_internal_state(&self) -> String {
        match &self.inner {
            MaybeDetached::Detached(d) => {
                let d = d.lock().unwrap();
                format!("{:#?}", &d.value)
            }
            MaybeDetached::Attached(a) => a.with_state(|state| {
                let a = state.as_movable_list_state().unwrap();
                format!("{a:#?}")
            }),
        }
    }

    pub fn new_detached() -> MovableListHandler {
        MovableListHandler {
            inner: MaybeDetached::new_detached(Default::default()),
        }
    }

    pub fn get_cursor(&self, pos: usize, side: Side) -> Option<Cursor> {
        match &self.inner {
            MaybeDetached::Detached(_) => None,
            MaybeDetached::Attached(inner) => {
                let (id, len) = inner.with_state(|s| {
                    let l = s.as_movable_list_state().unwrap();
                    (l.get_list_item_id_at(pos), l.len())
                });

                if len == 0 {
                    return Some(Cursor {
                        id: None,
                        container: self.id(),
                        side: if side == Side::Middle {
                            Side::Left
                        } else {
                            side
                        },
                        origin_pos: 0,
                    });
                }

                if len <= pos {
                    return Some(Cursor {
                        id: None,
                        container: self.id(),
                        side: Side::Right,
                        origin_pos: len,
                    });
                }

                let id = id?;
                Some(Cursor {
                    id: Some(id.id()),
                    container: self.id(),
                    side,
                    origin_pos: pos,
                })
            }
        }
    }

    pub(crate) fn op_pos_to_user_pos(&self, new_pos: usize) -> usize {
        match &self.inner {
            MaybeDetached::Detached(_) => new_pos,
            MaybeDetached::Attached(inner) => {
                let mut pos = new_pos;
                inner.with_state(|s| {
                    let l = s.as_movable_list_state().unwrap();
                    pos = l
                        .convert_index(new_pos, IndexType::ForOp, IndexType::ForUser)
                        .unwrap_or(l.len());
                });
                pos
            }
        }
    }

    pub fn is_deleted(&self) -> bool {
        match &self.inner {
            MaybeDetached::Detached(_) => false,
            MaybeDetached::Attached(a) => a.is_deleted(),
        }
    }

    pub fn clear(&self) -> LoroResult<()> {
        match &self.inner {
            MaybeDetached::Detached(d) => {
                let mut d = d.lock().unwrap();
                d.value.clear();
                Ok(())
            }
            MaybeDetached::Attached(a) => a.with_txn(|txn| self.clear_with_txn(txn)),
        }
    }

    pub fn clear_with_txn(&self, txn: &mut Transaction) -> LoroResult<()> {
        self.delete_with_txn(txn, 0, self.len())
    }

    pub fn get_creator_at(&self, pos: usize) -> Option<PeerID> {
        match &self.inner {
            MaybeDetached::Detached(_) => None,
            MaybeDetached::Attached(a) => {
                a.with_state(|state| state.as_movable_list_state().unwrap().get_creator_at(pos))
            }
        }
    }

    pub fn get_last_mover_at(&self, pos: usize) -> Option<PeerID> {
        match &self.inner {
            MaybeDetached::Detached(_) => None,
            MaybeDetached::Attached(a) => a.with_state(|state| {
                state
                    .as_movable_list_state()
                    .unwrap()
                    .get_last_mover_at(pos)
            }),
        }
    }

    pub fn get_last_editor_at(&self, pos: usize) -> Option<PeerID> {
        match &self.inner {
            MaybeDetached::Detached(_) => None,
            MaybeDetached::Attached(a) => a.with_state(|state| {
                state
                    .as_movable_list_state()
                    .unwrap()
                    .get_last_editor_at(pos)
            }),
        }
    }
}

impl MapHandler {
    /// Create a new container that is detached from the document.
    /// The edits on a detached container will not be persisted.
    /// To attach the container to the document, please insert it into an attached container.
    pub fn new_detached() -> Self {
        Self {
            inner: MaybeDetached::new_detached(Default::default()),
        }
    }

    pub fn insert(&self, key: &str, value: impl Into<LoroValue>) -> LoroResult<()> {
        match &self.inner {
            MaybeDetached::Detached(m) => {
                let mut m = m.lock().unwrap();
                m.value
                    .insert(key.into(), ValueOrHandler::Value(value.into()));
                Ok(())
            }
            MaybeDetached::Attached(a) => {
                a.with_txn(|txn| self.insert_with_txn(txn, key, value.into()))
            }
        }
    }

    /// This method will insert the value even if the same value is already in the given entry.
    fn insert_without_skipping(&self, key: &str, value: impl Into<LoroValue>) -> LoroResult<()> {
        match &self.inner {
            MaybeDetached::Detached(m) => {
                let mut m = m.lock().unwrap();
                m.value
                    .insert(key.into(), ValueOrHandler::Value(value.into()));
                Ok(())
            }
            MaybeDetached::Attached(a) => a.with_txn(|txn| {
                let this = &self;
                let value = value.into();
                if let Some(_value) = value.as_container() {
                    return Err(LoroError::ArgErr(
                        INSERT_CONTAINER_VALUE_ARG_ERROR
                            .to_string()
                            .into_boxed_str(),
                    ));
                }

                let inner = this.inner.try_attached_state()?;
                txn.apply_local_op(
                    inner.container_idx,
                    crate::op::RawOpContent::Map(crate::container::map::MapSet {
                        key: key.into(),
                        value: Some(value.clone()),
                    }),
                    EventHint::Map {
                        key: key.into(),
                        value: Some(value.clone()),
                    },
                    &inner.doc,
                )
            }),
        }
    }

    pub fn insert_with_txn(
        &self,
        txn: &mut Transaction,
        key: &str,
        value: LoroValue,
    ) -> LoroResult<()> {
        if let Some(_value) = value.as_container() {
            return Err(LoroError::ArgErr(
                INSERT_CONTAINER_VALUE_ARG_ERROR
                    .to_string()
                    .into_boxed_str(),
            ));
        }

        if self.get(key).map(|x| x == value).unwrap_or(false) {
            // skip if the value is already set
            return Ok(());
        }

        let inner = self.inner.try_attached_state()?;
        txn.apply_local_op(
            inner.container_idx,
            crate::op::RawOpContent::Map(crate::container::map::MapSet {
                key: key.into(),
                value: Some(value.clone()),
            }),
            EventHint::Map {
                key: key.into(),
                value: Some(value.clone()),
            },
            &inner.doc,
        )
    }

    pub fn insert_container<T: HandlerTrait>(&self, key: &str, handler: T) -> LoroResult<T> {
        match &self.inner {
            MaybeDetached::Detached(m) => {
                let mut m = m.lock().unwrap();
                let to_insert = handler.to_handler();
                m.value
                    .insert(key.into(), ValueOrHandler::Handler(to_insert.clone()));
                Ok(handler)
            }
            MaybeDetached::Attached(a) => {
                a.with_txn(|txn| self.insert_container_with_txn(txn, key, handler))
            }
        }
    }

    pub fn insert_container_with_txn<H: HandlerTrait>(
        &self,
        txn: &mut Transaction,
        key: &str,
        child: H,
    ) -> LoroResult<H> {
        let inner = self.inner.try_attached_state()?;
        let id = txn.next_id();
        let container_id = ContainerID::new_normal(id, child.kind());
        txn.apply_local_op(
            inner.container_idx,
            crate::op::RawOpContent::Map(crate::container::map::MapSet {
                key: key.into(),
                value: Some(LoroValue::Container(container_id.clone())),
            }),
            EventHint::Map {
                key: key.into(),
                value: Some(LoroValue::Container(container_id.clone())),
            },
            &inner.doc,
        )?;

        child.attach(txn, inner, container_id)
    }

    pub fn delete(&self, key: &str) -> LoroResult<()> {
        match &self.inner {
            MaybeDetached::Detached(m) => {
                let mut m = m.lock().unwrap();
                m.value.remove(key);
                Ok(())
            }
            MaybeDetached::Attached(a) => a.with_txn(|txn| self.delete_with_txn(txn, key)),
        }
    }

    pub fn delete_with_txn(&self, txn: &mut Transaction, key: &str) -> LoroResult<()> {
        let inner = self.inner.try_attached_state()?;
        txn.apply_local_op(
            inner.container_idx,
            crate::op::RawOpContent::Map(crate::container::map::MapSet {
                key: key.into(),
                value: None,
            }),
            EventHint::Map {
                key: key.into(),
                value: None,
            },
            &inner.doc,
        )
    }

    pub fn for_each<I>(&self, mut f: I)
    where
        I: FnMut(&str, ValueOrHandler),
    {
        match &self.inner {
            MaybeDetached::Detached(m) => {
                let m = m.lock().unwrap();
                for (k, v) in m.value.iter() {
                    f(k, v.clone());
                }
            }
            MaybeDetached::Attached(inner) => {
                let mut temp = vec![];
                inner.with_state(|state| {
                    let a = state.as_map_state().unwrap();
                    for (k, v) in a.iter() {
                        if let Some(v) = &v.value {
                            match v {
                                LoroValue::Container(c) => {
                                    temp.push((
                                        k.to_string(),
                                        ValueOrHandler::Handler(create_handler(inner, c.clone())),
                                    ));
                                }
                                value => {
                                    temp.push((k.to_string(), ValueOrHandler::Value(value.clone())))
                                }
                            }
                        }
                    }
                });

                for (k, v) in temp.into_iter() {
                    f(&k, v.clone());
                }
            }
        }
    }

    pub fn get_child_handler(&self, key: &str) -> LoroResult<Handler> {
        match &self.inner {
            MaybeDetached::Detached(m) => {
                let m = m.lock().unwrap();
                let value = m.value.get(key).unwrap();
                match value {
                    ValueOrHandler::Value(v) => Err(LoroError::ArgErr(
                        format!("Expected Handler but found {:?}", v).into_boxed_str(),
                    )),
                    ValueOrHandler::Handler(h) => Ok(h.clone()),
                }
            }
            MaybeDetached::Attached(inner) => {
                let container_id = inner.with_state(|state| {
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
                Ok(create_handler(inner, container_id))
            }
        }
    }

    pub fn get_deep_value_with_id(&self) -> LoroResult<LoroValue> {
        match &self.inner {
            MaybeDetached::Detached(_) => Err(LoroError::MisuseDetachedContainer {
                method: "get_deep_value_with_id",
            }),
            MaybeDetached::Attached(inner) => Ok(inner.with_doc_state(|state| {
                state.get_container_deep_value_with_id(inner.container_idx, None)
            })),
        }
    }

    pub fn get(&self, key: &str) -> Option<LoroValue> {
        match &self.inner {
            MaybeDetached::Detached(m) => {
                let m = m.lock().unwrap();
                m.value.get(key).map(|v| v.to_value())
            }
            MaybeDetached::Attached(inner) => {
                inner.with_state(|state| state.as_map_state().unwrap().get(key).cloned())
            }
        }
    }

    /// Get the value at given key, if value is a container, return a handler to the container
    pub fn get_(&self, key: &str) -> Option<ValueOrHandler> {
        match &self.inner {
            MaybeDetached::Detached(m) => {
                let m = m.lock().unwrap();
                m.value.get(key).cloned()
            }
            MaybeDetached::Attached(inner) => {
                let value =
                    inner.with_state(|state| state.as_map_state().unwrap().get(key).cloned());
                match value {
                    Some(LoroValue::Container(container_id)) => Some(ValueOrHandler::Handler(
                        create_handler(inner, container_id.clone()),
                    )),
                    Some(value) => Some(ValueOrHandler::Value(value.clone())),
                    None => None,
                }
            }
        }
    }

    pub fn get_or_create_container<C: HandlerTrait>(&self, key: &str, child: C) -> LoroResult<C> {
        if let Some(ans) = self.get_(key) {
            if let ValueOrHandler::Handler(h) = ans {
                let kind = h.kind();
                return C::from_handler(h).ok_or_else(move || {
                    LoroError::ArgErr(
                        format!("Expected value type {} but found {:?}", child.kind(), kind)
                            .into_boxed_str(),
                    )
                });
            } else if let ValueOrHandler::Value(LoroValue::Null) = ans {
                // do nothing
            } else {
                return Err(LoroError::ArgErr(
                    format!("Expected value type {} but found {:?}", child.kind(), ans)
                        .into_boxed_str(),
                ));
            }
        }

        self.insert_container(key, child)
    }

    pub fn contains_key(&self, key: &str) -> bool {
        self.get(key).is_some()
    }

    pub fn len(&self) -> usize {
        match &self.inner {
            MaybeDetached::Detached(m) => m.lock().unwrap().value.len(),
            MaybeDetached::Attached(a) => a.with_state(|state| state.as_map_state().unwrap().len()),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn is_deleted(&self) -> bool {
        match &self.inner {
            MaybeDetached::Detached(_) => false,
            MaybeDetached::Attached(a) => a.is_deleted(),
        }
    }

    pub fn clear(&self) -> LoroResult<()> {
        match &self.inner {
            MaybeDetached::Detached(m) => {
                let mut m = m.lock().unwrap();
                m.value.clear();
                Ok(())
            }
            MaybeDetached::Attached(a) => a.with_txn(|txn| self.clear_with_txn(txn)),
        }
    }

    pub fn clear_with_txn(&self, txn: &mut Transaction) -> LoroResult<()> {
        let keys: Vec<InternalString> = self.inner.try_attached_state()?.with_state(|state| {
            state
                .as_map_state()
                .unwrap()
                .iter()
                .map(|(k, _)| k.clone())
                .collect()
        });

        for key in keys {
            self.delete_with_txn(txn, &key)?;
        }

        Ok(())
    }

    pub fn keys(&self) -> impl Iterator<Item = InternalString> + '_ {
        let mut keys: Vec<InternalString> = Vec::with_capacity(self.len());
        match &self.inner {
            MaybeDetached::Detached(m) => {
                let m = m.lock().unwrap();
                keys = m.value.keys().map(|x| x.as_str().into()).collect();
            }
            MaybeDetached::Attached(a) => {
                a.with_state(|state| {
                    for (k, v) in state.as_map_state().unwrap().iter() {
                        if v.value.is_some() {
                            keys.push(k.clone());
                        }
                    }
                });
            }
        }

        keys.into_iter()
    }

    pub fn values(&self) -> impl Iterator<Item = ValueOrHandler> + '_ {
        let mut values: Vec<ValueOrHandler> = Vec::with_capacity(self.len());
        match &self.inner {
            MaybeDetached::Detached(m) => {
                let m = m.lock().unwrap();
                values = m.value.values().cloned().collect();
            }
            MaybeDetached::Attached(a) => {
                a.with_state(|state| {
                    for (_, v) in state.as_map_state().unwrap().iter() {
                        let value = match &v.value {
                            Some(LoroValue::Container(container_id)) => {
                                ValueOrHandler::Handler(create_handler(a, container_id.clone()))
                            }
                            Some(value) => ValueOrHandler::Value(value.clone()),
                            None => continue,
                        };
                        values.push(value);
                    }
                });
            }
        }

        values.into_iter()
    }

    pub fn get_last_editor(&self, key: &str) -> Option<PeerID> {
        match &self.inner {
            MaybeDetached::Detached(_) => None,
            MaybeDetached::Attached(a) => a.with_state(|state| {
                let m = state.as_map_state().unwrap();
                m.get_last_edit_peer(key)
            }),
        }
    }
}

fn with_txn<R>(doc: &LoroDoc, f: impl FnOnce(&mut Transaction) -> LoroResult<R>) -> LoroResult<R> {
    let txn = &doc.txn;
    let mut txn = txn.lock().unwrap();
    loop {
        if let Some(txn) = &mut *txn {
            return f(txn);
        } else if cfg!(target_arch = "wasm32") || !doc.can_edit() {
            return Err(LoroError::AutoCommitNotStarted);
        } else {
            drop(txn);
            #[cfg(loom)]
            loom::thread::yield_now();
            doc.start_auto_commit();
            txn = doc.txn.lock().unwrap();
        }
    }
}

#[cfg(feature = "counter")]
pub mod counter {

    use loro_common::LoroResult;

    use crate::{
        txn::{EventHint, Transaction},
        HandlerTrait,
    };

    use super::{create_handler, Handler, MaybeDetached};

    #[derive(Clone)]
    pub struct CounterHandler {
        pub(super) inner: MaybeDetached<f64>,
    }

    impl CounterHandler {
        pub fn new_detached() -> Self {
            Self {
                inner: MaybeDetached::new_detached(0.),
            }
        }

        pub fn increment(&self, n: f64) -> LoroResult<()> {
            match &self.inner {
                MaybeDetached::Detached(d) => {
                    let d = &mut d.lock().unwrap().value;
                    *d += n;
                    Ok(())
                }
                MaybeDetached::Attached(a) => a.with_txn(|txn| self.increment_with_txn(txn, n)),
            }
        }

        pub fn decrement(&self, n: f64) -> LoroResult<()> {
            match &self.inner {
                MaybeDetached::Detached(d) => {
                    let d = &mut d.lock().unwrap().value;
                    *d -= n;
                    Ok(())
                }
                MaybeDetached::Attached(a) => a.with_txn(|txn| self.increment_with_txn(txn, -n)),
            }
        }

        fn increment_with_txn(&self, txn: &mut Transaction, n: f64) -> LoroResult<()> {
            let inner = self.inner.try_attached_state()?;
            txn.apply_local_op(
                inner.container_idx,
                crate::op::RawOpContent::Counter(n),
                EventHint::Counter(n),
                &inner.doc,
            )
        }

        pub fn is_deleted(&self) -> bool {
            match &self.inner {
                MaybeDetached::Detached(_) => false,
                MaybeDetached::Attached(a) => a.is_deleted(),
            }
        }

        pub fn clear(&self) -> LoroResult<()> {
            self.decrement(self.get_value().into_double().unwrap())
        }
    }

    impl std::fmt::Debug for CounterHandler {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match &self.inner {
                MaybeDetached::Detached(_) => write!(f, "CounterHandler Detached"),
                MaybeDetached::Attached(a) => write!(f, "CounterHandler {}", a.id),
            }
        }
    }

    impl HandlerTrait for CounterHandler {
        fn is_attached(&self) -> bool {
            matches!(&self.inner, MaybeDetached::Attached(..))
        }

        fn attached_handler(&self) -> Option<&crate::BasicHandler> {
            self.inner.attached_handler()
        }

        fn get_value(&self) -> loro_common::LoroValue {
            match &self.inner {
                MaybeDetached::Detached(t) => {
                    let t = t.lock().unwrap();
                    t.value.into()
                }
                MaybeDetached::Attached(a) => a.get_value(),
            }
        }

        fn get_deep_value(&self) -> loro_common::LoroValue {
            self.get_value()
        }

        fn kind(&self) -> loro_common::ContainerType {
            loro_common::ContainerType::Counter
        }

        fn to_handler(&self) -> super::Handler {
            Handler::Counter(self.clone())
        }

        fn from_handler(h: super::Handler) -> Option<Self> {
            match h {
                Handler::Counter(x) => Some(x),
                _ => None,
            }
        }

        fn attach(
            &self,
            txn: &mut crate::txn::Transaction,
            parent: &crate::BasicHandler,
            self_id: loro_common::ContainerID,
        ) -> loro_common::LoroResult<Self> {
            match &self.inner {
                MaybeDetached::Detached(v) => {
                    let mut v = v.lock().unwrap();
                    let inner = create_handler(parent, self_id);
                    let c = inner.into_counter().unwrap();

                    c.increment_with_txn(txn, v.value)?;

                    v.attached = c.attached_handler().cloned();
                    Ok(c)
                }
                MaybeDetached::Attached(a) => {
                    let new_inner = create_handler(a, self_id);
                    let ans = new_inner.into_counter().unwrap();
                    let delta = *self.get_value().as_double().unwrap();
                    ans.increment_with_txn(txn, delta)?;
                    Ok(ans)
                }
            }
        }

        fn get_attached(&self) -> Option<Self> {
            match &self.inner {
                MaybeDetached::Attached(a) => Some(Self {
                    inner: MaybeDetached::Attached(a.clone()),
                }),
                MaybeDetached::Detached(_) => None,
            }
        }

        fn doc(&self) -> Option<crate::LoroDoc> {
            match &self.inner {
                MaybeDetached::Detached(_) => None,
                MaybeDetached::Attached(a) => Some(a.doc()),
            }
        }
    }
}

#[cfg(test)]
mod test {

    use super::{HandlerTrait, TextDelta};
    use crate::cursor::PosType;
    use crate::loro::ExportMode;
    use crate::state::TreeParentId;
    use crate::version::Frontiers;
    use crate::LoroDoc;
    use crate::{fx_map, ToJson};
    use loro_common::ID;
    use serde_json::json;

    #[test]
    fn richtext_handler() {
        let loro = LoroDoc::new();
        loro.set_peer_id(1).unwrap();
        let loro2 = LoroDoc::new();
        loro2.set_peer_id(2).unwrap();

        let mut txn = loro.txn().unwrap();
        let text = txn.get_text("hello");
        text.insert_with_txn(&mut txn, 0, "hello", PosType::Unicode)
            .unwrap();
        txn.commit().unwrap();
        let exported = loro.export(ExportMode::all_updates()).unwrap();

        loro2.import(&exported).unwrap();
        let mut txn = loro2.txn().unwrap();
        let text = txn.get_text("hello");
        assert_eq!(&**text.get_value().as_string().unwrap(), "hello");
        text.insert_with_txn(&mut txn, 5, " world", PosType::Unicode)
            .unwrap();
        assert_eq!(&**text.get_value().as_string().unwrap(), "hello world");
        txn.commit().unwrap();

        loro.import(&loro2.export(ExportMode::all_updates()).unwrap())
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
        handler
            .insert_with_txn(&mut txn, 0, "hello", PosType::Unicode)
            .unwrap();
        txn.commit().unwrap();
        for i in 0..100 {
            let new_loro = LoroDoc::new();
            new_loro
                .import(&loro.export(ExportMode::all_updates()).unwrap())
                .unwrap();
            let mut txn = new_loro.txn().unwrap();
            let handler = new_loro.get_text("richtext");
            handler
                .insert_with_txn(&mut txn, i % 5, &i.to_string(), PosType::Unicode)
                .unwrap();
            txn.commit().unwrap();
            loro.import(
                &new_loro
                    .export(ExportMode::updates(&loro.oplog_vv()))
                    .unwrap(),
            )
            .unwrap();
        }
    }

    #[test]
    fn richtext_handler_mark() {
        let loro = LoroDoc::new_auto_commit();
        let handler = loro.get_text("richtext");
        handler.insert(0, "hello world", PosType::Unicode).unwrap();
        handler
            .mark(0, 5, "bold", true.into(), PosType::Event)
            .unwrap();
        loro.commit_then_renew();

        // assert has bold
        let value = handler.get_richtext_value();
        assert_eq!(value[0]["insert"], "hello".into());
        let meta = value[0]["attributes"].as_map().unwrap();
        assert_eq!(meta.len(), 1);
        meta.get("bold").unwrap();

        let loro2 = LoroDoc::new_auto_commit();
        loro2
            .import(&loro.export(ExportMode::all_updates()).unwrap())
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
            handler2.insert(5, " new", PosType::Unicode).unwrap();
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
        handler
            .insert_with_txn(&mut txn, 0, "hello world", PosType::Unicode)
            .unwrap();
        handler
            .mark_with_txn(&mut txn, 0, 5, "bold", true.into(), PosType::Event, false)
            .unwrap();
        txn.commit().unwrap();

        let loro2 = LoroDoc::new();
        loro2
            .import(&loro.export(ExportMode::snapshot()).unwrap())
            .unwrap();
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
        let loro = LoroDoc::new_auto_commit();
        loro.set_peer_id(1).unwrap();
        let tree = loro.get_tree("root");
        let id = tree.create(TreeParentId::Root).unwrap();
        let meta = tree.get_meta(id).unwrap();
        meta.insert("a", 123).unwrap();
        loro.commit_then_renew();
        let meta = tree.get_meta(id).unwrap();
        assert_eq!(meta.get("a").unwrap(), 123.into());
        assert_eq!(
            json!([{"parent":null,"meta":{"a":123},"id":"0@1","index":0,"children":[],"fractional_index":"80"}]),
            tree.get_deep_value().to_json_value()
        );
        let bytes = loro.export(ExportMode::snapshot()).unwrap();
        let loro2 = LoroDoc::new();
        loro2.import(&bytes).unwrap();
    }

    #[test]
    fn tree_meta_event() {
        use std::sync::Arc;
        let loro = LoroDoc::new_auto_commit();
        let tree = loro.get_tree("root");
        let text = loro.get_text("text");

        let id = tree.create(TreeParentId::Root).unwrap();
        let meta = tree.get_meta(id).unwrap();
        meta.insert("a", 1).unwrap();
        text.insert(0, "abc", PosType::Unicode).unwrap();
        let _id2 = tree.create(TreeParentId::Root).unwrap();
        meta.insert("b", 2).unwrap();

        let loro2 = LoroDoc::new_auto_commit();
        let _g = loro2.subscribe_root(Arc::new(|e| {
            println!("{} {:?} ", e.event_meta.by, e.event_meta.diff)
        }));
        loro2
            .import(&loro.export(ExportMode::all_updates()).unwrap())
            .unwrap();
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

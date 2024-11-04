use super::{state::DocState, txn::Transaction};
use crate::{
    arena::SharedArena,
    container::{
        idx::ContainerIdx,
        list::list_op::{DeleteSpan, DeleteSpanWithId, ListOp},
        richtext::{richtext_state::PosType, RichtextState, StyleOp, TextStyleInfoFlag},
    },
    cursor::{Cursor, Side},
    delta::{DeltaItem, Meta, StyleMeta, TreeExternalDiff},
    diff::{myers_diff, OperateProxy},
    event::{Diff, TextDiffItem},
    op::ListSlice,
    state::{IndexType, State, TreeParentId},
    txn::EventHint,
    utils::{string_slice::StringSlice, utf16::count_utf16_len},
};
use append_only_bytes::BytesSlice;
use enum_as_inner::EnumAsInner;
use fxhash::FxHashMap;
use generic_btree::rle::HasLength;
use loro_common::{
    ContainerID, ContainerType, IdFull, InternalString, LoroError, LoroResult, LoroValue, TreeID,
    ID,
};
use serde::{Deserialize, Serialize};
use std::{
    borrow::Cow,
    cmp::Reverse,
    collections::BinaryHeap,
    fmt::Debug,
    ops::Deref,
    sync::{Arc, Mutex, Weak},
};
use tracing::{error, info, instrument, trace};

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
        let state = inner.state.upgrade().unwrap();
        let mut guard = state.try_lock().unwrap();
        guard.with_state_mut(inner.container_idx, f)
    }
}

fn create_handler(inner: &BasicHandler, id: ContainerID) -> Handler {
    Handler::new_attached(
        id,
        inner.arena.clone(),
        inner.txn.clone(),
        inner.state.clone(),
    )
}

/// Flatten attributes that allow overlap
#[derive(Clone, Debug)]
pub struct BasicHandler {
    id: ContainerID,
    arena: SharedArena,
    container_idx: ContainerIdx,
    txn: Weak<Mutex<Option<Transaction>>>,
    state: Weak<Mutex<DocState>>,
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
    #[inline]
    fn with_doc_state<R>(&self, f: impl FnOnce(&mut DocState) -> R) -> R {
        let state = self.state.upgrade().unwrap();
        let mut guard = state.try_lock().unwrap();
        f(&mut guard)
    }

    fn with_txn<R>(
        &self,
        f: impl FnOnce(&mut Transaction) -> Result<R, LoroError>,
    ) -> Result<R, LoroError> {
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
        self.state
            .upgrade()
            .unwrap()
            .try_lock()
            .unwrap()
            .get_value_by_idx(self.container_idx)
    }

    pub fn get_deep_value(&self) -> LoroValue {
        self.state
            .upgrade()
            .unwrap()
            .try_lock()
            .unwrap()
            .get_container_deep_value(self.container_idx)
    }

    fn with_state<R>(&self, f: impl FnOnce(&mut State) -> R) -> R {
        let state = self.state.upgrade().unwrap();
        let mut guard = state.try_lock().unwrap();
        guard.with_state_mut(self.container_idx, f)
    }

    pub fn parent(&self) -> Option<Handler> {
        self.get_parent()
    }

    fn is_deleted(&self) -> bool {
        match self.state.upgrade() {
            None => false,
            Some(state) => state.try_lock().unwrap().is_deleted(self.container_idx),
        }
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
                let mut t = t.try_lock().unwrap();
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
                ans.apply_delta_with_txn(txn, &delta).unwrap();
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
                let t = t.try_lock().unwrap();
                LoroValue::String(Arc::new(t.value.to_string()))
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
            MaybeDetached::Detached(d) => d.try_lock().unwrap().attached.clone().map(|x| Self {
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
                        attributes: attr.to_option_map(),
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
                            attributes: attr.to_option_map(),
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
                let m = m.try_lock().unwrap();
                let mut map = FxHashMap::default();
                for (k, v) in m.value.iter() {
                    map.insert(k.to_string(), v.to_value());
                }
                LoroValue::Map(Arc::new(map))
            }
            MaybeDetached::Attached(a) => a.get_value(),
        }
    }

    fn get_deep_value(&self) -> LoroValue {
        match &self.inner {
            MaybeDetached::Detached(m) => {
                let m = m.try_lock().unwrap();
                let mut map = FxHashMap::default();
                for (k, v) in m.value.iter() {
                    map.insert(k.to_string(), v.to_deep_value());
                }
                LoroValue::Map(Arc::new(map))
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
                let mut m = m.try_lock().unwrap();
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
                        ans.insert_container_with_txn(txn, k, create_handler(a, id.clone()))
                            .unwrap();
                    } else {
                        ans.insert_with_txn(txn, k, v.clone()).unwrap();
                    }
                }

                Ok(ans)
            }
        }
    }

    fn get_attached(&self) -> Option<Self> {
        match &self.inner {
            MaybeDetached::Detached(d) => d.try_lock().unwrap().attached.clone().map(|x| Self {
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
                let a = a.try_lock().unwrap();
                LoroValue::List(Arc::new(a.value.iter().map(|v| v.to_value()).collect()))
            }
            MaybeDetached::Attached(a) => a.get_value(),
        }
    }

    fn get_deep_value(&self) -> LoroValue {
        match &self.inner {
            MaybeDetached::Detached(a) => {
                let a = a.try_lock().unwrap();
                LoroValue::List(Arc::new(
                    a.value.iter().map(|v| v.to_deep_value()).collect(),
                ))
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
                let mut l = l.try_lock().unwrap();
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
                        ans.insert_container_with_txn(txn, i, create_handler(a, id.clone()))
                            .unwrap();
                    } else {
                        ans.insert_with_txn(txn, i, v.clone()).unwrap();
                    }
                }

                Ok(ans)
            }
        }
    }

    fn get_attached(&self) -> Option<Self> {
        match &self.inner {
            MaybeDetached::Detached(d) => d.try_lock().unwrap().attached.clone().map(|x| Self {
                inner: MaybeDetached::Attached(x),
            }),
            MaybeDetached::Attached(_a) => Some(self.clone()),
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
                let a = a.try_lock().unwrap();
                LoroValue::List(Arc::new(a.value.iter().map(|v| v.to_value()).collect()))
            }
            MaybeDetached::Attached(a) => a.get_value(),
        }
    }

    fn get_deep_value(&self) -> LoroValue {
        match &self.inner {
            MaybeDetached::Detached(a) => {
                let a = a.try_lock().unwrap();
                LoroValue::List(Arc::new(
                    a.value.iter().map(|v| v.to_deep_value()).collect(),
                ))
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
                let mut l = l.try_lock().unwrap();
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
                        ans.insert_container_with_txn(txn, i, create_handler(a, id.clone()))
                            .unwrap();
                    } else {
                        ans.insert_with_txn(txn, i, v.clone()).unwrap();
                    }
                }

                Ok(ans)
            }
        }
    }

    fn get_attached(&self) -> Option<Self> {
        match &self.inner {
            MaybeDetached::Detached(d) => d.try_lock().unwrap().attached.clone().map(|x| Self {
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
}

impl Handler {
    pub(crate) fn new_attached(
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
        trace!("apply_diff: {:#?}", &diff);
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
            ValueOrHandler::Handler(Handler::new_attached(
                c,
                arena.clone(),
                txn.clone(),
                state.clone(),
            ))
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
    pub fn version_id(&self) -> usize {
        match &self.inner {
            MaybeDetached::Detached(_) => {
                unimplemented!("Detached text container does not have version id")
            }
            MaybeDetached::Attached(a) => {
                a.with_state(|state| state.as_richtext_state_mut().unwrap().get_version_id())
            }
        }
    }

    pub fn get_richtext_value(&self) -> LoroValue {
        match &self.inner {
            MaybeDetached::Detached(t) => {
                let t = t.try_lock().unwrap();
                t.value.get_richtext_value()
            }
            MaybeDetached::Attached(a) => {
                a.with_state(|state| state.as_richtext_state_mut().unwrap().get_richtext_value())
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        match &self.inner {
            MaybeDetached::Detached(t) => t.try_lock().unwrap().value.is_empty(),
            MaybeDetached::Attached(a) => {
                a.with_state(|state| state.as_richtext_state_mut().unwrap().is_empty())
            }
        }
    }

    pub fn len_utf8(&self) -> usize {
        match &self.inner {
            MaybeDetached::Detached(t) => {
                let t = t.try_lock().unwrap();
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
                let t = t.try_lock().unwrap();
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
                let t = t.try_lock().unwrap();
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

    pub fn diagnose(&self) {
        match &self.inner {
            MaybeDetached::Detached(t) => {
                let t = t.try_lock().unwrap();
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
                let t = t.try_lock().unwrap();
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

    /// `pos` is a Event Index:
    ///
    /// - if feature="wasm", pos is a UTF-16 index
    /// - if feature!="wasm", pos is a Unicode index
    pub fn char_at(&self, pos: usize) -> LoroResult<char> {
        if pos >= self.len_event() {
            return Err(LoroError::OutOfBound {
                pos,
                len: self.len_event(),
                info: format!("Position: {}:{}", file!(), line!()).into_boxed_str(),
            });
        }
        if let Ok(c) = match &self.inner {
            MaybeDetached::Detached(t) => {
                let t = t.try_lock().unwrap();
                t.value.get_char_by_event_index(pos)
            }
            MaybeDetached::Attached(a) => a.with_state(|state| {
                state
                    .as_richtext_state_mut()
                    .unwrap()
                    .get_char_by_event_index(pos)
            }),
        } {
            Ok(c)
        } else {
            Err(LoroError::OutOfBound {
                pos,
                len: self.len_event(),
                info: format!("Position: {}:{}", file!(), line!()).into_boxed_str(),
            })
        }
    }

    /// `start_index` and `end_index` are Event Index:
    ///
    /// - if feature="wasm", pos is a UTF-16 index
    /// - if feature!="wasm", pos is a Unicode index
    ///
    pub fn slice(&self, start_index: usize, end_index: usize) -> LoroResult<String> {
        if end_index < start_index {
            return Err(LoroError::EndIndexLessThanStartIndex {
                start: start_index,
                end: end_index,
            });
        }
        match &self.inner {
            MaybeDetached::Detached(t) => {
                let t = t.try_lock().unwrap();
                t.value
                    .get_text_slice_by_event_index(start_index, end_index - start_index)
            }
            MaybeDetached::Attached(a) => a.with_state(|state| {
                state
                    .as_richtext_state_mut()
                    .unwrap()
                    .get_text_slice_by_event_index(start_index, end_index - start_index)
            }),
        }
    }

    /// `pos` is a Event Index:
    ///
    /// - if feature="wasm", pos is a UTF-16 index
    /// - if feature!="wasm", pos is a Unicode index
    ///
    /// This method requires auto_commit to be enabled.
    pub fn splice(&self, pos: usize, len: usize, s: &str) -> LoroResult<String> {
        let x = self.slice(pos, pos + len)?;
        self.delete(pos, len)?;
        self.insert(pos, s)?;
        Ok(x)
    }

    pub fn splice_utf8(&self, pos: usize, len: usize, s: &str) -> LoroResult<()> {
        // let x = self.slice(pos, pos + len)?;
        self.delete_utf8(pos, len)?;
        self.insert_utf8(pos, s)?;
        Ok(())
    }

    /// `pos` is a Event Index:
    ///
    /// - if feature="wasm", pos is a UTF-16 index
    /// - if feature!="wasm", pos is a Unicode index
    ///
    /// This method requires auto_commit to be enabled.
    pub fn insert(&self, pos: usize, s: &str) -> LoroResult<()> {
        match &self.inner {
            MaybeDetached::Detached(t) => {
                let mut t = t.try_lock().unwrap();
                let (index, _) = t
                    .value
                    .get_entity_index_for_text_insert(pos, PosType::Event)
                    .unwrap();
                t.value.insert_at_entity_index(
                    index,
                    BytesSlice::from_bytes(s.as_bytes()),
                    IdFull::NONE_ID,
                );
                Ok(())
            }
            MaybeDetached::Attached(a) => a.with_txn(|txn| self.insert_with_txn(txn, pos, s)),
        }
    }

    pub fn insert_utf8(&self, pos: usize, s: &str) -> LoroResult<()> {
        match &self.inner {
            MaybeDetached::Detached(t) => {
                let mut t = t.try_lock().unwrap();
                let (index, _) = t
                    .value
                    .get_entity_index_for_text_insert(pos, PosType::Bytes)
                    .unwrap();
                t.value.insert_at_entity_index(
                    index,
                    BytesSlice::from_bytes(s.as_bytes()),
                    IdFull::NONE_ID,
                );
                Ok(())
            }
            MaybeDetached::Attached(a) => a.with_txn(|txn| self.insert_with_txn_utf8(txn, pos, s)),
        }
    }

    pub fn insert_unicode(&self, pos: usize, s: &str) -> LoroResult<()> {
        match &self.inner {
            MaybeDetached::Detached(t) => {
                let mut t = t.try_lock().unwrap();
                let (index, _) = t
                    .value
                    .get_entity_index_for_text_insert(pos, PosType::Unicode)
                    .unwrap();
                t.value.insert_at_entity_index(
                    index,
                    BytesSlice::from_bytes(s.as_bytes()),
                    IdFull::NONE_ID,
                );
                Ok(())
            }
            MaybeDetached::Attached(a) => a.with_txn(|txn| {
                self.insert_with_txn_and_attr(txn, pos, s, None, PosType::Unicode)?;
                Ok(())
            }),
        }
    }

    /// `pos` is a Event Index:
    ///
    /// - if feature="wasm", pos is a UTF-16 index
    /// - if feature!="wasm", pos is a Unicode index
    pub fn insert_with_txn(&self, txn: &mut Transaction, pos: usize, s: &str) -> LoroResult<()> {
        self.insert_with_txn_and_attr(txn, pos, s, None, PosType::Event)?;
        Ok(())
    }

    pub fn insert_with_txn_utf8(
        &self,
        txn: &mut Transaction,
        pos: usize,
        s: &str,
    ) -> LoroResult<()> {
        self.insert_with_txn_and_attr(txn, pos, s, None, PosType::Bytes)?;
        Ok(())
    }

    /// `pos` is a Event Index:
    ///
    /// - if feature="wasm", pos is a UTF-16 index
    /// - if feature!="wasm", pos is a Unicode index
    ///
    /// This method requires auto_commit to be enabled.
    pub fn delete(&self, pos: usize, len: usize) -> LoroResult<()> {
        match &self.inner {
            MaybeDetached::Detached(t) => {
                let mut t = t.try_lock().unwrap();
                let ranges = t
                    .value
                    .get_text_entity_ranges(pos, len, PosType::Event)
                    .unwrap();
                for range in ranges.iter().rev() {
                    t.value
                        .drain_by_entity_index(range.entity_start, range.entity_len(), None);
                }
                Ok(())
            }
            MaybeDetached::Attached(a) => a.with_txn(|txn| self.delete_with_txn(txn, pos, len)),
        }
    }

    pub fn delete_utf8(&self, pos: usize, len: usize) -> LoroResult<()> {
        match &self.inner {
            MaybeDetached::Detached(t) => {
                let mut t = t.try_lock().unwrap();
                let ranges = match t.value.get_text_entity_ranges(pos, len, PosType::Bytes) {
                    Err(x) => return Err(x),
                    Ok(x) => x,
                };
                for range in ranges.iter().rev() {
                    t.value
                        .drain_by_entity_index(range.entity_start, range.entity_len(), None);
                }
                Ok(())
            }
            MaybeDetached::Attached(a) => {
                a.with_txn(|txn| self.delete_with_txn_inline(txn, pos, len, PosType::Bytes))
            }
        }
    }

    pub fn delete_unicode(&self, pos: usize, len: usize) -> LoroResult<()> {
        match &self.inner {
            MaybeDetached::Detached(t) => {
                let mut t = t.try_lock().unwrap();
                let ranges = match t.value.get_text_entity_ranges(pos, len, PosType::Unicode) {
                    Err(x) => return Err(x),
                    Ok(x) => x,
                };
                for range in ranges.iter().rev() {
                    t.value
                        .drain_by_entity_index(range.entity_start, range.entity_len(), None);
                }
                Ok(())
            }
            MaybeDetached::Attached(a) => {
                a.with_txn(|txn| self.delete_with_txn_inline(txn, pos, len, PosType::Unicode))
            }
        }
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
            &inner.state,
        )?;

        Ok(override_styles)
    }

    /// `pos` is a Event Index:
    ///
    /// - if feature="wasm", pos is a UTF-16 index
    /// - if feature!="wasm", pos is a Unicode index
    pub fn delete_with_txn(&self, txn: &mut Transaction, pos: usize, len: usize) -> LoroResult<()> {
        self.delete_with_txn_inline(txn, pos, len, PosType::Event)
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

        match pos_type {
            PosType::Event => {
                if pos + len > self.len_event() {
                    error!("pos={} len={} len_event={}", pos, len, self.len_event());
                    return Err(LoroError::OutOfBound {
                        pos: pos + len,
                        len: self.len_event(),
                        info: format!("Position: {}:{}", file!(), line!()).into_boxed_str(),
                    });
                }
            }
            PosType::Bytes => {
                if pos + len > self.len_utf8() {
                    error!("pos={} len={} len_bytes={}", pos, len, self.len_utf8());
                    return Err(LoroError::OutOfBound {
                        pos: pos + len,
                        len: self.len_utf8(),
                        info: format!("Position: {}:{}", file!(), line!()).into_boxed_str(),
                    });
                }
            }
            _ => (),
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
                &inner.state,
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
        match &self.inner {
            MaybeDetached::Detached(t) => self.mark_for_detached(
                &mut t.try_lock().unwrap().value,
                key,
                &value,
                start,
                end,
                false,
            ),
            MaybeDetached::Attached(a) => {
                a.with_txn(|txn| self.mark_with_txn(txn, start, end, key, value, false))
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
        is_delete: bool,
    ) -> Result<(), LoroError> {
        let key: InternalString = key.into();
        let len = self.len_event();
        if start >= end {
            return Err(loro_common::LoroError::ArgErr(
                "Start must be less than end".to_string().into_boxed_str(),
            ));
        }
        if end > len {
            return Err(LoroError::OutOfBound {
                pos: end,
                len,
                info: format!("Position: {}:{}", file!(), line!()).into_boxed_str(),
            });
        }
        let (entity_range, styles) =
            state.get_entity_range_and_text_styles_at_range(start..end, PosType::Event);
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
        match &self.inner {
            MaybeDetached::Detached(t) => self.mark_for_detached(
                &mut t.try_lock().unwrap().value,
                key,
                &LoroValue::Null,
                start,
                end,
                true,
            ),
            MaybeDetached::Attached(a) => {
                a.with_txn(|txn| self.mark_with_txn(txn, start, end, key, LoroValue::Null, true))
            }
        }
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
            return Err(LoroError::OutOfBound {
                pos: end,
                len,
                info: format!("Position: {}:{}", file!(), line!()).into_boxed_str(),
            });
        }

        let inner = self.inner.try_attached_state()?;
        let key: InternalString = key.into();

        let mutex = &inner.state.upgrade().unwrap();
        let mut doc_state = mutex.try_lock().unwrap();
        let (entity_range, skip) = doc_state.with_state_mut(inner.container_idx, |state| {
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
            inner.container_idx,
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
            &inner.state,
        )?;

        txn.apply_local_op(
            inner.container_idx,
            crate::op::RawOpContent::List(ListOp::StyleEnd),
            EventHint::MarkEnd,
            &inner.state,
        )?;

        Ok(())
    }

    pub fn check(&self) {
        match &self.inner {
            MaybeDetached::Detached(t) => {
                let t = t.try_lock().unwrap();
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
                let _t = t.try_lock().unwrap();
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
                        PosType::Event,
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

    #[instrument(level = "trace", skip(self))]
    pub fn update(&self, text: &str) {
        let old_str = self.to_string();
        let new = text.chars().map(|x| x as u32).collect::<Vec<u32>>();
        myers_diff(
            &mut OperateProxy::new(text_update::DiffHook::new(self, &new)),
            &old_str.chars().map(|x| x as u32).collect::<Vec<u32>>(),
            &new,
        );
    }

    #[instrument(level = "trace", skip(self))]
    pub fn update_by_line(&self, text: &str) {
        let hook = text_update::DiffHookForLine::new(self, text);
        let old_lines = hook.get_old_arr().to_vec();
        let new_lines = hook.get_new_arr().to_vec();
        trace!("old_lines: {:?}", old_lines);
        trace!("new_lines: {:?}", new_lines);
        myers_diff(&mut OperateProxy::new(hook), &old_lines, &new_lines);
    }

    #[allow(clippy::inherent_to_string)]
    pub fn to_string(&self) -> String {
        match &self.inner {
            MaybeDetached::Detached(t) => t.try_lock().unwrap().value.to_string(),
            MaybeDetached::Attached(a) => {
                Arc::unwrap_or_clone(a.get_value().into_string().unwrap())
            }
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
                .try_lock()
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

    fn get_delta(&self) -> Vec<TextDelta> {
        self.with_state(|state| {
            let state = state.as_richtext_state_mut().unwrap();
            Ok(state.get_delta())
        })
        .unwrap()
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
                let mut list = l.try_lock().unwrap();
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
            &inner.state,
        )
    }

    pub fn push(&self, v: impl Into<LoroValue>) -> LoroResult<()> {
        match &self.inner {
            MaybeDetached::Detached(l) => {
                let mut list = l.try_lock().unwrap();
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
                let mut list = l.try_lock().unwrap();
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
                let mut list = l.try_lock().unwrap();
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
            &inner.state,
        )?;
        let ans = child.attach(txn, inner, container_id)?;
        Ok(ans)
    }

    pub fn delete(&self, pos: usize, len: usize) -> LoroResult<()> {
        match &self.inner {
            MaybeDetached::Detached(l) => {
                let mut list = l.try_lock().unwrap();
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
                &inner.state,
            )?;
        }

        Ok(())
    }

    pub fn get_child_handler(&self, index: usize) -> LoroResult<Handler> {
        match &self.inner {
            MaybeDetached::Detached(l) => {
                let list = l.try_lock().unwrap();
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
            MaybeDetached::Detached(l) => l.try_lock().unwrap().value.len(),
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
            MaybeDetached::Detached(l) => {
                l.try_lock().unwrap().value.get(index).map(|x| x.to_value())
            }
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
                let l = l.try_lock().unwrap();
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
        I: FnMut((usize, ValueOrHandler)),
    {
        match &self.inner {
            MaybeDetached::Detached(l) => {
                let l = l.try_lock().unwrap();
                for (i, v) in l.value.iter().enumerate() {
                    f((i, v.clone()))
                }
            }
            MaybeDetached::Attached(inner) => {
                inner.with_state(|state| {
                    let a = state.as_list_state().unwrap();
                    for (i, v) in a.iter().enumerate() {
                        match v {
                            LoroValue::Container(c) => {
                                f((i, ValueOrHandler::Handler(create_handler(inner, c.clone()))));
                            }
                            value => {
                                f((i, ValueOrHandler::Value(value.clone())));
                            }
                        }
                    }
                });
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
                                    ValueOrHandler::Value(v) => {
                                        self.insert(index, v.clone())?;
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
                let mut l = l.try_lock().unwrap();
                l.value.clear();
                Ok(())
            }
            MaybeDetached::Attached(a) => a.with_txn(|txn| self.clear_with_txn(txn)),
        }
    }

    pub fn clear_with_txn(&self, txn: &mut Transaction) -> LoroResult<()> {
        self.delete_with_txn(txn, 0, self.len())
    }
}

impl MovableListHandler {
    pub fn insert(&self, pos: usize, v: impl Into<LoroValue>) -> LoroResult<()> {
        match &self.inner {
            MaybeDetached::Detached(d) => {
                let mut d = d.try_lock().unwrap();
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
            &inner.state,
        )
    }

    #[inline]
    pub fn mov(&self, from: usize, to: usize) -> LoroResult<()> {
        match &self.inner {
            MaybeDetached::Detached(d) => {
                let mut d = d.try_lock().unwrap();
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
            &inner.state,
        )
    }

    pub fn push(&self, v: LoroValue) -> LoroResult<()> {
        match &self.inner {
            MaybeDetached::Detached(d) => {
                let mut d = d.try_lock().unwrap();
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
                let mut d = d.try_lock().unwrap();
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
                let mut a = a.try_lock().unwrap();
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
                let mut d = d.try_lock().unwrap();
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
            &inner.state,
        )?;
        child.attach(txn, inner, container_id)
    }

    pub fn set(&self, index: usize, value: impl Into<LoroValue>) -> LoroResult<()> {
        match &self.inner {
            MaybeDetached::Detached(d) => {
                let mut d = d.try_lock().unwrap();
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
        txn.apply_local_op(inner.container_idx, op, hint, &inner.state)
    }

    pub fn set_container<H: HandlerTrait>(&self, pos: usize, child: H) -> LoroResult<H> {
        match &self.inner {
            MaybeDetached::Detached(d) => {
                let mut d = d.try_lock().unwrap();
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
    ) -> Result<H, LoroError> {
        let id = txn.next_id();
        let container_id = ContainerID::new_normal(id, child.kind());
        let v = LoroValue::Container(container_id.clone());
        let Some(elem_id) = self.with_state(|state| {
            let list = state.as_movable_list_state().unwrap();
            Ok(list.get_elem_id_at(pos, IndexType::ForUser))
        })?
        else {
            unreachable!()
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
            &inner.state,
        )?;

        child.attach(txn, inner, container_id)
    }

    pub fn delete(&self, pos: usize, len: usize) -> LoroResult<()> {
        match &self.inner {
            MaybeDetached::Detached(d) => {
                let mut d = d.try_lock().unwrap();
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

        info!(?pos, ?len, ?ids, ?new_poses, "delete_with_txn");
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
                &inner.state,
            )?;
        }

        Ok(())
    }

    pub fn get_child_handler(&self, index: usize) -> LoroResult<Handler> {
        match &self.inner {
            MaybeDetached::Detached(l) => {
                let list = l.try_lock().unwrap();
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
                let d = d.try_lock().unwrap();
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
            .state
            .upgrade()
            .unwrap()
            .try_lock()
            .unwrap()
            .get_container_deep_value_with_id(inner.container_idx, None)
    }

    pub fn get(&self, index: usize) -> Option<LoroValue> {
        match &self.inner {
            MaybeDetached::Detached(d) => {
                let d = d.try_lock().unwrap();
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
                let d = d.try_lock().unwrap();
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
                let d = d.try_lock().unwrap();
                for v in d.value.iter() {
                    f(v.clone());
                }
            }
            MaybeDetached::Attached(m) => m.with_state(|state| {
                let a = state.as_movable_list_state().unwrap();
                for v in a.iter() {
                    match v {
                        LoroValue::Container(c) => {
                            f(ValueOrHandler::Handler(create_handler(m, c.clone())));
                        }
                        value => {
                            f(ValueOrHandler::Value(value.clone()));
                        }
                    }
                }
            }),
        }
    }

    pub fn log_internal_state(&self) -> String {
        match &self.inner {
            MaybeDetached::Detached(d) => {
                let d = d.try_lock().unwrap();
                format!("{:#?}", &d.value)
            }
            MaybeDetached::Attached(a) => a.with_state(|state| {
                let a = state.as_movable_list_state().unwrap();
                format!("{:#?}", a)
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
                let mut d = d.try_lock().unwrap();
                d.value.clear();
                Ok(())
            }
            MaybeDetached::Attached(a) => a.with_txn(|txn| self.clear_with_txn(txn)),
        }
    }

    pub fn clear_with_txn(&self, txn: &mut Transaction) -> LoroResult<()> {
        self.delete_with_txn(txn, 0, self.len())
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
                let mut m = m.try_lock().unwrap();
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
                let mut m = m.try_lock().unwrap();
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
                    &inner.state,
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
            &inner.state,
        )
    }

    pub fn insert_container<T: HandlerTrait>(&self, key: &str, handler: T) -> LoroResult<T> {
        match &self.inner {
            MaybeDetached::Detached(m) => {
                let mut m = m.try_lock().unwrap();
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
            &inner.state,
        )?;

        child.attach(txn, inner, container_id)
    }

    pub fn delete(&self, key: &str) -> LoroResult<()> {
        match &self.inner {
            MaybeDetached::Detached(m) => {
                let mut m = m.try_lock().unwrap();
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
            &inner.state,
        )
    }

    pub fn for_each<I>(&self, mut f: I)
    where
        I: FnMut(&str, ValueOrHandler),
    {
        match &self.inner {
            MaybeDetached::Detached(m) => {
                let m = m.try_lock().unwrap();
                for (k, v) in m.value.iter() {
                    f(k, v.clone());
                }
            }
            MaybeDetached::Attached(inner) => {
                inner.with_state(|state| {
                    let a = state.as_map_state().unwrap();
                    for (k, v) in a.iter() {
                        if let Some(v) = &v.value {
                            match v {
                                LoroValue::Container(c) => {
                                    f(k, ValueOrHandler::Handler(create_handler(inner, c.clone())))
                                }
                                value => f(k, ValueOrHandler::Value(value.clone())),
                            }
                        }
                    }
                });
            }
        }
    }

    pub fn get_child_handler(&self, key: &str) -> LoroResult<Handler> {
        match &self.inner {
            MaybeDetached::Detached(m) => {
                let m = m.try_lock().unwrap();
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
                let m = m.try_lock().unwrap();
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
                let m = m.try_lock().unwrap();
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
            MaybeDetached::Detached(m) => m.try_lock().unwrap().value.len(),
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
                let mut m = m.try_lock().unwrap();
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
                let m = m.try_lock().unwrap();
                keys = m.value.keys().map(|x| x.as_str().into()).collect();
            }
            MaybeDetached::Attached(a) => {
                a.with_state(|state| {
                    for (k, _) in state.as_map_state().unwrap().iter() {
                        keys.push(k.clone());
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
                let m = m.try_lock().unwrap();
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
                    let d = &mut d.try_lock().unwrap().value;
                    *d += n;
                    Ok(())
                }
                MaybeDetached::Attached(a) => a.with_txn(|txn| self.increment_with_txn(txn, n)),
            }
        }

        pub fn decrement(&self, n: f64) -> LoroResult<()> {
            match &self.inner {
                MaybeDetached::Detached(d) => {
                    let d = &mut d.try_lock().unwrap().value;
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
                &inner.state,
            )
        }

        pub fn is_deleted(&self) -> bool {
            match &self.inner {
                MaybeDetached::Detached(_) => false,
                MaybeDetached::Attached(a) => a.is_deleted(),
            }
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
                    let t = t.try_lock().unwrap();
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
                    let mut v = v.try_lock().unwrap();
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
    }
}

#[cfg(test)]
mod test {

    use super::{HandlerTrait, TextDelta};
    use crate::loro::LoroDoc;
    use crate::state::TreeParentId;
    use crate::version::Frontiers;
    use crate::{fx_map, ToJson};
    use loro_common::ID;
    use serde_json::json;

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
        loro2.import(&loro.export_snapshot().unwrap()).unwrap();
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
            .with_txn(|txn| tree.create_with_txn(txn, TreeParentId::Root, 0))
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
            r#"[{"parent":null,"meta":{"a":123},"id":"0@1","index":0,"children":[],"fractional_index":"80"}]"#,
            tree.get_deep_value().to_json()
        );
        let bytes = loro.export_snapshot().unwrap();
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
            let id = tree.create_with_txn(txn, TreeParentId::Root, 0)?;
            let meta = tree.get_meta(id)?;
            meta.insert_with_txn(txn, "a", 1.into())?;
            text.insert_with_txn(txn, 0, "abc")?;
            let _id2 = tree.create_with_txn(txn, TreeParentId::Root, 0)?;
            meta.insert_with_txn(txn, "b", 2.into())?;
            Ok(id)
        })
        .unwrap();

        let loro2 = LoroDoc::new();
        let _g = loro2.subscribe_root(Arc::new(|e| {
            println!("{} {:?} ", e.event_meta.by, e.event_meta.diff)
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

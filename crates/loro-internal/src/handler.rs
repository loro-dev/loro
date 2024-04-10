use super::{state::DocState, txn::Transaction};
use crate::{
    arena::SharedArena,
    container::{
        idx::ContainerIdx,
        list::list_op::{DeleteSpan, DeleteSpanWithId, ListOp},
        richtext::{richtext_state::PosType, RichtextState, StyleOp, TextStyleInfoFlag},
        tree::tree_op::TreeOp,
    },
    cursor::{Cursor, Side},
    delta::{DeltaItem, StyleMeta, TreeDiffItem, TreeExternalDiff},
    op::ListSlice,
    state::{ContainerState, State, TreeParentId},
    txn::EventHint,
    utils::{string_slice::StringSlice, utf16::count_utf16_len},
};
use append_only_bytes::BytesSlice;
use enum_as_inner::EnumAsInner;
use fxhash::FxHashMap;
use loro_common::{
    ContainerID, ContainerType, Counter, IdFull, InternalString, LoroError, LoroResult,
    LoroTreeError, LoroValue, PeerID, TreeID, ID,
};
use serde::{Deserialize, Serialize};
use std::{
    borrow::Cow,
    fmt::Debug,
    ops::Deref,
    sync::{Arc, Mutex, Weak},
};

const INSERT_CONTAINER_VALUE_ARG_ERROR: &str =
    "Cannot insert a LoroValue::Container directly. To create child container, use insert_container";

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
            .ok_or(LoroError::MisuseDettachedContainer {
                method: "with_state",
            })?;
        let state = inner.state.upgrade().unwrap();
        let mut guard = state.lock().unwrap();
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
#[derive(Clone)]
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
            MaybeDetached::Detached(_) => Err(LoroError::MisuseDettachedContainer {
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
        let mut guard = state.lock().unwrap();
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
            })
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

    pub fn get_deep_value(&self) -> LoroValue {
        self.state
            .upgrade()
            .unwrap()
            .lock()
            .unwrap()
            .get_container_deep_value(self.container_idx)
    }

    fn with_state<R>(&self, f: impl FnOnce(&mut State) -> R) -> R {
        let state = self.state.upgrade().unwrap();
        let mut guard = state.lock().unwrap();
        guard.with_state_mut(self.container_idx, f)
    }

    pub fn parent(&self) -> Option<Handler> {
        self.get_parent()
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
            MaybeDetached::Attached(_a) => unreachable!(),
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
            MaybeDetached::Attached(a) => {
                a.with_state(|state| state.as_richtext_state_mut().unwrap().get_value())
            }
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
            MaybeDetached::Attached(_a) => unreachable!(),
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
}

impl std::fmt::Debug for MapHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.inner {
            MaybeDetached::Detached(_) => write!(f, "MapHandler Dettached"),
            MaybeDetached::Attached(a) => write!(f, "MapHandler {}", a.id),
        }
    }
}

#[derive(Clone)]
pub struct ListHandler {
    inner: MaybeDetached<Vec<ValueOrHandler>>,
}

impl std::fmt::Debug for ListHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.inner {
            MaybeDetached::Detached(_) => write!(f, "ListHandler Dettached"),
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
            MaybeDetached::Attached(_a) => unreachable!(),
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
}

///
#[derive(Clone)]
pub struct TreeHandler {
    inner: MaybeDetached<TreeInner>,
}

#[derive(Clone)]
struct TreeInner {
    next_counter: Counter,
    map: FxHashMap<TreeID, MapHandler>,
    parent_links: FxHashMap<TreeID, Option<TreeID>>,
}

impl TreeInner {
    fn new() -> Self {
        TreeInner {
            next_counter: 0,
            map: FxHashMap::default(),
            parent_links: FxHashMap::default(),
        }
    }

    fn create(&mut self, parent: Option<TreeID>) -> TreeID {
        let id = TreeID::new(PeerID::MAX, self.next_counter);
        self.next_counter += 1;
        self.map.insert(
            id,
            Handler::new_unattached(ContainerType::Map)
                .into_map()
                .unwrap(),
        );
        self.parent_links.insert(id, parent);
        id
    }

    fn delete(&mut self, id: TreeID) {
        self.map.remove(&id);
        self.parent_links.remove(&id);
    }

    fn get_parent(&self, id: TreeID) -> Option<Option<TreeID>> {
        self.parent_links.get(&id).cloned()
    }

    fn mov(&mut self, target: TreeID, new_parent: Option<TreeID>) {
        let old = self.parent_links.insert(target, new_parent);
        assert!(old.is_some());
    }

    fn get_children(&self, id: TreeID) -> Option<Vec<TreeID>> {
        let mut children = Vec::new();
        for (c, p) in &self.parent_links {
            if p.as_ref() == Some(&id) {
                children.push(*c);
            }
        }
        Some(children)
    }
}

impl HandlerTrait for TreeHandler {
    fn to_handler(&self) -> Handler {
        Handler::Tree(self.clone())
    }

    fn attach(
        &self,
        _txn: &mut Transaction,
        parent: &BasicHandler,
        self_id: ContainerID,
    ) -> LoroResult<Self> {
        match &self.inner {
            MaybeDetached::Detached(t) => {
                let mut t = t.try_lock().unwrap();
                let inner = create_handler(parent, self_id);
                let tree = inner.into_tree().unwrap();
                if t.value.map.is_empty() {
                    t.attached = tree.attached_handler().cloned();
                    Ok(tree)
                } else {
                    unimplemented!("attach detached tree");
                }
            }
            MaybeDetached::Attached(_a) => unreachable!(),
        }
    }

    fn is_attached(&self) -> bool {
        self.inner.is_attached()
    }

    fn attached_handler(&self) -> Option<&BasicHandler> {
        self.inner.attached_handler()
    }

    fn get_value(&self) -> LoroValue {
        match &self.inner {
            MaybeDetached::Detached(_) => unimplemented!(),
            MaybeDetached::Attached(a) => a.get_value(),
        }
    }

    fn get_deep_value(&self) -> LoroValue {
        match &self.inner {
            MaybeDetached::Detached(_) => unimplemented!(),
            MaybeDetached::Attached(a) => a.get_deep_value(),
        }
    }

    fn kind(&self) -> ContainerType {
        ContainerType::Tree
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
            Handler::Tree(x) => Some(x),
            _ => None,
        }
    }
}

impl std::fmt::Debug for TreeHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.inner {
            MaybeDetached::Detached(_) => write!(f, "TreeHandler Dettached"),
            MaybeDetached::Attached(a) => write!(f, "TreeHandler {}", a.id),
        }
    }
}

#[derive(Clone, EnumAsInner, Debug)]
pub enum Handler {
    Text(TextHandler),
    Map(MapHandler),
    List(ListHandler),
    Tree(TreeHandler),
}

impl HandlerTrait for Handler {
    fn is_attached(&self) -> bool {
        match self {
            Self::Text(x) => x.is_attached(),
            Self::Map(x) => x.is_attached(),
            Self::List(x) => x.is_attached(),
            Self::Tree(x) => x.is_attached(),
        }
    }

    fn attached_handler(&self) -> Option<&BasicHandler> {
        match self {
            Self::Text(x) => x.attached_handler(),
            Self::Map(x) => x.attached_handler(),
            Self::List(x) => x.attached_handler(),
            Self::Tree(x) => x.attached_handler(),
        }
    }

    fn get_value(&self) -> LoroValue {
        match self {
            Self::Text(x) => x.get_value(),
            Self::Map(x) => x.get_value(),
            Self::List(x) => x.get_value(),
            Self::Tree(x) => x.get_value(),
        }
    }

    fn get_deep_value(&self) -> LoroValue {
        match self {
            Self::Text(x) => x.get_deep_value(),
            Self::Map(x) => x.get_deep_value(),
            Self::List(x) => x.get_deep_value(),
            Self::Tree(x) => x.get_deep_value(),
        }
    }

    fn kind(&self) -> ContainerType {
        match self {
            Self::Text(x) => x.kind(),
            Self::Map(x) => x.kind(),
            Self::List(x) => x.kind(),
            Self::Tree(x) => x.kind(),
        }
    }

    fn to_handler(&self) -> Handler {
        match self {
            Self::Text(x) => x.to_handler(),
            Self::Map(x) => x.to_handler(),
            Self::List(x) => x.to_handler(),
            Self::Tree(x) => x.to_handler(),
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
            Self::Tree(x) => Ok(Handler::Tree(x.attach(txn, parent, self_id)?)),
        }
    }

    fn get_attached(&self) -> Option<Self> {
        match self {
            Self::Text(x) => x.get_attached().map(Handler::Text),
            Self::Map(x) => x.get_attached().map(Handler::Map),
            Self::List(x) => x.get_attached().map(Handler::List),
            Self::Tree(x) => x.get_attached().map(Handler::Tree),
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
        }
    }

    pub(crate) fn new_unattached(kind: ContainerType) -> Self {
        match kind {
            ContainerType::Text => Self::Text(TextHandler::new_detached()),
            ContainerType::Map => Self::Map(MapHandler::new_detached()),
            ContainerType::List => Self::List(ListHandler::new_detached()),
            ContainerType::Tree => Self::Tree(TreeHandler::new_detached()),
        }
    }

    pub fn id(&self) -> ContainerID {
        match self {
            Self::Map(x) => x.id(),
            Self::List(x) => x.id(),
            Self::Text(x) => x.id(),
            Self::Tree(x) => x.id(),
        }
    }

    pub(crate) fn container_idx(&self) -> ContainerIdx {
        match self {
            Self::Map(x) => x.idx(),
            Self::List(x) => x.idx(),
            Self::Text(x) => x.idx(),
            Self::Tree(x) => x.idx(),
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

    fn get_deep_value(&self) -> LoroValue {
        match self {
            Self::Map(x) => x.get_deep_value(),
            Self::List(x) => x.get_deep_value(),
            Self::Text(x) => x.get_deep_value(),
            Self::Tree(x) => x.get_deep_value(),
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

    fn to_value(&self) -> LoroValue {
        match self {
            Self::Value(v) => v.clone(),
            Self::Handler(h) => LoroValue::Container(h.id()),
        }
    }

    fn to_deep_value(&self) -> LoroValue {
        match self {
            Self::Value(v) => v.clone(),
            Self::Handler(h) => h.get_deep_value(),
        }
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
                let index = t
                    .value
                    .get_entity_index_for_text_insert(pos, PosType::Event);
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

        let inner = self.inner.try_attached_state()?;
        let (entity_index, styles) = inner.with_state(|state| {
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
            inner.container_idx,
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
            &inner.state,
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
        match &self.inner {
            MaybeDetached::Detached(t) => {
                let mut t = t.try_lock().unwrap();
                let ranges = t.value.get_text_entity_ranges(pos, len, PosType::Event);
                for range in ranges.iter().rev() {
                    t.value
                        .drain_by_entity_index(range.entity_start, range.entity_len(), None);
                }
                Ok(())
            }
            MaybeDetached::Attached(a) => a.with_txn(|txn| self.delete_with_txn(txn, pos, len)),
        }
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

        let inner = self.inner.try_attached_state()?;
        let s = tracing::span!(tracing::Level::INFO, "delete pos={} len={}", pos, len);
        let _e = s.enter();
        let ranges = inner.with_state(|state| {
            let richtext_state = state.as_richtext_state_mut().unwrap();
            richtext_state.get_text_entity_ranges_in_event_index_range(pos, len)
        });

        debug_assert_eq!(ranges.iter().map(|x| x.event_len).sum::<usize>(), len);
        let mut event_end = (pos + len) as isize;
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
            MaybeDetached::Detached(t) => {
                self.mark_for_detached(&mut t.lock().unwrap().value, key, &value, start, end, false)
            }
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
            return Err(LoroError::OutOfBound { pos: end, len });
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
                &mut t.lock().unwrap().value,
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
            return Err(LoroError::OutOfBound { pos: end, len });
        }

        let inner = self.inner.try_attached_state()?;
        let key: InternalString = key.into();

        let mutex = &inner.state.upgrade().unwrap();
        let mut doc_state = mutex.lock().unwrap();
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
        match &self.inner {
            MaybeDetached::Detached(t) => t.try_lock().unwrap().value.to_string(),
            MaybeDetached::Attached(a) => {
                a.with_state(|s| s.as_richtext_state_mut().unwrap().to_string_mut())
            }
        }
    }

    /// Get the stable position representation for the target pos
    pub fn get_cursor(&self, event_index: usize, side: Side) -> Option<Cursor> {
        match &self.inner {
            MaybeDetached::Detached(_) => None,
            MaybeDetached::Attached(a) => {
                let (id, len) = a.with_state(|s| {
                    let s = s.as_richtext_state_mut().unwrap();
                    (s.get_stable_position(event_index), s.len_event())
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
                    });
                }

                if len <= event_index {
                    return Some(Cursor {
                        id: None,
                        container: self.id(),
                        side: Side::Right,
                    });
                }

                let id = id?;
                Some(Cursor {
                    id: Some(id),
                    container: self.id(),
                    side,
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
            EventHint::InsertList { len: 1 },
            &inner.state,
        )
    }

    pub fn push(&self, v: LoroValue) -> LoroResult<()> {
        match &self.inner {
            MaybeDetached::Detached(l) => {
                let mut list = l.try_lock().unwrap();
                list.value.push(ValueOrHandler::Value(v.clone()));
                Ok(())
            }
            MaybeDetached::Attached(a) => a.with_txn(|txn| self.push_with_txn(txn, v)),
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

    pub fn insert_container_with_txn<H: HandlerTrait>(
        &self,
        txn: &mut Transaction,
        pos: usize,
        child: H,
    ) -> LoroResult<H> {
        if pos > self.len() {
            return Err(LoroError::OutOfBound {
                pos,
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
            EventHint::InsertList { len: 1 },
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
        I: FnMut(ValueOrHandler),
    {
        match &self.inner {
            MaybeDetached::Detached(l) => {
                let l = l.try_lock().unwrap();
                for v in l.value.iter() {
                    f(v.clone())
                }
            }
            MaybeDetached::Attached(inner) => {
                inner.with_state(|state| {
                    let a = state.as_list_state().unwrap();
                    for v in a.iter() {
                        match v {
                            LoroValue::Container(c) => {
                                f(ValueOrHandler::Handler(create_handler(inner, c.clone())));
                            }
                            value => {
                                f(ValueOrHandler::Value(value.clone()));
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
                    });
                }

                if len <= pos {
                    return Some(Cursor {
                        id: None,
                        container: self.id(),
                        side: Side::Right,
                    });
                }

                let id = id?;
                Some(Cursor {
                    id: Some(id.id()),
                    container: self.id(),
                    side,
                })
            }
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
        if handler.is_attached() {
            return Err(LoroError::ReattachAttachedContainer);
        }

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
                        match &v.value {
                            Some(v) => match v {
                                LoroValue::Container(c) => {
                                    f(k, ValueOrHandler::Handler(create_handler(inner, c.clone())))
                                }
                                value => f(k, ValueOrHandler::Value(value.clone())),
                            },
                            None => {}
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
            MaybeDetached::Detached(_) => Err(LoroError::MisuseDettachedContainer {
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

    pub fn len(&self) -> usize {
        match &self.inner {
            MaybeDetached::Detached(m) => m.try_lock().unwrap().value.len(),
            MaybeDetached::Attached(a) => a.with_state(|state| state.as_map_state().unwrap().len()),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl TreeHandler {
    /// Create a new container that is detached from the document.
    ///
    /// The edits on a detached container will not be persisted/synced.
    /// To attach the container to the document, please insert it into an attached
    /// container.
    pub fn new_detached() -> Self {
        Self {
            inner: MaybeDetached::new_detached(TreeInner::new()),
        }
    }

    pub fn delete(&self, target: TreeID) -> LoroResult<()> {
        match &self.inner {
            MaybeDetached::Detached(t) => {
                let mut t = t.try_lock().unwrap();
                t.value.delete(target);
                Ok(())
            }
            MaybeDetached::Attached(a) => a.with_txn(|txn| self.delete_with_txn(txn, target)),
        }
    }

    pub fn delete_with_txn(&self, txn: &mut Transaction, target: TreeID) -> LoroResult<()> {
        let inner = self.inner.try_attached_state()?;
        txn.apply_local_op(
            inner.container_idx,
            crate::op::RawOpContent::Tree(TreeOp {
                target,
                parent: Some(TreeID::delete_root()),
            }),
            EventHint::Tree(TreeDiffItem {
                target,
                action: TreeExternalDiff::Delete,
            }),
            &inner.state,
        )
    }

    pub fn create<T: Into<Option<TreeID>>>(&self, parent: T) -> LoroResult<TreeID> {
        match &self.inner {
            MaybeDetached::Detached(t) => {
                let mut t = t.try_lock().unwrap();
                Ok(t.value.create(parent.into()))
            }
            MaybeDetached::Attached(a) => a.with_txn(|txn| self.create_with_txn(txn, parent)),
        }
    }

    pub fn create_with_txn<T: Into<Option<TreeID>>>(
        &self,
        txn: &mut Transaction,
        parent: T,
    ) -> LoroResult<TreeID> {
        let inner = self.inner.try_attached_state()?;
        let parent: Option<TreeID> = parent.into();
        let tree_id = TreeID::from_id(txn.next_id());
        let event_hint = TreeDiffItem {
            target: tree_id,
            action: TreeExternalDiff::Create(parent),
        };
        txn.apply_local_op(
            inner.container_idx,
            crate::op::RawOpContent::Tree(TreeOp {
                target: tree_id,
                parent,
            }),
            EventHint::Tree(event_hint),
            &inner.state,
        )?;
        Ok(tree_id)
    }

    pub fn mov<T: Into<Option<TreeID>>>(&self, target: TreeID, parent: T) -> LoroResult<()> {
        match &self.inner {
            MaybeDetached::Detached(t) => {
                let mut t = t.try_lock().unwrap();
                t.value.mov(target, parent.into());
                Ok(())
            }
            MaybeDetached::Attached(a) => a.with_txn(|txn| self.mov_with_txn(txn, target, parent)),
        }
    }

    pub fn mov_with_txn<T: Into<Option<TreeID>>>(
        &self,
        txn: &mut Transaction,
        target: TreeID,
        parent: T,
    ) -> LoroResult<()> {
        let parent = parent.into();
        let inner = self.inner.try_attached_state()?;
        txn.apply_local_op(
            inner.container_idx,
            crate::op::RawOpContent::Tree(TreeOp { target, parent }),
            EventHint::Tree(TreeDiffItem {
                target,
                action: TreeExternalDiff::Move(parent),
            }),
            &inner.state,
        )
    }

    pub fn get_meta(&self, target: TreeID) -> LoroResult<MapHandler> {
        match &self.inner {
            MaybeDetached::Detached(d) => {
                let d = d.try_lock().unwrap();
                d.value
                    .map
                    .get(&target)
                    .cloned()
                    .ok_or(LoroTreeError::TreeNodeNotExist(target).into())
            }
            MaybeDetached::Attached(a) => {
                if !self.contains(target) {
                    return Err(LoroTreeError::TreeNodeNotExist(target).into());
                }
                let map_container_id = target.associated_meta_container();
                let handler = create_handler(a, map_container_id);
                Ok(handler.into_map().unwrap())
            }
        }
    }

    /// Get the parent of the node, if the node is deleted or does not exist, return None
    pub fn get_node_parent(&self, target: TreeID) -> Option<Option<TreeID>> {
        match &self.inner {
            MaybeDetached::Detached(t) => {
                let t = t.try_lock().unwrap();
                t.value.get_parent(target)
            }
            MaybeDetached::Attached(a) => a.with_state(|state| {
                let a = state.as_tree_state().unwrap();
                a.parent(target).map(|p| match p {
                    TreeParentId::None => None,
                    TreeParentId::Node(parent_id) => Some(parent_id),
                    _ => unreachable!(),
                })
            }),
        }
    }

    pub fn children(&self, target: TreeID) -> Vec<TreeID> {
        match &self.inner {
            MaybeDetached::Detached(t) => {
                let t = t.try_lock().unwrap();
                t.value.get_children(target).unwrap()
            }
            MaybeDetached::Attached(a) => a.with_state(|state| {
                let a = state.as_tree_state().unwrap();
                a.get_children(&TreeParentId::Node(target))
            }),
        }
    }

    pub fn contains(&self, target: TreeID) -> bool {
        match &self.inner {
            MaybeDetached::Detached(t) => {
                let t = t.try_lock().unwrap();
                t.value.map.contains_key(&target)
            }
            MaybeDetached::Attached(a) => a.with_state(|state| {
                let a = state.as_tree_state().unwrap();
                a.contains(target)
            }),
        }
    }

    pub fn nodes(&self) -> Vec<TreeID> {
        match &self.inner {
            MaybeDetached::Detached(t) => {
                let t = t.try_lock().unwrap();
                t.value.map.keys().cloned().collect()
            }
            MaybeDetached::Attached(a) => a.with_state(|state| {
                let a = state.as_tree_state().unwrap();
                a.nodes()
            }),
        }
    }

    #[cfg(feature = "test_utils")]
    pub fn next_tree_id(&self) -> TreeID {
        match &self.inner {
            MaybeDetached::Detached(d) => {
                let d = d.try_lock().unwrap();
                TreeID::new(PeerID::MAX, d.value.next_counter)
            }
            MaybeDetached::Attached(a) => a
                .with_txn(|txn| Ok(TreeID::from_id(txn.next_id())))
                .unwrap(),
        }
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

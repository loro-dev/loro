use super::{state::DocState, txn::Transaction};
use crate::sync::Mutex;
use crate::{
    container::{
        idx::ContainerIdx,
        list::list_op::{DeleteSpan, DeleteSpanWithId, ListOp},
        richtext::{richtext_state::PosType, RichtextState, StyleKey, StyleOp, TextStyleInfoFlag},
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

const REGULAR_CONTAINER_VALUE_ARG_ERROR: &str =
    "Cannot use a LoroValue::Container as a regular value. To create a child container, use insert_container/set_container, or ensure_mergeable_* on maps for mergeable children";

mod text_update;

fn ensure_no_regular_container_value(value: &LoroValue) -> LoroResult<()> {
    // Fast path: scalar values can never transitively hold a container, so we
    // skip the heap allocation + traversal below. This is the common case on
    // the per-op insert hot path (inserting numbers/strings/bools), where the
    // previous unconditional `vec![value]` allocation showed up as a measurable
    // regression.
    if !matches!(
        value,
        LoroValue::Container(_) | LoroValue::List(_) | LoroValue::Map(_)
    ) {
        return Ok(());
    }

    let mut stack = vec![value];
    while let Some(value) = stack.pop() {
        match value {
            LoroValue::Container(_) => {
                return Err(LoroError::ArgErr(
                    REGULAR_CONTAINER_VALUE_ARG_ERROR
                        .to_string()
                        .into_boxed_str(),
                ));
            }
            LoroValue::List(list) => {
                stack.extend(list.iter());
            }
            LoroValue::Map(map) => {
                stack.extend(map.values());
            }
            LoroValue::Null
            | LoroValue::Bool(_)
            | LoroValue::Double(_)
            | LoroValue::I64(_)
            | LoroValue::Binary(_)
            | LoroValue::String(_) => {}
        }
    }

    Ok(())
}

fn checked_range_end(
    pos: usize,
    len: usize,
    container_len: usize,
    // Lazily built: this is on the per-op edit hot path, so the position-context
    // string must only be allocated when a bound check actually fails.
    info: impl Fn() -> Box<str>,
) -> LoroResult<usize> {
    let end = pos.checked_add(len).ok_or_else(|| LoroError::OutOfBound {
        pos: usize::MAX,
        len: container_len,
        info: info(),
    })?;
    if end > container_len {
        return Err(LoroError::OutOfBound {
            pos: end,
            len: container_len,
            info: info(),
        });
    }

    Ok(end)
}

fn checked_delta_index_end(pos: usize, len: usize, container_len: usize) -> LoroResult<usize> {
    pos.checked_add(len).ok_or_else(|| LoroError::OutOfBound {
        pos: usize::MAX,
        len: container_len,
        info: format!("Position: {}:{}", file!(), line!()).into_boxed_str(),
    })
}

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
            .unwrap_or_else(|| {
                ContainerIdx::from_index_and_type(ContainerIdx::INDEX_MASK, self.kind())
            })
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
        let mut guard = state.lock();
        guard.with_state_mut(inner.container_idx, f)
    }
}

fn create_handler(inner: &BasicHandler, id: ContainerID) -> Handler {
    Handler::new_attached(id, inner.doc.clone())
}

fn value_to_value_or_handler(inner: &BasicHandler, value: LoroValue) -> ValueOrHandler {
    match value {
        LoroValue::Container(container_id) => {
            ValueOrHandler::Handler(create_handler(inner, container_id))
        }
        value => ValueOrHandler::Value(value),
    }
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
        let mut guard = state.lock();
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
        self.doc.state.lock().get_value_by_idx(self.container_idx)
    }

    pub fn get_deep_value(&self) -> LoroValue {
        self.doc
            .state
            .lock()
            .get_container_deep_value(self.container_idx)
    }

    fn with_state<R>(&self, f: impl FnOnce(&mut State) -> R) -> R {
        let mut guard = self.doc.state.lock();
        guard.with_state_mut(self.container_idx, f)
    }

    pub fn parent(&self) -> Option<Handler> {
        self.get_parent()
    }

    fn is_deleted(&self) -> bool {
        self.doc.state.lock().is_deleted(self.container_idx)
    }

    fn has_decoded_state(&self) -> bool {
        self.with_doc_state(|state| state.has_decoded_container_state(self.container_idx))
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
                let mut t = t.lock();
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
                let t = t.lock();
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
            MaybeDetached::Detached(d) => d.lock().attached.clone().map(|x| Self {
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
                let m = m.lock();
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
                let m = m.lock();
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
                let mut m = m.lock();
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
            MaybeDetached::Detached(d) => d.lock().attached.clone().map(|x| Self {
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
                let a = a.lock();
                LoroValue::List(a.value.iter().map(|v| v.to_value()).collect())
            }
            MaybeDetached::Attached(a) => a.get_value(),
        }
    }

    fn get_deep_value(&self) -> LoroValue {
        match &self.inner {
            MaybeDetached::Detached(a) => {
                let a = a.lock();
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
                let mut l = l.lock();
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
            MaybeDetached::Detached(d) => d.lock().attached.clone().map(|x| Self {
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
                let a = a.lock();
                LoroValue::List(a.value.iter().map(|v| v.to_value()).collect())
            }
            MaybeDetached::Attached(a) => a.get_value(),
        }
    }

    fn get_deep_value(&self) -> LoroValue {
        match &self.inner {
            MaybeDetached::Detached(a) => {
                let a = a.lock();
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
                let mut l = l.lock();
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
            MaybeDetached::Detached(d) => d.lock().attached.clone().map(|x| Self {
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
    fn apply_map_container_diff_value(
        map: &MapHandler,
        key: &str,
        old_id: ContainerID,
        on_container_remap: &mut dyn FnMut(ContainerID, ContainerID),
    ) -> LoroResult<()> {
        if old_id.is_mergeable() {
            let parent_id = map.id();
            let kind = old_id.container_type();
            let new_id = ContainerID::new_mergeable(&parent_id, key, kind);
            let marker = loro_common::mergeable_marker(&parent_id, key, kind);
            map.insert_without_skipping(key, marker)?;
            on_container_remap(old_id, new_id);
            return Ok(());
        }

        let new_h = map.insert_container(key, Handler::new_unattached(old_id.container_type()))?;
        let new_id = new_h.id();
        on_container_remap(old_id, new_id);
        Ok(())
    }

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
            if old_id != new_id {
                container_remap.insert(old_id, new_id);
            }
        };
        match self {
            Self::Map(x) => {
                let diff = match diff {
                    crate::event::Diff::Map(d) => d,
                    _ => {
                        return Err(LoroError::DecodeError(
                            "Invalid diff type for map container".into(),
                        ));
                    }
                };
                for (key, value) in diff.updated.into_iter() {
                    match value.value {
                        Some(ValueOrHandler::Handler(h)) => {
                            Self::apply_map_container_diff_value(
                                x,
                                &key,
                                h.id(),
                                on_container_remap,
                            )?;
                        }
                        Some(ValueOrHandler::Value(LoroValue::Container(old_id))) => {
                            Self::apply_map_container_diff_value(
                                x,
                                &key,
                                old_id,
                                on_container_remap,
                            )?;
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
                let delta = match diff {
                    crate::event::Diff::Text(d) => d,
                    _ => {
                        return Err(LoroError::DecodeError(
                            "Invalid diff type for text container".into(),
                        ));
                    }
                };
                x.apply_delta(&TextDelta::from_text_diff(delta.iter()))?;
            }
            Self::List(x) => {
                let delta = match diff {
                    crate::event::Diff::List(d) => d,
                    _ => {
                        return Err(LoroError::DecodeError(
                            "Invalid diff type for list container".into(),
                        ));
                    }
                };
                x.apply_delta(delta, on_container_remap)?;
            }
            Self::MovableList(x) => {
                let delta = match diff {
                    crate::event::Diff::List(d) => d,
                    _ => {
                        return Err(LoroError::DecodeError(
                            "Invalid diff type for movable list container".into(),
                        ));
                    }
                };
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
                let tree_diff = match diff {
                    crate::event::Diff::Tree(d) => d,
                    _ => {
                        return Err(LoroError::DecodeError(
                            "Invalid diff type for tree container".into(),
                        ));
                    }
                };
                for diff in tree_diff.diff {
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
                            if x.is_node_unexist(&target) || x.is_node_deleted(&target)? {
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
                            if !x.is_node_deleted(&target)? {
                                x.delete(target)?;
                            }
                        }
                    }
                }
            }
            #[cfg(feature = "counter")]
            Self::Counter(x) => {
                let delta = match diff {
                    crate::event::Diff::Counter(d) => d,
                    _ => {
                        return Err(LoroError::DecodeError(
                            "Invalid diff type for counter container".into(),
                        ));
                    }
                };
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
                let t = t.lock();
                t.value.get_richtext_value()
            }
            MaybeDetached::Attached(a) => {
                a.with_state(|state| state.as_richtext_state_mut().unwrap().get_richtext_value())
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        match &self.inner {
            MaybeDetached::Detached(t) => t.lock().value.is_empty(),
            MaybeDetached::Attached(a) if a.has_decoded_state() => {
                a.with_state(|state| state.as_richtext_state_mut().unwrap().is_empty())
            }
            MaybeDetached::Attached(a) => a.get_value().as_string().unwrap().is_empty(),
        }
    }

    pub fn len_utf8(&self) -> usize {
        match &self.inner {
            MaybeDetached::Detached(t) => {
                let t = t.lock();
                t.value.len_utf8()
            }
            MaybeDetached::Attached(a) => {
                a.with_doc_state(|state| state.get_text_len(a.container_idx, PosType::Bytes))
            }
        }
    }

    pub fn len_utf16(&self) -> usize {
        match &self.inner {
            MaybeDetached::Detached(t) => {
                let t = t.lock();
                t.value.len_utf16()
            }
            MaybeDetached::Attached(a) => {
                a.with_doc_state(|state| state.get_text_len(a.container_idx, PosType::Utf16))
            }
        }
    }

    pub fn len_unicode(&self) -> usize {
        match &self.inner {
            MaybeDetached::Detached(t) => {
                let t = t.lock();
                t.value.len_unicode()
            }
            MaybeDetached::Attached(a) => {
                a.with_doc_state(|state| state.get_text_len(a.container_idx, PosType::Unicode))
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
            MaybeDetached::Detached(t) => t.lock().value.len(pos_type),
            MaybeDetached::Attached(a) => {
                a.with_doc_state(|state| state.get_text_len(a.container_idx, pos_type))
            }
        }
    }

    fn validate_text_boundary(&self, pos: usize, pos_type: PosType) -> LoroResult<()> {
        let err = match pos_type {
            PosType::Bytes => Some(LoroError::UTF8InUnicodeCodePoint { pos }),
            PosType::Utf16 => Some(LoroError::UTF16InUnicodeCodePoint { pos }),
            PosType::Event if cfg!(feature = "wasm") => {
                Some(LoroError::UTF16InUnicodeCodePoint { pos })
            }
            _ => None,
        };

        let Some(err) = err else {
            return Ok(());
        };

        let len = self.len(pos_type);
        if pos > len {
            return Ok(());
        }

        if self.all_text_positions_are_boundaries(pos_type, len) {
            return Ok(());
        }

        let Some(unicode_pos) = self.convert_pos(pos, pos_type, PosType::Unicode) else {
            return Err(err);
        };
        if self.convert_pos(unicode_pos, PosType::Unicode, pos_type) != Some(pos) {
            return Err(err);
        }

        Ok(())
    }

    fn all_text_positions_are_boundaries(&self, pos_type: PosType, len: usize) -> bool {
        match pos_type {
            PosType::Bytes => len == self.len_unicode(),
            PosType::Utf16 => len == self.len_unicode(),
            PosType::Event if cfg!(feature = "wasm") => len == self.len_unicode(),
            _ => false,
        }
    }

    pub fn diagnose(&self) {
        match &self.inner {
            MaybeDetached::Detached(t) => {
                let t = t.lock();
                t.value.diagnose();
            }
            MaybeDetached::Attached(a) => {
                a.with_state(|state| state.as_richtext_state_mut().unwrap().diagnose());
            }
        }
    }

    pub fn iter(&self, mut callback: impl FnMut(&str) -> bool) {
        // Do not call user callbacks while holding the state lock; callbacks may re-enter Loro.
        let spans: Vec<String> = match &self.inner {
            MaybeDetached::Detached(t) => {
                let t = t.lock();
                t.value
                    .iter()
                    .map(|span| span.text.as_str().to_owned())
                    .collect()
            }
            MaybeDetached::Attached(a) => a.with_state(|state| {
                let mut spans = Vec::new();
                state.as_richtext_state_mut().unwrap().iter(|span| {
                    spans.push(span.to_owned());
                    true
                });
                spans
            }),
        };

        for span in spans {
            if !callback(span.as_str()) {
                return;
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
                let t = t.lock();
                let event_pos = match pos_type {
                    PosType::Event => pos,
                    _ => t.value.index_to_event_index(pos, pos_type),
                };
                t.value.get_char_by_event_index(event_pos)
            }
            MaybeDetached::Attached(a) if a.has_decoded_state() || pos_type == PosType::Entity => a
                .with_state(|state| {
                    let state = state.as_richtext_state_mut().unwrap();
                    let event_pos = match pos_type {
                        PosType::Event => pos,
                        _ => state.index_to_event_index(pos, pos_type),
                    };
                    state.get_char_by_event_index(event_pos)
                }),
            MaybeDetached::Attached(a) => {
                return text_char_at(a.get_value().as_string().unwrap(), pos, pos_type);
            }
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
                let t = t.lock();
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
            MaybeDetached::Attached(a) if a.has_decoded_state() || pos_type == PosType::Entity => a
                .with_state(|state| {
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
            MaybeDetached::Attached(a) => text_slice(
                a.get_value().as_string().unwrap(),
                start_index,
                end_index,
                pos_type,
            )
            .map_err(|err| match err {
                LoroError::OutOfBound { pos, len, .. } => LoroError::OutOfBound {
                    pos,
                    len,
                    info: info(),
                },
                err => err,
            }),
        }
    }

    pub fn slice_delta(
        &self,
        start_index: usize,
        end_index: usize,
        pos_type: PosType,
    ) -> LoroResult<Vec<TextDelta>> {
        if end_index < start_index {
            return Err(LoroError::EndIndexLessThanStartIndex {
                start: start_index,
                end: end_index,
            });
        }
        if start_index == end_index {
            return Ok(Vec::new());
        }

        let len = self.len(pos_type);
        if end_index > len {
            return Err(LoroError::OutOfBound {
                pos: end_index,
                len,
                info: format!("Position: {}:{}", file!(), line!()).into_boxed_str(),
            });
        }
        self.validate_text_boundary(start_index, pos_type)?;
        self.validate_text_boundary(end_index, pos_type)?;

        match &self.inner {
            MaybeDetached::Detached(t) => {
                let t = t.lock();
                let ans = t.value.slice_delta(start_index, end_index, pos_type)?;
                Ok(ans
                    .into_iter()
                    .map(|(s, a)| TextDelta::Insert {
                        insert: s,
                        attributes: a.to_option_map_without_null_value(),
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
                        attributes: a.to_option_map_without_null_value(),
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
        let end = checked_range_end(
            pos,
            len,
            self.len(pos_type),
            || format!("Position: {}:{}", file!(), line!()).into_boxed_str(),
        )?;
        let x = self.slice(pos, end, pos_type)?;
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
                let len = self.len(pos_type);
                if pos > len {
                    return Err(LoroError::OutOfBound {
                        pos,
                        len,
                        info: format!("Position: {}:{}", file!(), line!()).into_boxed_str(),
                    });
                }
                self.validate_text_boundary(pos, pos_type)?;

                let mut t = t.lock();
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
                if s.is_empty() {
                    let len = self.len(pos_type);
                    if pos > len {
                        return Err(LoroError::OutOfBound {
                            pos,
                            len,
                            info: format!("Position: {}:{}", file!(), line!()).into_boxed_str(),
                        });
                    }
                    self.validate_text_boundary(pos, pos_type)?;
                    return Ok(());
                }

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
        if len == 0 {
            return Ok(());
        }

        let text_len = self.len(pos_type);
        let end = checked_range_end(
            pos,
            len,
            text_len,
            || format!("Position: {}:{}", file!(), line!()).into_boxed_str(),
        )?;
        self.validate_text_boundary(pos, pos_type)?;
        self.validate_text_boundary(end, pos_type)?;

        match &self.inner {
            MaybeDetached::Detached(t) => {
                let mut t = t.lock();
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

        // Fast path: plain-text insert into a style-free document (non-wasm).
        // With no style anchors, entity_index == unicode pos and the event index
        // equals the unicode index, so bounds + no-styles are checked in a single
        // state access and the entire read phase (cursor location + two
        // visit_previous_caches walks + styles lookup) is skipped; apply_local_op
        // then locates the cursor exactly once.
        #[cfg(not(feature = "wasm"))]
        if attr.is_none() && pos_type == PosType::Unicode {
            let inner = self.inner.try_attached_state()?;
            let fast = inner.with_state(|state| {
                let rt = state.as_richtext_state_mut().unwrap();
                if rt.has_styles() {
                    return Ok(false);
                }
                let len = rt.len_unicode();
                if pos > len {
                    return Err(LoroError::OutOfBound {
                        pos,
                        len,
                        info: format!("Position: {}:{}", file!(), line!()).into_boxed_str(),
                    });
                }
                Ok(true)
            })?;
            if fast {
                let unicode_len = s.chars().count();
                txn.apply_local_op(
                    inner.container_idx,
                    crate::op::RawOpContent::List(
                        crate::container::list::list_op::ListOp::Insert {
                            slice: ListSlice::RawStr {
                                str: Cow::Borrowed(s),
                                unicode_len,
                            },
                            // entity_index == unicode pos (no style anchors)
                            pos,
                        },
                    ),
                    EventHint::InsertText {
                        // event index == unicode index (non-wasm)
                        pos: pos as u32,
                        styles: StyleMeta::empty(),
                        unicode_len: unicode_len as u32,
                        event_len: unicode_len as u32,
                    },
                    &inner.doc,
                )?;
                return Ok(Vec::new());
            }
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
        self.validate_text_boundary(pos, pos_type)?;

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

        let text_len = self.len(pos_type);
        let end = checked_range_end(
            pos,
            len,
            text_len,
            || format!("Position: {}:{}", file!(), line!()).into_boxed_str(),
        )
        .inspect_err(|_| error!("pos={} len={} len={}", pos, len, text_len))?;
        self.validate_text_boundary(pos, pos_type)?;
        self.validate_text_boundary(end, pos_type)?;

        let inner = self.inner.try_attached_state()?;
        let s = tracing::span!(tracing::Level::INFO, "delete", "pos={} len={}", pos, len);
        let _e = s.enter();
        let mut event_pos = 0;
        let mut event_len = 0;
        let ranges = inner.with_state(|state| {
            let richtext_state = state.as_richtext_state_mut().unwrap();
            // Fast path: with no style anchors (non-wasm), the event index equals
            // the unicode index, so the two index_to_event_index walks collapse to
            // identity.
            let fast = cfg!(not(feature = "wasm"))
                && pos_type == PosType::Unicode
                && !richtext_state.has_styles();
            if fast {
                event_pos = pos;
                event_len = len;
            } else {
                event_pos = richtext_state.index_to_event_index(pos, pos_type);
                let event_end = richtext_state.index_to_event_index(end, pos_type);
                event_len = event_end - event_pos;
            }

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
                let mut g = t.lock();
                self.mark_for_detached(&mut g.value, key, &value, start, end, pos_type)
            }
            MaybeDetached::Attached(a) => {
                a.with_txn(|txn| self.mark_with_txn(txn, start, end, key, value, pos_type))
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
    ) -> Result<(), LoroError> {
        let key: InternalString = key.into();
        let is_delete = matches!(value, &LoroValue::Null);
        if start >= end {
            return Err(loro_common::LoroError::ArgErr(
                "Start must be less than end".to_string().into_boxed_str(),
            ));
        }
        ensure_no_regular_container_value(value)?;

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
                return Ok(());
            }
        }

        let has_target_style =
            state.range_has_style_key(entity_range.clone(), &StyleKey::Key(key.clone()));
        let missing_style_key = is_delete && !has_target_style;

        if missing_style_key {
            return Ok(());
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
                &mut t.lock().value,
                key,
                &LoroValue::Null,
                start,
                end,
                pos_type,
            ),
            MaybeDetached::Attached(a) => a.with_txn(|txn| {
                self.mark_with_txn(txn, start, end, key, LoroValue::Null, pos_type)
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
    ) -> LoroResult<()> {
        if start >= end {
            return Err(loro_common::LoroError::ArgErr(
                "Start must be less than end".to_string().into_boxed_str(),
            ));
        }
        ensure_no_regular_container_value(&value)?;

        let inner = self.inner.try_attached_state()?;
        let key: InternalString = key.into();
        let is_delete = matches!(&value, &LoroValue::Null);

        let mut doc_state = inner.doc.state.lock();
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

        let (entity_range, skip, missing_style_key, event_start, event_end) = doc_state
            .with_state_mut(inner.container_idx, |state| {
                let state = state.as_richtext_state_mut().unwrap();
                let event_start = state.index_to_event_index(start, pos_type);
                let event_end = state.index_to_event_index(end, pos_type);
                let (entity_range, styles) =
                    state.get_entity_range_and_styles_at_range(start..end, pos_type);

                let skip = styles
                    .as_ref()
                    .map(|styles| styles.has_key_value(&key, &value))
                    .unwrap_or(false);
                let has_target_style = state.has_style_key_in_entity_range(
                    entity_range.clone(),
                    &StyleKey::Key(key.clone()),
                );
                let missing_style_key = is_delete && !has_target_style;

                (
                    entity_range,
                    skip,
                    missing_style_key,
                    event_start,
                    event_end,
                )
            });

        if skip || missing_style_key {
            return Ok(());
        }

        let entity_start = entity_range.start;
        let entity_end = entity_range.end;
        let style_config = doc_state.config.text_style_config.read();
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
                let t = t.lock();
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
                let _t = t.lock();
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
                    let insert_len = event_len(insert.as_str());
                    if insert_len == 0 {
                        continue;
                    }

                    let mut empty_attr = None;
                    let attr_ref = attributes.as_ref().unwrap_or_else(|| {
                        empty_attr = Some(FxHashMap::default());
                        empty_attr.as_ref().unwrap()
                    });

                    let end = checked_delta_index_end(index, insert_len, self.len_event())?;
                    let override_styles = self.insert_with_txn_and_attr(
                        txn,
                        index,
                        insert.as_str(),
                        Some(attr_ref),
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
                    let end = checked_delta_index_end(index, *retain, self.len_event())?;
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

        let mut len = match &self.inner {
            MaybeDetached::Detached(_) => self.len_event(),
            MaybeDetached::Attached(a) => {
                a.with_state(|state| state.as_richtext_state_mut().unwrap().len(PosType::Event))
            }
        };
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
            MaybeDetached::Detached(t) => t.lock().value.to_string(),
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
            MaybeDetached::Detached(s) => s.lock().value.entity_index_to_event_index(entity_index),
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
                for span in s.lock().value.iter() {
                    if span.text.as_str().is_empty() {
                        continue;
                    }

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
                let mut t = mutex.lock();
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

    /// Convert a position `index` from one coordinate system to another.
    ///
    /// Supported `PosType` conversions: `Event`, `Unicode`, `Utf16`, and `Bytes`.
    /// Returns `None` if the index is out of bounds or the conversion is unsupported.
    pub fn convert_pos(&self, index: usize, from: PosType, to: PosType) -> Option<usize> {
        if from == to {
            return Some(index);
        }

        if matches!(from, PosType::Entity) || matches!(to, PosType::Entity) {
            return None;
        }

        // Normalize to event + unicode indices for the given position.
        let (event_index, unicode_index) = match &self.inner {
            MaybeDetached::Detached(t) => {
                let t = t.lock();
                if index > t.value.len(from) {
                    return None;
                }
                let event_index = if from == PosType::Event {
                    index
                } else {
                    t.value.index_to_event_index(index, from)
                };
                let unicode_index = if from == PosType::Unicode {
                    index
                } else {
                    t.value.event_index_to_unicode_index(event_index)
                };
                (event_index, unicode_index)
            }
            MaybeDetached::Attached(a) if a.has_decoded_state() => {
                let res: Option<(usize, usize)> = a.with_state(|state| {
                    let state = state.as_richtext_state_mut().unwrap();
                    if index > state.len(from) {
                        return None;
                    }

                    let event_index = if from == PosType::Event {
                        index
                    } else {
                        state.index_to_event_index(index, from)
                    };
                    let unicode_index = if from == PosType::Unicode {
                        index
                    } else {
                        state.event_index_to_unicode_index(event_index)
                    };
                    Some((event_index, unicode_index))
                });

                res?
            }
            MaybeDetached::Attached(a) => {
                let value = a.get_value();
                let s = value.as_string().unwrap();
                let unicode_index = text_pos_to_unicode(s, index, from)?;
                let event_index = unicode_to_text_pos(s, unicode_index, PosType::Event)?;
                (event_index, unicode_index)
            }
        };

        let result = match to {
            PosType::Unicode => Some(unicode_index),
            PosType::Event => Some(event_index),
            PosType::Bytes | PosType::Utf16 => {
                // Map the event-index position onto the target coordinate via the
                // rope's prefix caches. This is O(log n); materializing the prefix
                // string would be O(n) and makes repeated edits O(n^2).
                match &self.inner {
                    MaybeDetached::Detached(t) => {
                        let t = t.lock();
                        if event_index > t.value.len_event() {
                            return None;
                        }
                        Some(t.value.event_index_to_index(event_index, to))
                    }
                    MaybeDetached::Attached(a) if a.has_decoded_state() => a.with_state(|state| {
                        let state = state.as_richtext_state_mut().unwrap();
                        if event_index > state.len_event() {
                            return None;
                        }
                        Some(state.event_index_to_index(event_index, to))
                    }),
                    MaybeDetached::Attached(a) => {
                        let value = a.get_value();
                        let s = value.as_string().unwrap();
                        unicode_to_text_pos(s, unicode_index, to)
                    }
                }
            }
            PosType::Entity => None,
        };
        result
    }
}

fn event_len(s: &str) -> usize {
    if cfg!(feature = "wasm") {
        count_utf16_len(s.as_bytes())
    } else {
        s.chars().count()
    }
}

fn text_len(s: &str, pos_type: PosType) -> Option<usize> {
    Some(match pos_type {
        PosType::Bytes => s.len(),
        PosType::Unicode => s.chars().count(),
        PosType::Utf16 => count_utf16_len(s.as_bytes()),
        PosType::Event => event_len(s),
        PosType::Entity => return None,
    })
}

fn text_pos_to_unicode(s: &str, index: usize, pos_type: PosType) -> Option<usize> {
    match pos_type {
        PosType::Unicode => (index <= s.chars().count()).then_some(index),
        PosType::Bytes => {
            if index > s.len() {
                None
            } else {
                Some(
                    s.char_indices()
                        .take_while(|(pos, c)| *pos + c.len_utf8() <= index)
                        .count(),
                )
            }
        }
        PosType::Utf16 => utf16_to_unicode_pos(s, index),
        PosType::Event if cfg!(feature = "wasm") => utf16_to_unicode_pos(s, index),
        PosType::Event => (index <= s.chars().count()).then_some(index),
        PosType::Entity => None,
    }
}

fn unicode_to_text_pos(s: &str, index: usize, pos_type: PosType) -> Option<usize> {
    match pos_type {
        PosType::Unicode => (index <= s.chars().count()).then_some(index),
        PosType::Bytes => unicode_to_byte_pos(s, index),
        PosType::Utf16 => unicode_to_utf16_pos(s, index),
        PosType::Event if cfg!(feature = "wasm") => unicode_to_utf16_pos(s, index),
        PosType::Event => (index <= s.chars().count()).then_some(index),
        PosType::Entity => None,
    }
}

fn unicode_to_byte_pos(s: &str, index: usize) -> Option<usize> {
    if index == 0 {
        return Some(0);
    }

    let mut unicode_pos = 0;
    for (byte_pos, _) in s.char_indices() {
        if unicode_pos == index {
            return Some(byte_pos);
        }
        unicode_pos += 1;
    }

    (unicode_pos == index).then_some(s.len())
}

fn unicode_to_utf16_pos(s: &str, index: usize) -> Option<usize> {
    let mut unicode_pos = 0;
    let mut utf16_pos = 0;
    if index == 0 {
        return Some(0);
    }

    for c in s.chars() {
        unicode_pos += 1;
        utf16_pos += c.len_utf16();
        if unicode_pos == index {
            return Some(utf16_pos);
        }
    }

    (unicode_pos == index).then_some(utf16_pos)
}

fn utf16_to_unicode_pos(s: &str, index: usize) -> Option<usize> {
    let mut unicode_pos = 0;
    let mut utf16_pos = 0;
    if index == 0 {
        return Some(0);
    }

    for c in s.chars() {
        let next_utf16_pos = utf16_pos + c.len_utf16();
        if index < next_utf16_pos {
            return Some(unicode_pos);
        }
        if index == next_utf16_pos {
            return Some(unicode_pos + 1);
        }
        utf16_pos = next_utf16_pos;
        unicode_pos += 1;
    }

    (index == utf16_pos).then_some(unicode_pos)
}

fn text_boundary_error(pos: usize, pos_type: PosType) -> LoroError {
    match pos_type {
        PosType::Bytes => LoroError::UTF8InUnicodeCodePoint { pos },
        PosType::Utf16 => LoroError::UTF16InUnicodeCodePoint { pos },
        PosType::Event if cfg!(feature = "wasm") => LoroError::UTF16InUnicodeCodePoint { pos },
        _ => LoroError::OutOfBound {
            pos,
            len: 0,
            info: format!("Position: {}:{}", file!(), line!()).into_boxed_str(),
        },
    }
}

fn text_char_at(s: &str, pos: usize, pos_type: PosType) -> LoroResult<char> {
    let len = text_len(s, pos_type).unwrap_or(0);
    if pos >= len {
        return Err(LoroError::OutOfBound {
            pos,
            len,
            info: format!("Position: {}:{}", file!(), line!()).into_boxed_str(),
        });
    }

    let unicode_pos =
        text_pos_to_unicode(s, pos, pos_type).ok_or_else(|| text_boundary_error(pos, pos_type))?;
    s.chars().nth(unicode_pos).ok_or(LoroError::OutOfBound {
        pos,
        len,
        info: format!("Position: {}:{}", file!(), line!()).into_boxed_str(),
    })
}

fn text_slice(s: &str, start: usize, end: usize, pos_type: PosType) -> LoroResult<String> {
    if end < start {
        return Err(LoroError::EndIndexLessThanStartIndex { start, end });
    }
    if start == end {
        return Ok(String::new());
    }

    let len = text_len(s, pos_type).unwrap_or(0);
    if end > len {
        return Err(LoroError::OutOfBound {
            pos: end,
            len,
            info: format!("Position: {}:{}", file!(), line!()).into_boxed_str(),
        });
    }

    let start = text_pos_to_unicode(s, start, pos_type)
        .ok_or_else(|| text_boundary_error(start, pos_type))?;
    let end =
        text_pos_to_unicode(s, end, pos_type).ok_or_else(|| text_boundary_error(end, pos_type))?;
    let start = unicode_to_byte_pos(s, start).expect("unicode index must map to a byte boundary");
    let end = unicode_to_byte_pos(s, end).expect("unicode index must map to a byte boundary");
    Ok(s[start..end].to_string())
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
                let mut list = l.lock();
                let len = list.value.len();
                if pos > len {
                    return Err(LoroError::OutOfBound {
                        pos,
                        info: format!("Position: {}:{}", file!(), line!()).into_boxed_str(),
                        len,
                    });
                }
                let value = v.into();
                ensure_no_regular_container_value(&value)?;
                list.value.insert(pos, ValueOrHandler::Value(value));
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
        ensure_no_regular_container_value(&v)?;

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
                let mut list = l.lock();
                let value = v.into();
                ensure_no_regular_container_value(&value)?;
                list.value.push(ValueOrHandler::Value(value));
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
                let mut list = l.lock();
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
                let mut list = l.lock();
                if pos > list.value.len() {
                    return Err(LoroError::OutOfBound {
                        pos,
                        info: format!("Position: {}:{}", file!(), line!()).into_boxed_str(),
                        len: list.value.len(),
                    });
                }
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
                let mut list = l.lock();
                let end = checked_range_end(
                    pos,
                    len,
                    list.value.len(),
                    || format!("Position: {}:{}", file!(), line!()).into_boxed_str(),
                )?;
                list.value.drain(pos..end);
                Ok(())
            }
            MaybeDetached::Attached(a) => a.with_txn(|txn| self.delete_with_txn(txn, pos, len)),
        }
    }

    pub fn delete_with_txn(&self, txn: &mut Transaction, pos: usize, len: usize) -> LoroResult<()> {
        if len == 0 {
            return Ok(());
        }

        let list_len = self.len();
        let end = checked_range_end(
            pos,
            len,
            list_len,
            || format!("Position: {}:{}", file!(), line!()).into_boxed_str(),
        )?;

        let inner = self.inner.try_attached_state()?;
        let ids: Vec<_> = inner.with_state(|state| {
            let list = state.as_list_state().unwrap();
            (pos..end).map(|i| list.get_id_at(i).unwrap()).collect()
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
                let list = l.lock();
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
            MaybeDetached::Attached(_) => {
                let Some(value) = self.get_(index) else {
                    return Err(LoroError::OutOfBound {
                        pos: index,
                        info: format!("Position: {}:{}", file!(), line!()).into_boxed_str(),
                        len: self.len(),
                    });
                };
                match value {
                    ValueOrHandler::Handler(handler) => Ok(handler),
                    ValueOrHandler::Value(value) => Err(LoroError::ArgErr(
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
            MaybeDetached::Detached(l) => l.lock().value.len(),
            MaybeDetached::Attached(a) => {
                a.with_doc_state(|state| state.get_list_len(a.container_idx))
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
            MaybeDetached::Detached(l) => l.lock().value.get(index).map(|x| x.to_value()),
            MaybeDetached::Attached(a) => {
                a.with_doc_state(|state| state.get_list_value_at(a.container_idx, index))
            }
        }
    }

    /// Get value at given index, if it's a container, return a handler to the container
    pub fn get_(&self, index: usize) -> Option<ValueOrHandler> {
        match &self.inner {
            MaybeDetached::Detached(l) => {
                let l = l.lock();
                l.value.get(index).cloned()
            }
            MaybeDetached::Attached(inner) => {
                let value = inner
                    .with_doc_state(|state| state.get_list_value_at(inner.container_idx, index));
                value.map(|value| value_to_value_or_handler(inner, value))
            }
        }
    }

    pub fn for_each<I>(&self, mut f: I)
    where
        I: FnMut(ValueOrHandler),
    {
        match &self.inner {
            MaybeDetached::Detached(l) => {
                let l = l.lock();
                for v in l.value.iter() {
                    f(v.clone())
                }
            }
            MaybeDetached::Attached(inner) => {
                let temp = inner.with_doc_state(|state| {
                    state
                        .get_list_values(inner.container_idx)
                        .into_iter()
                        .map(|value| value_to_value_or_handler(inner, value))
                        .collect::<Vec<_>>()
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
                let mut l = l.lock();
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
                let mut d = d.lock();
                if pos > d.value.len() {
                    return Err(LoroError::OutOfBound {
                        pos,
                        info: format!("Position: {}:{}", file!(), line!()).into_boxed_str(),
                        len: d.value.len(),
                    });
                }
                let value = v.into();
                ensure_no_regular_container_value(&value)?;
                d.value.insert(pos, ValueOrHandler::Value(value));
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

        ensure_no_regular_container_value(&v)?;

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
                let mut d = d.lock();
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
                let mut d = d.lock();
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
                let mut d = d.lock();
                Ok(d.value.pop())
            }
            MaybeDetached::Attached(a) => {
                if self.is_empty() {
                    return Ok(None);
                }
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
                let mut a = a.lock();
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
                let mut d = d.lock();
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
                let mut d = d.lock();
                if index >= d.value.len() {
                    return Err(LoroError::OutOfBound {
                        pos: index,
                        info: format!("Position: {}:{}", file!(), line!()).into_boxed_str(),
                        len: d.value.len(),
                    });
                }
                let value = value.into();
                ensure_no_regular_container_value(&value)?;
                d.value[index] = ValueOrHandler::Value(value);
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
        ensure_no_regular_container_value(&value)?;

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
                let mut d = d.lock();
                if pos >= d.value.len() {
                    return Err(LoroError::OutOfBound {
                        pos,
                        info: format!("Position: {}:{}", file!(), line!()).into_boxed_str(),
                        len: d.value.len(),
                    });
                }
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
                let mut d = d.lock();
                let end = checked_range_end(
                    pos,
                    len,
                    d.value.len(),
                    || format!("Position: {}:{}", file!(), line!()).into_boxed_str(),
                )?;
                d.value.drain(pos..end);
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

        let list_len = self.len();
        let end = checked_range_end(
            pos,
            len,
            list_len,
            || format!("Position: {}:{}", file!(), line!()).into_boxed_str(),
        )?;

        let (ids, new_poses) = self.with_state(|state| {
            let list = state.as_movable_list_state().unwrap();
            let ids: Vec<_> = (pos..end)
                .map(|i| list.get_list_id_at(i, IndexType::ForUser).unwrap())
                .collect();
            let poses: Vec<_> = (pos..end)
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
                let list = l.lock();
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
            MaybeDetached::Attached(_) => {
                let Some(value) = self.get_(index) else {
                    return Err(LoroError::OutOfBound {
                        pos: index,
                        info: format!("Position: {}:{}", file!(), line!()).into_boxed_str(),
                        len: self.len(),
                    });
                };
                match value {
                    ValueOrHandler::Handler(handler) => Ok(handler),
                    ValueOrHandler::Value(value) => Err(LoroError::ArgErr(
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
                let d = d.lock();
                d.value.len()
            }
            MaybeDetached::Attached(a) => {
                a.with_doc_state(|state| state.get_list_len(a.container_idx))
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
            .get_container_deep_value_with_id(inner.container_idx, None)
    }

    pub fn get(&self, index: usize) -> Option<LoroValue> {
        match &self.inner {
            MaybeDetached::Detached(d) => {
                let d = d.lock();
                d.value.get(index).map(|v| v.to_value())
            }
            MaybeDetached::Attached(a) => {
                a.with_doc_state(|state| state.get_list_value_at(a.container_idx, index))
            }
        }
    }

    /// Get value at given index, if it's a container, return a handler to the container
    pub fn get_(&self, index: usize) -> Option<ValueOrHandler> {
        match &self.inner {
            MaybeDetached::Detached(d) => {
                let d = d.lock();
                d.value.get(index).cloned()
            }
            MaybeDetached::Attached(m) => {
                let value =
                    m.with_doc_state(|state| state.get_list_value_at(m.container_idx, index));
                value.map(|value| value_to_value_or_handler(m, value))
            }
        }
    }

    pub fn for_each<I>(&self, mut f: I)
    where
        I: FnMut(ValueOrHandler),
    {
        match &self.inner {
            MaybeDetached::Detached(d) => {
                let d = d.lock();
                for v in d.value.iter() {
                    f(v.clone());
                }
            }
            MaybeDetached::Attached(m) => {
                let temp = m.with_doc_state(|state| {
                    state
                        .get_list_values(m.container_idx)
                        .into_iter()
                        .map(|value| value_to_value_or_handler(m, value))
                        .collect::<Vec<_>>()
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
                let d = d.lock();
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
                let mut d = d.lock();
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
                let mut m = m.lock();
                let value = value.into();
                ensure_no_regular_container_value(&value)?;
                m.value.insert(key.into(), ValueOrHandler::Value(value));
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
                let mut m = m.lock();
                let value = value.into();
                ensure_no_regular_container_value(&value)?;
                m.value.insert(key.into(), ValueOrHandler::Value(value));
                Ok(())
            }
            MaybeDetached::Attached(a) => a.with_txn(|txn| {
                let this = &self;
                let value = value.into();
                ensure_no_regular_container_value(&value)?;

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
        ensure_no_regular_container_value(&value)?;

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
                let mut m = m.lock();
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
                let mut m = m.lock();
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
                let m = m.lock();
                for (k, v) in m.value.iter() {
                    f(k, v.clone());
                }
            }
            MaybeDetached::Attached(inner) => {
                let temp = inner.with_doc_state(|state| {
                    state
                        .get_map_entries(inner.container_idx)
                        .into_iter()
                        .map(|(key, value)| {
                            let translated = loro_common::translate_mergeable_marker_value(
                                &inner.id,
                                key.as_ref(),
                                value,
                            );
                            (
                                key.to_string(),
                                value_to_value_or_handler(inner, translated),
                            )
                        })
                        .collect::<Vec<_>>()
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
                let m = m.lock();
                let value = m.value.get(key).unwrap();
                match value {
                    ValueOrHandler::Value(v) => Err(LoroError::ArgErr(
                        format!("Expected Handler but found {:?}", v).into_boxed_str(),
                    )),
                    ValueOrHandler::Handler(h) => Ok(h.clone()),
                }
            }
            MaybeDetached::Attached(_) => {
                let Some(value) = self.get_(key) else {
                    return Err(LoroError::ArgErr(
                        format!("Key {key} does not exist").into_boxed_str(),
                    ));
                };
                match value {
                    ValueOrHandler::Handler(handler) => Ok(handler),
                    ValueOrHandler::Value(value) => Err(LoroError::ArgErr(
                        format!("Expected Handler but found {:?}", value).into_boxed_str(),
                    )),
                }
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
                let m = m.lock();
                m.value.get(key).map(|v| v.to_value())
            }
            MaybeDetached::Attached(inner) => {
                let value = inner
                    .with_doc_state(|state| state.get_map_value_by_key(inner.container_idx, key))?;
                Some(loro_common::translate_mergeable_marker_value(
                    &inner.id, key, value,
                ))
            }
        }
    }

    /// Get the value at given key, if value is a container, return a handler to the container
    pub fn get_(&self, key: &str) -> Option<ValueOrHandler> {
        match &self.inner {
            MaybeDetached::Detached(m) => {
                let m = m.lock();
                m.value.get(key).cloned()
            }
            MaybeDetached::Attached(inner) => {
                let value = inner
                    .with_doc_state(|state| state.get_map_value_by_key(inner.container_idx, key))?;
                let value = loro_common::translate_mergeable_marker_value(&inner.id, key, value);
                Some(value_to_value_or_handler(inner, value))
            }
        }
    }

    /// Get or create a regular child container at `key`.
    ///
    /// This legacy method creates regular op-id child containers when the key is empty or `null`.
    /// It is not mergeable: concurrent first creation at the same map key can fork child state and
    /// leave one branch hidden by map conflict resolution. Prefer `ensure_mergeable_*` for lazy
    /// map-key child creation.
    #[deprecated(
        note = "use ensure_mergeable_map/list/movable_list/text/tree/counter for lazy map-key child creation; this method creates regular op-id children"
    )]
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

    /// Shared implementation for all `ensure_mergeable_*` methods.
    ///
    /// Computes a deterministic [`ContainerID::Root`] in the mergeable namespace from
    /// `(parent.id, key, child.kind())` and constructs the handler from it. Two peers calling this
    /// with the same `(parent, key, kind)` receive handlers with identical container ids, which is
    /// what makes the child container mergeable on concurrent first-write.
    ///
    /// # Errors
    ///
    /// Returns [`LoroError::MisuseDetachedContainer`] when called on a detached handler. The
    /// deterministic cid is computed from the parent's cid, which a detached parent does not have
    /// yet; falling back to a non-deterministic regular child would silently drop the mergeable
    /// guarantee at attach time. Detached callers must attach the parent first.
    ///
    /// Returns [`LoroError::ArgErr`] if the parent slot already holds a non-mergeable value, or if
    /// `C::from_handler` rejects the handler built from the deterministic cid (unreachable by
    /// construction; guards against future drift between `from_handler` and `kind`).
    fn ensure_mergeable_container<C: HandlerTrait>(&self, key: &str, child: C) -> LoroResult<C> {
        let MaybeDetached::Attached(parent) = &self.inner else {
            return Err(LoroError::MisuseDetachedContainer {
                method: "ensure_mergeable_container",
            });
        };

        // Compare against the raw marker bytes (skipping `MapHandler::get`'s user-facing
        // marker → Container translation) so the non-mergeable-occupant guard sees the real
        // slot value and the same-kind idempotent-skip can match.
        let existing_raw =
            parent.with_doc_state(|state| state.get_map_value_by_key(parent.container_idx, key));

        // A non-mergeable occupant (scalar, arbitrary binary, regular child container) would be
        // silently clobbered by the marker write, so reject rather than overwrite under a
        // `get_`-named API. Only the exact binary marker for this `(parent, key, kind)` is
        // accepted as an existing mergeable occupant.
        if let Some(existing) = &existing_raw {
            if !matches!(existing, LoroValue::Null)
                && loro_common::parse_mergeable_marker(&parent.id, key, existing).is_none()
            {
                return Err(LoroError::ArgErr(
                    format!(
                        "Cannot create a mergeable {} at key {key:?}: the key already holds a non-mergeable value",
                        child.kind()
                    )
                    .into_boxed_str(),
                ));
            }
        }

        let cid = ContainerID::new_mergeable(&parent.id, key, child.kind());
        let marker = loro_common::mergeable_marker(&parent.id, key, child.kind());

        // Idempotent-skip on same marker: `MapHandler::get` translates markers to Container, so
        // `insert_with_txn`'s equality check can't see this collision — do it directly.
        // A different-kind marker is a deliberate kind change; let the insert through.
        if existing_raw.as_ref() != Some(&marker) {
            self.insert(key, marker)?;
        }

        C::from_handler(create_handler(parent, cid.clone())).ok_or_else(|| {
            LoroError::ArgErr(
                format!(
                    "Expected value type {} but found {}",
                    child.kind(),
                    cid.container_type()
                )
                .into_boxed_str(),
            )
        })
    }

    #[cfg(feature = "counter")]
    /// Ensure a mergeable Counter child exists under `key` and return its handler.
    ///
    /// Returns [`LoroError::MisuseDetachedContainer`] when called on a detached map.
    /// Returns [`LoroError::ArgErr`] if the parent slot already holds a non-mergeable value.
    /// Repeated same-kind calls are idempotent; different mergeable kinds deliberately rewrite
    /// the active marker while preserving each mergeable child's deterministic state.
    pub fn ensure_mergeable_counter(&self, key: &str) -> LoroResult<counter::CounterHandler> {
        self.ensure_mergeable_container(key, counter::CounterHandler::new_detached())
    }

    /// Ensure a mergeable Map child exists under `key` and return its handler.
    ///
    /// Returns [`LoroError::MisuseDetachedContainer`] when called on a detached map.
    /// Returns [`LoroError::ArgErr`] if the parent slot already holds a non-mergeable value.
    /// Repeated same-kind calls are idempotent; different mergeable kinds deliberately rewrite
    /// the active marker while preserving each mergeable child's deterministic state.
    ///
    /// Prefer to avoid very deep mergeable-map chains: mergeable cids encode their flattened
    /// logical path, so cid size still grows with depth and rides through every op/snapshot
    /// reference to it. See [`MERGEABLE_NAMESPACE_PREFIX`](loro_common::MERGEABLE_NAMESPACE_PREFIX).
    pub fn ensure_mergeable_map(&self, key: &str) -> LoroResult<MapHandler> {
        self.ensure_mergeable_container(key, MapHandler::new_detached())
    }

    /// Ensure a mergeable List child exists under `key` and return its handler.
    ///
    /// Returns [`LoroError::MisuseDetachedContainer`] when called on a detached map.
    /// Returns [`LoroError::ArgErr`] if the parent slot already holds a non-mergeable value.
    /// Repeated same-kind calls are idempotent; different mergeable kinds deliberately rewrite
    /// the active marker while preserving each mergeable child's deterministic state.
    pub fn ensure_mergeable_list(&self, key: &str) -> LoroResult<ListHandler> {
        self.ensure_mergeable_container(key, ListHandler::new_detached())
    }

    /// Ensure a mergeable MovableList child exists under `key` and return its handler.
    ///
    /// Returns [`LoroError::MisuseDetachedContainer`] when called on a detached map.
    /// Returns [`LoroError::ArgErr`] if the parent slot already holds a non-mergeable value.
    /// Repeated same-kind calls are idempotent; different mergeable kinds deliberately rewrite
    /// the active marker while preserving each mergeable child's deterministic state.
    pub fn ensure_mergeable_movable_list(&self, key: &str) -> LoroResult<MovableListHandler> {
        self.ensure_mergeable_container(key, MovableListHandler::new_detached())
    }

    /// Ensure a mergeable Text child exists under `key` and return its handler.
    ///
    /// Returns [`LoroError::MisuseDetachedContainer`] when called on a detached map.
    /// Returns [`LoroError::ArgErr`] if the parent slot already holds a non-mergeable value.
    /// Repeated same-kind calls are idempotent; different mergeable kinds deliberately rewrite
    /// the active marker while preserving each mergeable child's deterministic state.
    pub fn ensure_mergeable_text(&self, key: &str) -> LoroResult<TextHandler> {
        self.ensure_mergeable_container(key, TextHandler::new_detached())
    }

    /// Ensure a mergeable Tree child exists under `key` and return its handler.
    ///
    /// Returns [`LoroError::MisuseDetachedContainer`] when called on a detached map.
    /// Returns [`LoroError::ArgErr`] if the parent slot already holds a non-mergeable value.
    /// Repeated same-kind calls are idempotent; different mergeable kinds deliberately rewrite
    /// the active marker while preserving each mergeable child's deterministic state.
    pub fn ensure_mergeable_tree(&self, key: &str) -> LoroResult<TreeHandler> {
        self.ensure_mergeable_container(key, TreeHandler::new_detached())
    }

    pub fn contains_key(&self, key: &str) -> bool {
        self.get(key).is_some()
    }

    pub fn len(&self) -> usize {
        match &self.inner {
            MaybeDetached::Detached(m) => m.lock().value.len(),
            MaybeDetached::Attached(a) => {
                a.with_doc_state(|state| state.get_map_len(a.container_idx))
            }
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
                let mut m = m.lock();
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
        let keys: Vec<InternalString> = match &self.inner {
            MaybeDetached::Detached(m) => {
                let m = m.lock();
                m.value.keys().map(|x| x.as_str().into()).collect()
            }
            MaybeDetached::Attached(a) => {
                a.with_doc_state(|state| state.get_map_keys(a.container_idx))
            }
        };

        keys.into_iter()
    }

    pub fn values(&self) -> impl Iterator<Item = ValueOrHandler> + '_ {
        let values: Vec<ValueOrHandler> = match &self.inner {
            MaybeDetached::Detached(m) => {
                let m = m.lock();
                m.value.values().cloned().collect()
            }
            MaybeDetached::Attached(a) => a.with_doc_state(|state| {
                // A mergeable child's marker lives in the parent map's value table; iterate
                // entries (key + value) so the user-boundary translation can resolve each marker
                // to the deterministic child cid before wrapping into a handler.
                state
                    .get_map_entries(a.container_idx)
                    .into_iter()
                    .map(|(key, value)| {
                        let translated = loro_common::translate_mergeable_marker_value(
                            &a.id,
                            key.as_ref(),
                            value,
                        );
                        value_to_value_or_handler(a, translated)
                    })
                    .collect()
            }),
        };

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
    let mut txn = txn.lock();
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
            txn = doc.txn.lock();
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
                    let d = &mut d.lock().value;
                    *d += n;
                    Ok(())
                }
                MaybeDetached::Attached(a) => a.with_txn(|txn| self.increment_with_txn(txn, n)),
            }
        }

        pub fn decrement(&self, n: f64) -> LoroResult<()> {
            match &self.inner {
                MaybeDetached::Detached(d) => {
                    let d = &mut d.lock().value;
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
                    let t = t.lock();
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
                    let mut v = v.lock();
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
                MaybeDetached::Detached(v) => v.lock().attached.clone().map(|x| Self {
                    inner: MaybeDetached::Attached(x),
                }),
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
    use std::borrow::Cow;

    use super::{
        Handler, HandlerTrait, ListHandler, MapHandler, MovableListHandler, TextDelta, TextHandler,
        ValueOrHandler,
    };
    use crate::container::list::list_op::ListOp;
    use crate::cursor::PosType;
    use crate::loro::ExportMode;
    use crate::op::ListSlice;
    use crate::state::TreeParentId;
    use crate::txn::EventHint;
    use crate::version::Frontiers;
    use crate::LoroDoc;
    use crate::{fx_map, ToJson};
    use loro_common::{ContainerID, ContainerType, LoroError, LoroValue, ID};
    use serde_json::json;

    fn recheck_fast_blob(mut bytes: Vec<u8>) -> Vec<u8> {
        let checksum = xxhash_rust::xxh32::xxh32(&bytes[20..], u32::from_le_bytes(*b"LORO"));
        bytes[16..20].copy_from_slice(&checksum.to_le_bytes());
        bytes
    }

    fn replace_fast_snapshot_state_bytes(mut snapshot: Vec<u8>, state_bytes: &[u8]) -> Vec<u8> {
        let mut body = &snapshot[22..];
        let oplog_len = u32::from_le_bytes(body[..4].try_into().unwrap()) as usize;
        body = &body[4 + oplog_len..];
        let old_state_len = u32::from_le_bytes(body[..4].try_into().unwrap()) as usize;
        let state_len_pos = 22 + 4 + oplog_len;
        let state_start = state_len_pos + 4;
        let state_end = state_start + old_state_len;
        snapshot[state_len_pos..state_start]
            .copy_from_slice(&(state_bytes.len() as u32).to_le_bytes());
        snapshot.splice(state_start..state_end, state_bytes.iter().copied());
        recheck_fast_blob(snapshot)
    }

    fn insert_many_with_single_list_op(
        txn: &mut crate::txn::Transaction,
        list: &crate::handler::ListHandler,
        pos: usize,
        values: Vec<LoroValue>,
    ) {
        let len = values.len();
        let inner = list.inner.try_attached_state().unwrap();
        txn.apply_local_op(
            inner.container_idx,
            crate::op::RawOpContent::List(ListOp::Insert {
                slice: ListSlice::RawData(Cow::Owned(values)),
                pos,
            }),
            EventHint::InsertList {
                len: len as u32,
                pos,
            },
            &inner.doc,
        )
        .unwrap();
    }

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
    fn cross_doc_txn_is_rejected() {
        // `insert_with_txn`/`delete_with_txn` are public API, so a transaction
        // from one document can be fed to another document's handler. That must
        // be rejected with `UnmatchedContext` rather than silently stamping the
        // target doc's state/oplog with the wrong peer+counter. Regression test
        // for the always-on (release included) context check in
        // `Transaction::apply_local_op`.
        let doc_a = LoroDoc::new();
        doc_a.set_peer_id(1).unwrap();
        let doc_b = LoroDoc::new();
        doc_b.set_peer_id(2).unwrap();

        // Seed doc_b so it has real state we can prove stays untouched.
        {
            let mut txn_b = doc_b.txn().unwrap();
            doc_b
                .get_text("text")
                .insert_with_txn(&mut txn_b, 0, "ok", PosType::Unicode)
                .unwrap();
            txn_b.commit().unwrap();
        }
        let vv_before = doc_b.oplog_vv();

        // Feed doc_a's transaction to doc_b's handler.
        let mut txn_a = doc_a.txn().unwrap();
        let text_b = doc_b.get_text("text");
        let insert_err = text_b
            .insert_with_txn(&mut txn_a, 0, "x", PosType::Unicode)
            .unwrap_err();
        assert!(matches!(insert_err, LoroError::UnmatchedContext { .. }));
        let delete_err = text_b
            .delete_with_txn(&mut txn_a, 0, 1, PosType::Unicode)
            .unwrap_err();
        assert!(matches!(delete_err, LoroError::UnmatchedContext { .. }));
        txn_a.commit().unwrap();

        // doc_b is unchanged: content and version vector identical.
        assert_eq!(&**text_b.get_value().as_string().unwrap(), "ok");
        assert_eq!(doc_b.oplog_vv(), vv_before);
    }

    #[test]
    fn list_import_batch_stays_consistent_after_repeated_tail_splits() {
        let doc_a = LoroDoc::new();
        doc_a.set_peer_id(1).unwrap();
        let mut txn = doc_a.txn().unwrap();
        let list_a = txn.get_list("list");
        insert_many_with_single_list_op(
            &mut txn,
            &list_a,
            0,
            (0..300).map(|i| LoroValue::I64(i)).collect(),
        );
        txn.commit().unwrap();

        let doc_b = LoroDoc::new();
        doc_b.set_peer_id(2).unwrap();
        doc_b
            .import(&doc_a.export(ExportMode::all_updates()).unwrap())
            .unwrap();

        let list_b = doc_b.get_list("list");
        let mut vv = doc_a.oplog_vv();
        let mut updates = Vec::new();
        for (i, pos) in [100, 201, 252, 278].into_iter().enumerate() {
            list_b.insert(pos, 1000 + i as i64).unwrap();
            updates.push(doc_b.export(ExportMode::updates(&vv)).unwrap());
            vv = doc_b.oplog_vv();
        }

        doc_a.import_batch(&updates).unwrap();
        doc_a.check_state_diff_calc_consistency_slow();
        doc_b.check_state_diff_calc_consistency_slow();
        assert_eq!(doc_a.get_deep_value(), doc_b.get_deep_value());
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
            .mark_with_txn(&mut txn, 0, 5, "bold", true.into(), PosType::Event)
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
    fn text_snapshot_string_queries_do_not_decode_state() {
        let loro = LoroDoc::new_auto_commit();
        let text = loro.get_text("text");
        text.insert(0, "a😀文", PosType::Unicode).unwrap();
        text.mark(1, 3, "bold", true.into(), PosType::Unicode)
            .unwrap();

        let restored = LoroDoc::new();
        restored
            .import(&loro.export(ExportMode::snapshot()).unwrap())
            .unwrap();
        let text = restored.get_text("text");
        assert!(!text.attached_handler().unwrap().has_decoded_state());

        assert_eq!(text.len_unicode(), 3);
        assert_eq!(text.len_utf16(), 4);
        assert_eq!(text.len_utf8(), "a😀文".len());
        assert_eq!(text.char_at(1, PosType::Unicode).unwrap(), '😀');
        assert_eq!(text.slice(1, 3, PosType::Unicode).unwrap(), "😀文");
        assert_eq!(
            text.convert_pos(2, PosType::Unicode, PosType::Utf16),
            Some(3)
        );
        assert!(matches!(
            text.delete_utf16(2, 1),
            Err(LoroError::UTF16InUnicodeCodePoint { pos: 2 })
        ));
        assert!(matches!(
            text.delete_utf8(2, 1),
            Err(LoroError::UTF8InUnicodeCodePoint { pos: 2 })
        ));
        assert!(matches!(
            text.slice_delta(2, 3, PosType::Utf16),
            Err(LoroError::UTF16InUnicodeCodePoint { pos: 2 })
        ));
        assert!(matches!(
            text.slice_delta(2, 3, PosType::Bytes),
            Err(LoroError::UTF8InUnicodeCodePoint { pos: 2 })
        ));
        assert!(!text.attached_handler().unwrap().has_decoded_state());

        assert_eq!(text.get_delta().len(), 2);
        assert!(text.attached_handler().unwrap().has_decoded_state());
    }

    #[test]
    fn text_lazy_event_queries_match_decoded_state() {
        let loro = LoroDoc::new_auto_commit();
        let text = loro.get_text("text");
        text.insert(0, "ab😀cd", PosType::Unicode).unwrap();
        text.mark(1, 4, "bold", true.into(), PosType::Unicode)
            .unwrap();
        text.mark(2, 3, "link", "x".into(), PosType::Unicode)
            .unwrap();

        let lazy_doc = LoroDoc::new();
        lazy_doc
            .import(&loro.export(ExportMode::snapshot()).unwrap())
            .unwrap();
        let lazy_text = lazy_doc.get_text("text");

        let decoded_doc = LoroDoc::new();
        decoded_doc
            .import(&loro.export(ExportMode::snapshot()).unwrap())
            .unwrap();
        let decoded_text = decoded_doc.get_text("text");
        decoded_text.get_delta();

        assert!(!lazy_text.attached_handler().unwrap().has_decoded_state());
        assert!(decoded_text.attached_handler().unwrap().has_decoded_state());

        for pos_type in [
            PosType::Event,
            PosType::Unicode,
            PosType::Utf16,
            PosType::Bytes,
        ] {
            assert_eq!(lazy_text.len(pos_type), decoded_text.len(pos_type));
            for pos in 0..=decoded_text.len(pos_type) {
                assert_eq!(
                    lazy_text.convert_pos(pos, pos_type, PosType::Unicode),
                    decoded_text.convert_pos(pos, pos_type, PosType::Unicode),
                    "convert {pos_type:?} pos {pos} to unicode"
                );
                assert_eq!(
                    lazy_text.convert_pos(pos, pos_type, PosType::Event),
                    decoded_text.convert_pos(pos, pos_type, PosType::Event),
                    "convert {pos_type:?} pos {pos} to event"
                );
                if pos < decoded_text.len(pos_type) {
                    assert_eq!(
                        lazy_text.char_at(pos, pos_type),
                        decoded_text.char_at(pos, pos_type),
                        "char_at {pos_type:?} pos {pos}"
                    );
                }
                for end in pos..=decoded_text.len(pos_type) {
                    assert_eq!(
                        lazy_text.slice(pos, end, pos_type),
                        decoded_text.slice(pos, end, pos_type),
                        "slice {pos_type:?} {pos}..{end}"
                    );
                }
            }
        }
    }

    #[test]
    fn deep_value_with_id_uses_lazy_values_for_snapshot_roots() {
        let loro = LoroDoc::new_auto_commit();
        let text = loro.get_text("text");
        text.insert(0, "hello", PosType::Unicode).unwrap();
        let map = loro.get_map("map");
        map.insert("key", "value").unwrap();
        let list = loro.get_list("list");
        list.push("item").unwrap();

        let restored = LoroDoc::new();
        restored
            .import(&loro.export(ExportMode::snapshot()).unwrap())
            .unwrap();
        let text = restored.get_text("text");
        let map = restored.get_map("map");
        let list = restored.get_list("list");

        let value = restored.get_deep_value_with_id();
        assert_eq!(value["text"]["value"], "hello".into());
        assert_eq!(value["map"]["value"]["key"], "value".into());
        assert_eq!(value["list"]["value"][0], "item".into());
        assert!(!text.attached_handler().unwrap().has_decoded_state());
        assert!(!map.attached_handler().unwrap().has_decoded_state());
        assert!(!list.attached_handler().unwrap().has_decoded_state());
    }

    #[test]
    fn lazy_value_reads_do_not_write_stale_snapshot_after_mutation() {
        let loro = LoroDoc::new_auto_commit();
        let map = loro.get_map("map");
        map.insert("key", "old").unwrap();
        let child = map
            .insert_container("child", MapHandler::new_detached())
            .unwrap();
        child.insert("nested", "old").unwrap();
        let list = loro.get_list("list");
        list.push("old").unwrap();
        let child_list = list.push_container(ListHandler::new_detached()).unwrap();
        child_list.push("nested-old").unwrap();

        let restored = LoroDoc::new();
        restored
            .import(&loro.export(ExportMode::snapshot()).unwrap())
            .unwrap();
        let map = restored.get_map("map");
        let list = restored.get_list("list");

        assert_eq!(map.get("key").unwrap(), "old".into());
        assert_eq!(list.get(0).unwrap(), "old".into());
        let child = match map.get_("child").unwrap() {
            ValueOrHandler::Handler(handler) => handler.into_map().unwrap(),
            ValueOrHandler::Value(value) => panic!("expected child map, got {value:?}"),
        };
        let child_list = match list.get_(1).unwrap() {
            ValueOrHandler::Handler(handler) => handler.into_list().unwrap(),
            ValueOrHandler::Value(value) => panic!("expected child list, got {value:?}"),
        };

        map.insert("key", "new").unwrap();
        child.insert("nested", "new").unwrap();
        list.delete(0, 1).unwrap();
        list.insert(0, "new").unwrap();
        child_list.delete(0, 1).unwrap();
        child_list.insert(0, "nested-new").unwrap();
        restored.commit_then_renew();

        let roundtrip = LoroDoc::new();
        roundtrip
            .import(&restored.export(ExportMode::snapshot()).unwrap())
            .unwrap();
        assert_eq!(
            roundtrip.get_deep_value().to_json_value(),
            serde_json::json!({
                "map": { "key": "new", "child": { "nested": "new" } },
                "list": ["new", ["nested-new"]]
            })
        );
    }

    #[test]
    fn fast_snapshot_with_trailing_bytes_is_rejected_on_import() {
        let loro = LoroDoc::new_auto_commit();
        let map = loro.get_map("map");
        map.insert("key", "value").unwrap();
        let mut snapshot = loro.export(ExportMode::snapshot()).unwrap();
        snapshot.push(0xff);
        let corrupted = recheck_fast_blob(snapshot);

        let doc = LoroDoc::new();
        assert!(doc.import(&corrupted).is_err());
    }

    #[test]
    fn fast_snapshot_with_trailing_bytes_is_rejected_by_meta_decoder() {
        let loro = LoroDoc::new_auto_commit();
        let map = loro.get_map("map");
        map.insert("key", "value").unwrap();
        let mut snapshot = loro.export(ExportMode::snapshot()).unwrap();
        snapshot.push(0xff);
        let corrupted = recheck_fast_blob(snapshot);

        assert!(LoroDoc::decode_import_blob_meta(&corrupted, true).is_err());
    }

    #[test]
    fn fast_snapshot_empty_sstable_meta_is_rejected_on_import() {
        let loro = LoroDoc::new_auto_commit();
        let map = loro.get_map("map");
        map.insert("key", "value").unwrap();
        let snapshot = loro.export(ExportMode::snapshot()).unwrap();

        let mut malformed_state = Vec::new();
        malformed_state.extend_from_slice(b"LORO");
        malformed_state.push(0);
        malformed_state.extend_from_slice(&0u32.to_le_bytes());
        let checksum = xxhash_rust::xxh32::xxh32(&[], u32::from_le_bytes(*b"LORO"));
        malformed_state.extend_from_slice(&checksum.to_le_bytes());
        malformed_state.extend_from_slice(&5u32.to_le_bytes());
        let corrupted = replace_fast_snapshot_state_bytes(snapshot, &malformed_state);

        let doc = LoroDoc::new();
        assert!(doc.import(&corrupted).is_err());
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

    #[test]
    fn richtext_apply_delta_marks_without_growth() {
        let loro = LoroDoc::new_auto_commit();
        let text = loro.get_text("text");
        text.insert(0, "abc", PosType::Unicode).unwrap();

        text.apply_delta(&[TextDelta::Retain {
            retain: 3,
            attributes: Some(fx_map!("bold".into() => LoroValue::Bool(true))),
        }])
        .unwrap();
        loro.commit_then_renew();

        assert_eq!(text.to_string(), "abc");
        assert_eq!(
            text.get_richtext_value().to_json_value(),
            json!([{"insert": "abc", "attributes": {"bold": true}}])
        );
    }

    #[test]
    fn richtext_apply_delta_grows_for_mark_gap() {
        let loro = LoroDoc::new_auto_commit();
        let text = loro.get_text("text");

        text.apply_delta(&[TextDelta::Retain {
            retain: 1,
            attributes: Some(fx_map!("bold".into() => LoroValue::Bool(true))),
        }])
        .unwrap();
        loro.commit_then_renew();

        assert_eq!(text.to_string(), "\n");
        assert_eq!(
            text.get_richtext_value().to_json_value(),
            json!([{"insert": "\n", "attributes": {"bold": true}}])
        );
    }

    #[test]
    fn richtext_apply_delta_ignores_empty_inserts() {
        let loro = LoroDoc::new_auto_commit();
        let text = loro.get_text("text");
        text.insert(0, "seed", PosType::Unicode).unwrap();

        text.apply_delta(&[TextDelta::Insert {
            insert: "".into(),
            attributes: Some(fx_map!("bold".into() => LoroValue::Bool(true))),
        }])
        .unwrap();
        loro.commit_then_renew();

        assert_eq!(text.to_string(), "seed");
        assert_eq!(
            text.get_richtext_value().to_json_value(),
            json!([{"insert": "seed"}])
        );
    }

    #[test]
    fn handler_trait_dispatch_reports_attached_container_identity() {
        let loro = LoroDoc::new_auto_commit();
        let handlers = [
            (loro.get_text("text").to_handler(), ContainerType::Text),
            (loro.get_map("map").to_handler(), ContainerType::Map),
            (loro.get_list("list").to_handler(), ContainerType::List),
            (
                loro.get_movable_list("movable").to_handler(),
                ContainerType::MovableList,
            ),
            (loro.get_tree("tree").to_handler(), ContainerType::Tree),
        ];

        for (handler, expected_type) in handlers {
            assert!(handler.is_attached());
            assert!(handler.attached_handler().is_some());
            assert!(handler.doc().is_some());
            assert!(handler.get_attached().is_some());
            assert_eq!(handler.kind(), expected_type);
            assert_eq!(handler.c_type(), expected_type);
            assert_eq!(handler.id().container_type(), expected_type);
            assert_eq!(
                Handler::from_handler(handler.clone()).unwrap().c_type(),
                expected_type
            );

            handler.get_value();
            handler.get_deep_value();
            handler.clear().unwrap();
        }
    }

    #[test]
    fn handler_trait_dispatch_reports_detached_container_identity() {
        let handlers = [
            (
                Handler::new_unattached(ContainerType::Text),
                ContainerType::Text,
            ),
            (
                Handler::new_unattached(ContainerType::Map),
                ContainerType::Map,
            ),
            (
                Handler::new_unattached(ContainerType::List),
                ContainerType::List,
            ),
            (
                Handler::new_unattached(ContainerType::MovableList),
                ContainerType::MovableList,
            ),
            (
                Handler::new_unattached(ContainerType::Tree),
                ContainerType::Tree,
            ),
        ];

        for (handler, expected_type) in handlers {
            assert!(!handler.is_attached());
            assert!(handler.attached_handler().is_none());
            assert!(handler.doc().is_none());
            assert!(handler.get_attached().is_none());
            assert_eq!(handler.kind(), expected_type);
            assert_eq!(handler.c_type(), expected_type);
            assert_eq!(handler.id().container_type(), expected_type);
            assert_eq!(handler.idx().get_type(), expected_type);
            assert_eq!(
                Handler::from_handler(handler.clone()).unwrap().c_type(),
                expected_type
            );
        }
    }

    #[test]
    fn attaching_detached_handlers_sets_parent_and_attached_back_reference() {
        let loro = LoroDoc::new_auto_commit();

        let map = loro.get_map("map");
        let detached_text = TextHandler::new_detached();
        detached_text
            .insert(0, "detached", PosType::Unicode)
            .unwrap();
        let attached_text = map.insert_container("text", detached_text.clone()).unwrap();
        assert!(attached_text.is_attached());
        assert_eq!(attached_text.to_string(), "detached");
        assert_eq!(attached_text.parent().unwrap().c_type(), ContainerType::Map);
        assert_eq!(
            detached_text.get_attached().unwrap().id(),
            attached_text.id()
        );

        let list = loro.get_list("list");
        let detached_map = MapHandler::new_detached();
        detached_map.insert("k", 1_i64).unwrap();
        let attached_map = list.insert_container(0, detached_map.clone()).unwrap();
        assert!(attached_map.is_attached());
        assert_eq!(attached_map.parent().unwrap().c_type(), ContainerType::List);
        assert_eq!(detached_map.get_attached().unwrap().id(), attached_map.id());

        let movable = loro.get_movable_list("movable");
        let detached_list = ListHandler::new_detached();
        detached_list.push("item").unwrap();
        let attached_list = movable.insert_container(0, detached_list.clone()).unwrap();
        assert!(attached_list.is_attached());
        assert_eq!(
            attached_list.parent().unwrap().c_type(),
            ContainerType::MovableList
        );
        assert_eq!(
            detached_list.get_attached().unwrap().id(),
            attached_list.id()
        );

        let nested = attached_map
            .insert_container("movable", MovableListHandler::new_detached())
            .unwrap();
        assert_eq!(nested.parent().unwrap().id(), attached_map.id());
    }

    #[test]
    fn unknown_handler_reports_identity_without_materializing_value() {
        let loro = LoroDoc::new_auto_commit();
        let id = ContainerID::Root {
            name: "unknown".into(),
            container_type: ContainerType::Unknown(7),
        };
        let handler = Handler::new_attached(id.clone(), loro.clone());
        let unknown = handler.as_unknown().unwrap();

        assert!(unknown.is_attached());
        assert_eq!(unknown.kind(), ContainerType::Unknown(7));
        assert_eq!(unknown.id(), id);
        assert_eq!(unknown.to_handler().c_type(), ContainerType::Unknown(7));
        assert!(unknown.doc().is_some());
        assert!(!unknown.is_deleted());
        assert_eq!(format!("{unknown:?}"), "UnknownHandler");
        assert!(unknown.get_attached().is_some());
        assert!(super::UnknownHandler::from_handler(handler).is_some());
    }
}

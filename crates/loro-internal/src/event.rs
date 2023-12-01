use enum_as_inner::EnumAsInner;
use fxhash::{FxHashMap, FxHasher64};
use serde::{Deserialize, Serialize};
use smallvec::SmallVec;

use crate::{
    arena::SharedArena,
    container::richtext::richtext_state::RichtextStateChunk,
    delta::{
        Delta, DeltaItem, MapDelta, ResolvedMapDelta, ResolvedMapValue, StyleMeta, TreeDelta,
        TreeDiff,
    },
    handler::{Handler, ValueOrContainer},
    op::SliceRanges,
    txn::Transaction,
    utils::string_slice::StringSlice,
    DocState, InternalString, LoroValue,
};

use std::{
    borrow::Cow,
    hash::{Hash, Hasher},
    sync::{Mutex, Weak},
};

use loro_common::{ContainerID, TreeID};

use crate::{container::idx::ContainerIdx, version::Frontiers};

#[derive(Debug, Clone)]
pub struct ContainerDiff {
    pub id: ContainerID,
    pub path: Vec<(ContainerID, Index)>,
    pub(crate) idx: ContainerIdx,
    pub diff: Diff,
}

#[derive(Debug, Clone)]
pub struct UnresolvedContainerDiff {
    pub id: ContainerID,
    pub path: Vec<(ContainerID, Index)>,
    pub(crate) idx: ContainerIdx,
    pub diff: UnresolvedDiff,
}

#[derive(Debug, Clone)]
pub struct DiffEvent<'a> {
    /// whether the event comes from the children of the container.
    pub from_children: bool,
    pub container: &'a ContainerDiff,
    pub doc: &'a DocDiff,
}

#[derive(Debug, Clone)]
pub struct UnresolvedDocDiff {
    pub from: Frontiers,
    pub to: Frontiers,
    pub origin: InternalString,
    pub local: bool,
    /// Whether the diff is created from the checkout operation.
    pub from_checkout: bool,
    pub diff: Vec<UnresolvedContainerDiff>,
}

/// It's the exposed event type.
/// It's exposed to the user. The user can use this to apply the diff to their local state.
///
/// [DocDiff] may include the diff that calculated from several transactions and imports.
/// They all should have the same origin and local flag.
#[derive(Debug, Clone)]
pub struct DocDiff {
    pub from: Frontiers,
    pub to: Frontiers,
    pub origin: InternalString,
    pub local: bool,
    /// Whether the diff is created from the checkout operation.
    pub from_checkout: bool,
    pub diff: Vec<ContainerDiff>,
}

impl DocDiff {
    /// Get the unique id of the diff.
    pub fn id(&self) -> u64 {
        let mut hasher = FxHasher64::default();
        self.from.hash(&mut hasher);
        self.to.hash(&mut hasher);
        hasher.finish()
    }

    pub(crate) fn from_unsolved_diff(
        diff: UnresolvedDocDiff,
        state: &Weak<Mutex<DocState>>,
        arena: &SharedArena,
        txn: &Weak<Mutex<Option<Transaction>>>,
    ) -> Self {
        DocDiff {
            from: diff.from,
            to: diff.to,
            origin: diff.origin,
            local: diff.local,
            from_checkout: diff.from_checkout,
            diff: diff
                .diff
                .into_iter()
                .map(|uc| ContainerDiff {
                    id: uc.id,
                    path: uc.path,
                    idx: uc.idx,
                    diff: external_diff_to_resolved(uc.diff, state, arena, txn),
                })
                .collect::<Vec<_>>(),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct InternalContainerDiff {
    pub(crate) idx: ContainerIdx,
    // If true, this event is created by the container which was resurrected by another container
    pub(crate) bring_back: bool,
    pub(crate) is_container_deleted: bool,
    pub(crate) diff: Option<DiffVariant>,
}

#[derive(Debug, Clone, EnumAsInner)]
pub(crate) enum DiffVariant {
    Internal(InternalDiff),
    External(UnresolvedDiff),
}

/// It's used for transmitting and recording the diff internally.
///
/// It can be convert into a [DocDiff].
// Internally, we need to batch the diff then calculate the event. Because
// we need to sort the diff by containers' created time, to make sure the
// the path to each container is up-to-date.
#[derive(Debug, Clone)]
pub(crate) struct InternalDocDiff<'a> {
    pub(crate) origin: InternalString,
    pub(crate) local: bool,
    pub(crate) from_checkout: bool,
    pub(crate) diff: Cow<'a, [InternalContainerDiff]>,
    pub(crate) new_version: Cow<'a, Frontiers>,
}

impl<'a> InternalDocDiff<'a> {
    pub fn into_owned(self) -> InternalDocDiff<'static> {
        InternalDocDiff {
            origin: self.origin,
            local: self.local,
            from_checkout: self.from_checkout,
            diff: Cow::Owned((*self.diff).to_owned()),
            new_version: Cow::Owned((*self.new_version).to_owned()),
        }
    }

    pub fn can_merge(&self, other: &Self) -> bool {
        self.origin == other.origin && self.local == other.local
    }
}

pub type Path = SmallVec<[Index; 4]>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Index {
    Key(InternalString),
    Seq(usize),
    Node(TreeID),
}

impl DiffVariant {
    pub fn compose(self, other: Self) -> Result<Self, Self> {
        match (self, other) {
            (DiffVariant::Internal(a), DiffVariant::Internal(b)) => {
                Ok(DiffVariant::Internal(a.compose(b)?))
            }
            (DiffVariant::External(a), DiffVariant::External(b)) => {
                Ok(DiffVariant::External(a.compose(b)?))
            }
            (a, _) => Err(a),
        }
    }
}

#[non_exhaustive]
#[derive(Clone, Debug, EnumAsInner, Serialize)]
pub(crate) enum InternalDiff {
    SeqRaw(Delta<SliceRanges>),
    /// This always uses entity indexes.
    RichtextRaw(Delta<RichtextStateChunk>),
    Map(MapDelta),
    Tree(TreeDelta),
}

impl From<UnresolvedDiff> for DiffVariant {
    fn from(diff: UnresolvedDiff) -> Self {
        DiffVariant::External(diff)
    }
}

impl From<InternalDiff> for DiffVariant {
    fn from(diff: InternalDiff) -> Self {
        DiffVariant::Internal(diff)
    }
}

/// Diff is the diff between two versions of a container.
/// It's used to describe the change of a container and the events.
///
/// # Internal
///
/// Text index variants:
///
/// - When `wasm` is enabled, it should use utf16 indexes.
/// - When `wasm` is disabled, it should use unicode indexes.
#[non_exhaustive]
#[derive(Clone, Debug, EnumAsInner, Serialize)]
pub enum UnresolvedDiff {
    List(Delta<Vec<LoroValue>>),
    /// - When feature `wasm` is enabled, it should use utf16 indexes.
    /// - When feature `wasm` is disabled, it should use unicode indexes.
    Text(Delta<StringSlice, StyleMeta>),
    Map(MapDelta),
    Tree(TreeDiff),
}

#[non_exhaustive]
#[derive(Clone, Debug, EnumAsInner)]
pub enum Diff {
    List(Delta<Vec<ValueOrContainer>>),
    /// - When feature `wasm` is enabled, it should use utf16 indexes.
    /// - When feature `wasm` is disabled, it should use unicode indexes.
    Text(Delta<StringSlice, StyleMeta>),
    Map(ResolvedMapDelta),
    Tree(TreeDiff),
}

impl InternalDiff {
    pub(crate) fn is_empty(&self) -> bool {
        match self {
            InternalDiff::SeqRaw(s) => s.is_empty(),
            InternalDiff::RichtextRaw(t) => t.is_empty(),
            InternalDiff::Map(m) => m.updated.is_empty(),
            InternalDiff::Tree(t) => t.is_empty(),
        }
    }

    pub(crate) fn compose(self, diff: InternalDiff) -> Result<Self, Self> {
        // PERF: avoid clone
        match (self, diff) {
            (InternalDiff::SeqRaw(a), InternalDiff::SeqRaw(b)) => {
                Ok(InternalDiff::SeqRaw(a.compose(b)))
            }
            (InternalDiff::RichtextRaw(a), InternalDiff::RichtextRaw(b)) => {
                Ok(InternalDiff::RichtextRaw(a.compose(b)))
            }
            (InternalDiff::Map(a), InternalDiff::Map(b)) => Ok(InternalDiff::Map(a.compose(b))),
            (InternalDiff::Tree(a), InternalDiff::Tree(b)) => Ok(InternalDiff::Tree(a.compose(b))),
            (a, _) => Err(a),
        }
    }
}

impl UnresolvedDiff {
    pub(crate) fn compose(self, diff: UnresolvedDiff) -> Result<UnresolvedDiff, Self> {
        // PERF: avoid clone
        match (self, diff) {
            (UnresolvedDiff::List(a), UnresolvedDiff::List(b)) => {
                Ok(UnresolvedDiff::List(a.compose(b)))
            }
            (UnresolvedDiff::Text(a), UnresolvedDiff::Text(b)) => {
                Ok(UnresolvedDiff::Text(a.compose(b)))
            }
            (UnresolvedDiff::Map(a), UnresolvedDiff::Map(b)) => {
                Ok(UnresolvedDiff::Map(a.compose(b)))
            }

            (UnresolvedDiff::Tree(a), UnresolvedDiff::Tree(b)) => {
                Ok(UnresolvedDiff::Tree(a.compose(b)))
            }
            (a, _) => Err(a),
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        match self {
            UnresolvedDiff::List(s) => s.is_empty(),
            UnresolvedDiff::Text(t) => t.is_empty(),
            UnresolvedDiff::Map(m) => m.updated.is_empty(),
            UnresolvedDiff::Tree(t) => t.diff.is_empty(),
        }
    }

    pub(crate) fn concat(self, diff: UnresolvedDiff) -> UnresolvedDiff {
        match (self, diff) {
            (UnresolvedDiff::List(a), UnresolvedDiff::List(b)) => {
                UnresolvedDiff::List(a.compose(b))
            }
            (UnresolvedDiff::Text(a), UnresolvedDiff::Text(b)) => {
                UnresolvedDiff::Text(a.compose(b))
            }
            (UnresolvedDiff::Map(a), UnresolvedDiff::Map(b)) => {
                let mut a = a;
                for (k, v) in b.updated {
                    a = a.with_entry(k, v);
                }
                UnresolvedDiff::Map(a)
            }

            (UnresolvedDiff::Tree(a), UnresolvedDiff::Tree(b)) => {
                UnresolvedDiff::Tree(a.extend(b.diff))
            }
            _ => unreachable!(),
        }
    }
}

pub(crate) fn external_diff_to_resolved(
    diff: UnresolvedDiff,
    state: &Weak<Mutex<DocState>>,
    arena: &SharedArena,
    txn: &Weak<Mutex<Option<Transaction>>>,
) -> Diff {
    match diff {
        UnresolvedDiff::List(list) => {
            let vec = list
                .vec
                .into_iter()
                .map(|item| match item {
                    DeltaItem::Insert { insert, attributes } => {
                        let insert = insert
                            .into_iter()
                            .map(|v| {
                                if let LoroValue::Container(c) = v {
                                    let idx = arena.id_to_idx(&c).unwrap();
                                    ValueOrContainer::Container(Handler::new(
                                        txn.clone(),
                                        idx,
                                        state.clone(),
                                    ))
                                } else {
                                    ValueOrContainer::Value(v)
                                }
                            })
                            .collect();
                        DeltaItem::Insert { insert, attributes }
                    }
                    DeltaItem::Delete { delete, attributes } => {
                        DeltaItem::Delete { delete, attributes }
                    }
                    DeltaItem::Retain { retain, attributes } => {
                        DeltaItem::Retain { retain, attributes }
                    }
                })
                .collect();
            Diff::List(Delta { vec })
        }
        UnresolvedDiff::Map(map) => {
            let mut resolved_map = FxHashMap::default();
            for (k, v) in map.updated.into_iter() {
                let counter = v.counter;
                let lamport = v.lamport;
                let value = v.value.map(|v| {
                    if let LoroValue::Container(c) = v {
                        let idx = arena.id_to_idx(&c).unwrap();
                        ValueOrContainer::Container(Handler::new(txn.clone(), idx, state.clone()))
                    } else {
                        ValueOrContainer::Value(v)
                    }
                });
                resolved_map.insert(
                    k,
                    ResolvedMapValue {
                        counter,
                        value,
                        lamport,
                    },
                );
            }
            Diff::Map(ResolvedMapDelta {
                updated: resolved_map,
            })
        }
        UnresolvedDiff::Text(t) => Diff::Text(t),
        UnresolvedDiff::Tree(t) => Diff::Tree(t),
    }
}

#[cfg(test)]
mod test {
    use std::sync::Arc;

    use loro_common::LoroValue;

    use crate::{ApplyDiff, LoroDoc};

    #[test]
    fn test_text_event() {
        let loro = LoroDoc::new();
        loro.subscribe_root(Arc::new(|event| {
            let mut value = LoroValue::String(Default::default());
            value.apply_diff(&[event.container.diff.clone()]);
            assert_eq!(value, "h223ello".into());
        }));
        let mut txn = loro.txn().unwrap();
        let text = loro.get_text("id");
        text.insert_with_txn(&mut txn, 0, "hello").unwrap();
        text.insert_with_txn(&mut txn, 1, "223").unwrap();
        txn.commit().unwrap();
    }
}

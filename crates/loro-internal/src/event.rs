use enum_as_inner::EnumAsInner;
use fxhash::FxHasher64;
use serde::{Deserialize, Serialize};
use smallvec::SmallVec;

use crate::{
    container::richtext::richtext_state::RichtextStateChunk,
    delta::{Delta, MapDelta, ResolvedMapDelta, StyleMeta, TreeDelta, TreeDiff},
    handler::ValueOrContainer,
    op::SliceRanges,
    utils::string_slice::StringSlice,
    InternalString, LoroValue,
};

use std::{
    borrow::Cow,
    hash::{Hash, Hasher},
};

use loro_common::{ContainerID, TreeID};

use crate::{container::idx::ContainerIdx, version::Frontiers};

#[derive(Debug, Clone)]
pub struct ContainerDiff {
    pub id: ContainerID,
    pub path: Vec<(ContainerID, Index)>,
    pub(crate) idx: ContainerIdx,
    pub diff: ResolvedDiff,
}

#[derive(Debug, Clone)]
pub struct DiffEvent<'a> {
    /// whether the event comes from the children of the container.
    pub from_children: bool,
    pub container: &'a ContainerDiff,
    pub doc: &'a DocDiff,
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
    External(Diff),
    Resolved(ResolvedDiff),
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

impl From<Diff> for DiffVariant {
    fn from(diff: Diff) -> Self {
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
pub enum Diff {
    List(Delta<Vec<LoroValue>>),
    /// - When feature `wasm` is enabled, it should use utf16 indexes.
    /// - When feature `wasm` is disabled, it should use unicode indexes.
    Text(Delta<StringSlice, StyleMeta>),
    NewMap(MapDelta),
    Tree(TreeDiff),
}

#[non_exhaustive]
#[derive(Clone, Debug, EnumAsInner)]
pub enum ResolvedDiff {
    List(Delta<Vec<ValueOrContainer>>),
    /// - When feature `wasm` is enabled, it should use utf16 indexes.
    /// - When feature `wasm` is disabled, it should use unicode indexes.
    Text(Delta<StringSlice, StyleMeta>),
    NewMap(ResolvedMapDelta),
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

impl Diff {
    pub(crate) fn compose(self, diff: Diff) -> Result<Diff, Self> {
        // PERF: avoid clone
        match (self, diff) {
            (Diff::List(a), Diff::List(b)) => Ok(Diff::List(a.compose(b))),
            (Diff::Text(a), Diff::Text(b)) => Ok(Diff::Text(a.compose(b))),
            (Diff::NewMap(a), Diff::NewMap(b)) => Ok(Diff::NewMap(a.compose(b))),

            (Diff::Tree(a), Diff::Tree(b)) => Ok(Diff::Tree(a.compose(b))),
            (a, _) => Err(a),
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        match self {
            Diff::List(s) => s.is_empty(),
            Diff::Text(t) => t.is_empty(),
            Diff::NewMap(m) => m.updated.is_empty(),
            Diff::Tree(t) => t.diff.is_empty(),
        }
    }

    pub(crate) fn concat(self, diff: Diff) -> Diff {
        match (self, diff) {
            (Diff::List(a), Diff::List(b)) => Diff::List(a.compose(b)),
            (Diff::Text(a), Diff::Text(b)) => Diff::Text(a.compose(b)),
            (Diff::NewMap(a), Diff::NewMap(b)) => {
                let mut a = a;
                for (k, v) in b.updated {
                    a = a.with_entry(k, v);
                }
                Diff::NewMap(a)
            }

            (Diff::Tree(a), Diff::Tree(b)) => Diff::Tree(a.extend(b.diff)),
            _ => unreachable!(),
        }
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

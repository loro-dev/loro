use enum_as_inner::EnumAsInner;
use fxhash::FxHasher64;
use itertools::Itertools;
use loro_delta::{array_vec::ArrayVec, delta_trait::DeltaAttr, DeltaItem, DeltaRope};
use serde::{Deserialize, Serialize};
use smallvec::SmallVec;

use crate::{
    container::richtext::richtext_state::RichtextStateChunk,
    delta::{
        Delta, MapDelta, Meta, MovableListInnerDelta, ResolvedMapDelta, StyleMeta, TreeDelta,
        TreeDiff,
    },
    handler::ValueOrHandler,
    op::SliceRanges,
    utils::string_slice::StringSlice,
    InternalString,
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
    pub is_unknown: bool,
    pub diff: Diff,
}

/// The kind of the event trigger.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EventTriggerKind {
    /// The event is triggered by a local transaction.
    Local,
    /// The event is triggered by importing
    Import,
    /// The event is triggered by checkout
    Checkout,
}

impl std::fmt::Display for EventTriggerKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EventTriggerKind::Local => write!(f, "local"),
            EventTriggerKind::Import => write!(f, "import"),
            EventTriggerKind::Checkout => write!(f, "checkout"),
        }
    }
}

impl EventTriggerKind {
    #[inline]
    pub fn is_local(&self) -> bool {
        matches!(self, EventTriggerKind::Local)
    }

    #[inline]
    pub fn is_import(&self) -> bool {
        matches!(self, EventTriggerKind::Import)
    }

    #[inline]
    pub fn is_checkout(&self) -> bool {
        matches!(self, EventTriggerKind::Checkout)
    }
}

#[derive(Debug, Clone)]
pub struct DiffEvent<'a> {
    /// The receiver of the event.
    pub current_target: Option<ContainerID>,
    /// A list of events that should be received by the current target.
    pub events: &'a [&'a ContainerDiff],
    pub event_meta: &'a DocDiff,
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
    pub by: EventTriggerKind,
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
    pub(crate) diff: DiffVariant,
}

#[derive(Default, Debug, Clone, EnumAsInner)]
pub(crate) enum DiffVariant {
    #[default]
    None,
    Internal(InternalDiff),
    External(Diff),
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
    pub(crate) by: EventTriggerKind,
    /// The values inside this array is in random order
    pub(crate) diff: Cow<'a, [InternalContainerDiff]>,
    pub(crate) new_version: Cow<'a, Frontiers>,
}

impl<'a> InternalDocDiff<'a> {
    pub fn into_owned(self) -> InternalDocDiff<'static> {
        InternalDocDiff {
            origin: self.origin,
            by: self.by,
            diff: Cow::Owned((*self.diff).to_owned()),
            new_version: Cow::Owned((*self.new_version).to_owned()),
        }
    }

    pub fn can_merge(&self, other: &Self) -> bool {
        self.by == other.by
    }
}

pub type Path = SmallVec<[Index; 4]>;

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, enum_as_inner::EnumAsInner)]
pub enum Index {
    Key(InternalString),
    Seq(usize),
    Node(TreeID),
}

impl std::fmt::Debug for Index {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Key(arg0) => write!(f, "Index::Key(\"{}\")", arg0),
            Self::Seq(arg0) => write!(f, "Index::Seq({})", arg0),
            Self::Node(arg0) => write!(f, "Index::Node({})", arg0),
        }
    }
}

impl std::fmt::Display for Index {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Index::Key(key) => write!(f, "{}", key),
            Index::Seq(s) => write!(f, "{}", s),
            Index::Node(id) => write!(f, "{}@{}", id.peer, id.counter),
        }
    }
}

impl TryFrom<&str> for Index {
    type Error = &'static str;
    fn try_from(s: &str) -> Result<Self, &'static str> {
        if s.is_empty() {
            return Ok(Index::Key(InternalString::default()));
        }

        let c = s.chars().next().unwrap();
        if c.is_ascii_digit() {
            if let Ok(seq) = s.parse::<usize>() {
                Ok(Index::Seq(seq))
            } else if let Ok(id) = s.try_into() {
                Ok(Index::Node(id))
            } else {
                Ok(Index::Key(InternalString::from(s)))
            }
        } else {
            Ok(Index::Key(InternalString::from(s)))
        }
    }
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
#[derive(Clone, Debug, EnumAsInner)]
pub(crate) enum InternalDiff {
    ListRaw(Delta<SliceRanges>),
    /// This always uses entity indexes.
    RichtextRaw(Delta<RichtextStateChunk>),
    Map(MapDelta),
    Tree(TreeDelta),
    MovableList(MovableListInnerDelta),
    #[cfg(feature = "counter")]
    Counter(i64),
    Unknown,
}

impl From<InternalDiff> for DiffVariant {
    fn from(diff: InternalDiff) -> Self {
        DiffVariant::Internal(diff)
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ListDeltaMeta {
    /// Whether the content of the insert is moved from
    /// a deletion in the same delta and **the value is not changed**.
    ///
    /// If true, this op must be a move op under the hood.
    /// But an insert created by a move op doesn't necessarily
    /// have this flag, because the insert content may not
    /// be moved from a deletion in the same delta.
    pub from_move: bool,
}

impl Meta for ListDeltaMeta {
    fn is_empty(&self) -> bool {
        !self.from_move
    }

    fn compose(
        &mut self,
        other: &Self,
        type_pair: (crate::delta::DeltaType, crate::delta::DeltaType),
    ) {
        // We can't have two Some because we don't have `move_from` for Retain.
        // And this function is only called when composing a insert/retain with a retain.
        if let (crate::delta::DeltaType::Insert, crate::delta::DeltaType::Insert) = type_pair {
            unreachable!()
        }

        self.from_move = self.from_move || other.from_move;
    }

    fn is_mergeable(&self, other: &Self) -> bool {
        self.from_move == other.from_move
    }

    fn merge(&mut self, _other: &Self) {}
}

impl DeltaAttr for ListDeltaMeta {
    fn compose(&mut self, other: &Self) {
        self.from_move = self.from_move || other.from_move;
    }

    fn attr_is_empty(&self) -> bool {
        !self.from_move
    }
}

pub type ListDiffInsertItem = ArrayVec<ValueOrHandler, 8>;
pub type ListDiffItem = DeltaItem<ListDiffInsertItem, ListDeltaMeta>;
pub type ListDiff = DeltaRope<ListDiffInsertItem, ListDeltaMeta>;

pub type TextDiffItem = DeltaItem<StringSlice, StyleMeta>;
pub type TextDiff = DeltaRope<StringSlice, StyleMeta>;

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
#[derive(Clone, Debug, EnumAsInner)]
pub enum Diff {
    List(ListDiff),
    // TODO: refactor, doesn't make much sense to use `StyleMeta` here, because sometime style
    // don't have peer and lamport info
    /// - When feature `wasm` is enabled, it should use utf16 indexes.
    /// - When feature `wasm` is disabled, it should use unicode indexes.
    Text(TextDiff),
    Map(ResolvedMapDelta),
    Tree(TreeDiff),
    #[cfg(feature = "counter")]
    Counter(i64),
    Unknown,
}

impl From<Diff> for DiffVariant {
    fn from(diff: Diff) -> Self {
        DiffVariant::External(diff)
    }
}

impl InternalDiff {
    pub(crate) fn is_empty(&self) -> bool {
        match self {
            InternalDiff::ListRaw(s) => s.is_empty(),
            InternalDiff::RichtextRaw(t) => t.is_empty(),
            InternalDiff::Map(m) => m.updated.is_empty(),
            InternalDiff::Tree(t) => t.is_empty(),
            InternalDiff::MovableList(t) => t.is_empty(),
            #[cfg(feature = "counter")]
            InternalDiff::Counter(c) => c.is_zero(),
            InternalDiff::Unknown => true,
        }
    }

    pub(crate) fn compose(self, diff: InternalDiff) -> Result<Self, Self> {
        // PERF: avoid clone
        match (self, diff) {
            (InternalDiff::ListRaw(a), InternalDiff::ListRaw(b)) => {
                Ok(InternalDiff::ListRaw(a.compose(b)))
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
    pub(crate) fn compose(self, diff: Diff) -> Result<Self, Self> {
        // PERF: avoid clone
        match (self, diff) {
            (Diff::List(mut a), Diff::List(b)) => {
                a.compose(&b);
                Ok(Diff::List(a))
            }
            (Diff::Text(mut a), Diff::Text(b)) => {
                a.compose(&b);
                Ok(Diff::Text(a))
            }
            (Diff::Map(a), Diff::Map(b)) => Ok(Diff::Map(a.compose(b))),

            (Diff::Tree(a), Diff::Tree(b)) => Ok(Diff::Tree(a.compose(b))),
            #[cfg(feature = "counter")]
            (Diff::Counter(a), Diff::Counter(b)) => Ok(Diff::Counter(a + b)),
            (a, _) => Err(a),
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        match self {
            Diff::List(s) => s.is_empty(),
            Diff::Text(t) => t.is_empty(),
            Diff::Map(m) => m.updated.is_empty(),
            Diff::Tree(t) => t.diff.is_empty(),
            #[cfg(feature = "counter")]
            Diff::Counter(c) => *c == 0,
            Diff::Unknown => true,
        }
    }

    #[allow(unused)]
    pub(crate) fn concat(self, diff: Diff) -> Diff {
        match (self, diff) {
            (Diff::List(mut a), Diff::List(b)) => {
                a.compose(&b);
                Diff::List(a)
            }
            (Diff::Text(mut a), Diff::Text(b)) => {
                a.compose(&b);
                Diff::Text(a)
            }
            (Diff::Map(a), Diff::Map(b)) => {
                let mut a = a;
                for (k, v) in b.updated {
                    a = a.with_entry(k, v);
                }
                Diff::Map(a)
            }

            (Diff::Tree(a), Diff::Tree(b)) => Diff::Tree(a.extend(b.diff)),
            #[cfg(feature = "counter")]
            (Diff::Counter(a), Diff::Counter(b)) => Diff::Counter(a + b),
            _ => unreachable!(),
        }
    }
}

pub fn str_to_path(s: &str) -> Option<Vec<Index>> {
    s.split('/').map(|x| x.try_into()).try_collect().ok()
}

pub fn path_to_str(path: &[Index]) -> String {
    path.iter().map(|x| x.to_string()).join("/")
}

#[cfg(test)]
mod test {
    use std::sync::Arc;

    use itertools::Itertools;
    use loro_common::LoroValue;

    use crate::{ApplyDiff, LoroDoc};

    #[test]
    fn test_text_event() {
        let loro = LoroDoc::new();
        loro.subscribe_root(Arc::new(|event| {
            let mut value = LoroValue::String(Default::default());
            value.apply_diff(&event.events.iter().map(|x| x.diff.clone()).collect_vec());
            assert_eq!(value, "h223ello".into());
        }));
        let mut txn = loro.txn().unwrap();
        let text = loro.get_text("id");
        text.insert_with_txn(&mut txn, 0, "hello").unwrap();
        text.insert_with_txn(&mut txn, 1, "223").unwrap();
        txn.commit().unwrap();
    }
}

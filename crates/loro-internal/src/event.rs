use enum_as_inner::EnumAsInner;
use serde::{Deserialize, Serialize};
use smallvec::SmallVec;

use crate::{
    delta::{Delta, MapDelta, MapDiff, TreeDelta},
    text::text_content::SliceRanges,
    InternalString, LoroValue,
};

use std::borrow::Cow;

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
    pub diff: Vec<ContainerDiff>,
}

#[derive(Debug, Clone)]
pub(crate) struct InternalContainerDiff {
    pub(crate) idx: ContainerIdx,
    pub(crate) diff: Diff,
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
    pub(crate) diff: Cow<'a, [InternalContainerDiff]>,
    pub(crate) new_version: Cow<'a, Frontiers>,
}

impl<'a> InternalDocDiff<'a> {
    pub fn into_owned(self) -> InternalDocDiff<'static> {
        InternalDocDiff {
            origin: self.origin,
            local: self.local,
            diff: Cow::Owned((*self.diff).to_owned()),
            new_version: Cow::Owned((*self.new_version).to_owned()),
        }
    }

    pub fn can_merge(&self, other: &Self) -> bool {
        self.origin == other.origin && self.local == other.local
    }
}

#[cfg(test)]
mod test {
    use std::sync::Arc;

    use crate::LoroDoc;

    #[test]
    fn test_text_event() {
        let loro = LoroDoc::new();
        loro.subscribe_deep(Arc::new(|event| {
            assert_eq!(
                &event.container.diff.as_text().unwrap().vec[0]
                    .as_insert()
                    .unwrap()
                    .0,
                &"h223ello"
            );
            dbg!(event);
        }));
        let mut txn = loro.txn().unwrap();
        let text = loro.get_text("id");
        text.insert(&mut txn, 0, "hello").unwrap();
        text.insert(&mut txn, 1, "223").unwrap();
        txn.commit().unwrap();
    }
}

pub type Path = SmallVec<[Index; 4]>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Index {
    Key(InternalString),
    Seq(usize),
    Node(TreeID),
}

/// Diff is the diff between two versions of a container.
/// It's used to describe the change of a container and the events.
///
/// # Internal
///
/// SeqRaw & SeqRawUtf16 is internal stuff, it should not be exposed to user.
/// The len inside SeqRaw uses utf8 for Text by default.
///
/// Text always uses platform specific indexes:
///
/// - When `wasm` is enabled, it should use utf16 indexes.
/// - When `wasm` is disabled, it should use utf8 indexes.
#[derive(Clone, Debug, EnumAsInner, Serialize)]
pub enum Diff {
    List(Delta<Vec<LoroValue>>),
    SeqRaw(Delta<SliceRanges>),
    SeqRawUtf16(Delta<SliceRanges>),
    Text(Delta<String>),
    /// @deprecated
    Map(MapDiff<LoroValue>),
    NewMap(MapDelta),
    Tree(TreeDelta),
}

impl Diff {
    pub(crate) fn compose(self, diff: Diff) -> Result<Diff, Self> {
        // PERF: avoid clone
        match (self, diff) {
            (Diff::List(a), Diff::List(b)) => Ok(Diff::List(a.compose(b))),
            (Diff::SeqRaw(a), Diff::SeqRaw(b)) => Ok(Diff::SeqRaw(a.compose(b))),
            (Diff::Text(a), Diff::Text(b)) => Ok(Diff::Text(a.compose(b))),
            (Diff::Map(a), Diff::Map(b)) => Ok(Diff::Map(a.compose(b))),
            (Diff::NewMap(a), Diff::NewMap(b)) => Ok(Diff::NewMap(a.compose(b))),

            (Diff::Tree(a), Diff::Tree(b)) => Ok(Diff::Tree(a.compose(b))),
            (a, _) => Err(a),
        }
    }
}

impl Default for Diff {
    fn default() -> Self {
        Diff::List(Delta::default())
    }
}

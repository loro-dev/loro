use enum_as_inner::EnumAsInner;
use serde::{Deserialize, Serialize};
use smallvec::SmallVec;

use crate::{
    delta::{Delta, MapDelta, MapDiff},
    text::text_content::SliceRanges,
    InternalString, LoroValue,
};

pub type Path = SmallVec<[Index; 4]>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Index {
    Key(InternalString),
    Seq(usize),
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
            (a, _) => Err(a),
        }
    }
}

impl Default for Diff {
    fn default() -> Self {
        Diff::List(Delta::default())
    }
}

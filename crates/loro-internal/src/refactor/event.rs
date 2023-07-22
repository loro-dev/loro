use std::borrow::Cow;

use loro_common::ContainerID;

use crate::{
    container::registry::ContainerIdx,
    event::{Diff, Index},
    version::Frontiers,
    InternalString,
};

#[derive(Debug, Clone)]
pub struct ContainerDiff {
    pub id: ContainerID,
    pub path: Vec<(ContainerID, Index)>,
    pub(crate) idx: ContainerIdx,
    pub diff: Diff,
}

#[derive(Debug)]
pub struct DiffEvent<'a> {
    pub container: &'a ContainerDiff,
    pub doc: &'a DocDiff,
}

/// It's the exposed event type.
/// It's exposed to the user. The user can use this to apply the diff to their local state.
///
/// DocDiff may include the diff that calculated from several transactions and imports.
/// They all should have the same origin and local flag.
#[derive(Debug, Clone)]
pub struct DocDiff {
    pub from: Frontiers,
    pub to: Frontiers,
    pub origin: InternalString,
    pub local: bool,
    pub diff: Vec<ContainerDiff>,
    // state: Weak<Mutex<DocState>>,
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

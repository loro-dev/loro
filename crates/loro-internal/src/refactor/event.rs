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

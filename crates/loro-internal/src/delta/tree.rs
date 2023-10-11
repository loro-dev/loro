use loro_common::TreeID;
use serde::Serialize;

#[derive(Debug, Clone, Default, Serialize)]
pub struct TreeDelta {
    pub(crate) diff: Vec<(TreeID, Option<TreeID>)>,
}

// TODO: tree
pub enum TreeDiff {
    Create,
    Move(Option<TreeID>),
    Delete,
}

impl TreeDelta {
    // TODO:
    pub(crate) fn compose(&self, x: TreeDelta) -> TreeDelta {
        todo!();
    }
}

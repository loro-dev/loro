use loro_common::TreeID;
use serde::Serialize;

#[derive(Debug, Clone, Default, Serialize)]
pub struct TreeDelta {
    pub(crate) diff: Vec<TreeDiff>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TreeDiff {
    pub target: TreeID,
    pub action: TreeDiffItem,
}

// TODO: tree
#[derive(Debug, Clone, Serialize)]
pub enum TreeDiffItem {
    CreateOrRestore,
    Move(TreeID),
    Delete,
}

impl From<(TreeID, Option<TreeID>)> for TreeDiff {
    fn from(value: (TreeID, Option<TreeID>)) -> Self {
        let (target, parent) = value;
        let action = if parent.is_some() {
            if TreeID::is_deleted(parent) {
                TreeDiffItem::Delete
            } else {
                TreeDiffItem::Move(parent.unwrap())
            }
        } else {
            TreeDiffItem::CreateOrRestore
        };
        TreeDiff { target, action }
    }
}

impl TreeDelta {
    // TODO:
    pub(crate) fn compose(&self, x: TreeDelta) -> TreeDelta {
        todo!();
    }
}

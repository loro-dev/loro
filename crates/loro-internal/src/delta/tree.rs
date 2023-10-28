use loro_common::TreeID;
use serde::Serialize;

/// Representation of differences in movable tree. It's an ordered list of [`TreeDiff`].
#[derive(Debug, Clone, Default, Serialize)]
pub struct TreeDelta {
    pub(crate) diff: Vec<TreeDiff>,
}

/// The semantic action in movable tree.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct TreeDiff {
    pub target: TreeID,
    pub action: TreeDiffItem,
}

/// The action of [`TreeDiff`]. It's the same as  [`crate::container::tree::tree_op::TreeOp`], but semantic.
#[derive(Debug, Clone, Copy, Serialize)]
pub enum TreeDiffItem {
    CreateOrRestore,
    Move(TreeID),
    Delete,
    UnCreate,
}

impl From<(TreeID, Option<TreeID>)> for TreeDiff {
    fn from(value: (TreeID, Option<TreeID>)) -> Self {
        let (target, parent) = value;
        let action = if let Some(p) = parent {
            if TreeID::is_deleted_root(parent) {
                TreeDiffItem::Delete
            } else if TreeID::is_unexist_root(parent) {
                TreeDiffItem::UnCreate
            } else {
                TreeDiffItem::Move(p)
            }
        } else {
            TreeDiffItem::CreateOrRestore
        };
        TreeDiff { target, action }
    }
}

impl TreeDelta {
    // TODO: cannot handle this for now
    pub(crate) fn compose(&self, _x: TreeDelta) -> TreeDelta {
        unimplemented!("tree compose")
    }

    pub(crate) fn push(mut self, diff: TreeDiff) -> Self {
        self.diff.push(diff);
        self
    }
}

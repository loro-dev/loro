use loro_common::TreeID;
use rle::{HasLength, Mergable, Sliceable};
use serde::{Deserialize, Serialize};

use crate::state::TreeParentId;

/// The operation of movable tree.
///
/// In the movable tree, there are three actions:
/// - **Create**: target tree id will be generated by [`Transaction`], and parent tree id is `None`.
/// - **Move**: move target tree node a child node of the specified parent node.
/// - **Delete**: move target tree node to [`loro_common::DELETED_TREE_ROOT`].
///
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct TreeOp {
    pub(crate) target: TreeID,
    pub(crate) parent: Option<TreeID>,
}

impl TreeOp {
    // TODO: use `TreeParentId` instead of `Option<TreeID>`
    pub(crate) fn parent_id(&self) -> TreeParentId {
        match self.parent {
            Some(parent) => {
                if TreeID::is_deleted_root(&parent) {
                    TreeParentId::Deleted
                } else {
                    TreeParentId::Node(parent)
                }
            }
            None => TreeParentId::None,
        }
    }
}

impl HasLength for TreeOp {
    fn content_len(&self) -> usize {
        1
    }
}

impl Sliceable for TreeOp {
    fn slice(&self, from: usize, to: usize) -> Self {
        assert!(from == 0 && to == 1);
        *self
    }
}

impl Mergable for TreeOp {}

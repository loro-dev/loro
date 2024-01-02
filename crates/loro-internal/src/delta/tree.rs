use std::{
    ops::{Deref, DerefMut},
    sync::Arc,
};

use fxhash::{FxHashMap, FxHashSet};
use loro_common::{ContainerType, LoroValue, TreeID, ID};
use serde::Serialize;
use smallvec::{smallvec, SmallVec};

#[derive(Debug, Clone, Default, Serialize)]
pub struct TreeDiff {
    pub(crate) diff: Vec<TreeDiffItem>,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct TreeDiffItem {
    pub target: TreeID,
    pub action: TreeExternalDiff,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub enum TreeExternalDiff {
    Create,
    Move(Option<TreeID>),
    Delete,
}

impl TreeDiffItem {
    pub(crate) fn from_delta_item(item: TreeDeltaItem) -> SmallVec<[TreeDiffItem; 2]> {
        let target = item.target;
        match item.action {
            TreeInternalDiff::Create | TreeInternalDiff::Restore => {
                smallvec![TreeDiffItem {
                    target,
                    action: TreeExternalDiff::Create
                }]
            }
            TreeInternalDiff::AsRoot => {
                smallvec![TreeDiffItem {
                    target,
                    action: TreeExternalDiff::Move(None)
                }]
            }
            TreeInternalDiff::Move(p) => {
                smallvec![TreeDiffItem {
                    target,
                    action: TreeExternalDiff::Move(Some(p))
                }]
            }
            TreeInternalDiff::CreateMove(p) | TreeInternalDiff::RestoreMove(p) => {
                smallvec![
                    TreeDiffItem {
                        target,
                        action: TreeExternalDiff::Create
                    },
                    TreeDiffItem {
                        target,
                        action: TreeExternalDiff::Move(Some(p))
                    }
                ]
            }
            TreeInternalDiff::Delete | TreeInternalDiff::UnCreate => {
                smallvec![TreeDiffItem {
                    target,
                    action: TreeExternalDiff::Delete
                }]
            }
        }
    }
}

impl TreeDiff {
    pub(crate) fn compose(self, _other: Self) -> Self {
        unreachable!("tree compose")
    }

    pub(crate) fn extend<I: IntoIterator<Item = TreeDiffItem>>(mut self, other: I) -> Self {
        self.diff.extend(other);
        self
    }
}

/// Representation of differences in movable tree. It's an ordered list of [`TreeDiff`].
#[derive(Debug, Clone, Default, Serialize)]
pub struct TreeDelta {
    pub(crate) diff: Vec<TreeDeltaItem>,
}

/// The semantic action in movable tree.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct TreeDeltaItem {
    pub target: TreeID,
    pub action: TreeInternalDiff,
    pub last_effective_move_op_id: ID,
}

/// The action of [`TreeDiff`]. It's the same as  [`crate::container::tree::tree_op::TreeOp`], but semantic.
#[derive(Debug, Clone, Copy, Serialize)]
pub enum TreeInternalDiff {
    /// First create the node, have not seen it before
    Create,
    /// Recreate the node, the node has been deleted before
    Restore,
    /// Same as move to `None` and the node exists
    AsRoot,
    /// Move the node to the parent, the node exists
    Move(TreeID),
    /// First create the node and move it to the parent
    CreateMove(TreeID),
    /// Recreate the node, and move it to the parent
    RestoreMove(TreeID),
    /// Delete the node
    Delete,
    /// For retreating, if the node is only created, not move it to `DELETED_ROOT` but delete it directly
    UnCreate,
}

impl TreeDeltaItem {
    pub(crate) fn new(
        target: TreeID,
        parent: Option<TreeID>,
        old_parent: Option<TreeID>,
        op_id: ID,
        is_parent_deleted: bool,
        is_old_parent_deleted: bool,
    ) -> Self {
        let action = match (parent, old_parent) {
            (Some(p), _) => {
                if is_parent_deleted {
                    TreeInternalDiff::Delete
                } else if TreeID::is_unexist_root(parent) {
                    TreeInternalDiff::UnCreate
                } else if TreeID::is_unexist_root(old_parent) {
                    TreeInternalDiff::CreateMove(p)
                } else if is_old_parent_deleted {
                    TreeInternalDiff::RestoreMove(p)
                } else {
                    TreeInternalDiff::Move(p)
                }
            }
            (None, Some(_)) => {
                if TreeID::is_unexist_root(old_parent) {
                    TreeInternalDiff::Create
                } else if is_old_parent_deleted {
                    TreeInternalDiff::Restore
                } else {
                    TreeInternalDiff::AsRoot
                }
            }
            (None, None) => {
                unreachable!()
            }
        };
        TreeDeltaItem {
            target,
            action,
            last_effective_move_op_id: op_id,
        }
    }
}

impl Deref for TreeDelta {
    type Target = Vec<TreeDeltaItem>;
    fn deref(&self) -> &Self::Target {
        &self.diff
    }
}

impl TreeDelta {
    // TODO: cannot handle this for now
    pub(crate) fn compose(&self, _x: TreeDelta) -> TreeDelta {
        unimplemented!("tree compose")
    }
}

#[derive(Debug)]
pub(crate) struct TreeValue<'a>(pub(crate) &'a mut Vec<LoroValue>);

impl<'a> TreeValue<'a> {
    pub(crate) fn apply_diff(&mut self, diff: &TreeDiff) {
        for d in diff.diff.iter() {
            let target = d.target;
            match d.action {
                TreeExternalDiff::Create => self.create_target(target),
                TreeExternalDiff::Delete => self.delete_target(target),
                TreeExternalDiff::Move(parent) => self.mov(target, parent),
            }
        }
    }

    fn mov(&mut self, target: TreeID, parent: Option<TreeID>) {
        let map = self
            .0
            .iter_mut()
            .find(|x| {
                let id = x.as_map().unwrap().get("id").unwrap().as_string().unwrap();
                id.as_ref() == &target.to_string()
            })
            .unwrap()
            .as_map_mut()
            .unwrap();
        let map_mut = Arc::make_mut(map);
        let p = if let Some(p) = parent {
            p.to_string().into()
        } else {
            LoroValue::Null
        };
        map_mut.insert("parent".to_string(), p);
    }

    fn create_target(&mut self, target: TreeID) {
        let mut t = FxHashMap::default();
        t.insert("id".to_string(), target.id().to_string().into());
        t.insert("parent".to_string(), LoroValue::Null);
        t.insert("meta".to_string(), ContainerType::Map.default_value());
        self.0.push(t.into());
    }

    fn delete_target(&mut self, target: TreeID) {
        let mut deleted = FxHashSet::default();
        let mut s = vec![target.to_string()];
        while let Some(delete) = s.pop() {
            deleted.insert(delete.clone());
            self.0.retain_mut(|x| {
                let id = x.as_map().unwrap().get("id").unwrap().as_string().unwrap();
                !deleted.contains(id.as_ref())
            });
            for node in self.0.iter() {
                let node = node.as_map().unwrap().as_ref();
                if let Some(LoroValue::String(parent)) = node.get("parent") {
                    if parent.as_ref() == &delete {
                        s.push((*node.get("id").unwrap().as_string().unwrap().clone()).clone());
                    }
                }
            }
        }
    }
}

impl Deref for TreeDiff {
    type Target = Vec<TreeDiffItem>;
    fn deref(&self) -> &Self::Target {
        &self.diff
    }
}

impl DerefMut for TreeDiff {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.diff
    }
}

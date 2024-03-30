use std::{
    ops::{Deref, DerefMut},
    sync::Arc,
};

use fxhash::{FxHashMap, FxHashSet};
use loro_common::{ContainerType, IdFull, LoroValue, TreeID};
use serde::Serialize;

use crate::{container::tree::fractional_index::FracIndex, state::TreeParentId};

#[derive(Debug, Clone, Default, Serialize)]
pub struct TreeDiff {
    pub diff: Vec<TreeDiffItem>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TreeDiffItem {
    pub target: TreeID,
    pub action: TreeExternalDiff,
}

#[derive(Debug, Clone, Serialize)]
pub enum TreeExternalDiff {
    Create {
        parent: Option<TreeID>,
        position: FracIndex,
    },
    Move {
        parent: Option<TreeID>,
        position: FracIndex,
    },
    Delete,
}

// impl TreeDiffItem {
//     pub(crate) fn from_delta_item(item: TreeDeltaItem) -> Option<TreeDiffItem> {
//         let target = item.target;
//         match item.action {
//             TreeInternalDiff::Create(p) => Some(TreeDiffItem {
//                 target,
//                 action: TreeExternalDiff::Create(p.into_node().ok()),
//             }),
//             TreeInternalDiff::Move(p) => Some(TreeDiffItem {
//                 target,
//                 action: TreeExternalDiff::Move(p.into_node().ok()),
//             }),
//             TreeInternalDiff::Delete(_) | TreeInternalDiff::UnCreate => Some(TreeDiffItem {
//                 target,
//                 action: TreeExternalDiff::Delete,
//             }),
//             TreeInternalDiff::MoveInDelete(_) => None,
//         }
//     }
// }

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
#[derive(Debug, Clone, Default)]
pub struct TreeDelta {
    pub(crate) diff: Vec<TreeDeltaItem>,
}

/// The semantic action in movable tree.
#[derive(Debug, Clone)]
pub struct TreeDeltaItem {
    pub target: TreeID,
    pub action: TreeInternalDiff,
    pub last_effective_move_op_id: IdFull,
}

/// The action of [`TreeDiff`]. It's the same as  [`crate::container::tree::tree_op::TreeOp`], but semantic.
#[derive(Debug, Clone)]
pub enum TreeInternalDiff {
    /// First create the node, have not seen it before
    Create {
        parent: TreeParentId,
        position: FracIndex,
    },
    /// For retreating, if the node is only created, not move it to `DELETED_ROOT` but delete it directly
    UnCreate,
    /// Move the node to the parent, the node exists
    Move {
        parent: TreeParentId,
        position: FracIndex,
    },
    /// move under a parent that is deleted
    Delete { parent: TreeParentId },
    /// old parent is deleted, new parent is deleted too
    MoveInDelete {
        parent: TreeParentId,
        position: Option<FracIndex>,
    },
}

impl TreeDeltaItem {
    /// * `is_new_parent_deleted` and `is_old_parent_deleted`: we need to infer whether it's a `creation`.
    ///    It's a creation if the old_parent is deleted but the new parent isn't.
    ///    If it is a creation, we need to emit the `Create` event so that downstream event handler can
    ///    handle the new containers easier.
    pub(crate) fn new(
        target: TreeID,
        parent: TreeParentId,
        old_parent: TreeParentId,
        op_id: IdFull,
        is_new_parent_deleted: bool,
        is_old_parent_deleted: bool,
        position: Option<FracIndex>,
    ) -> Self {
        let action = if matches!(parent, TreeParentId::Unexist) {
            TreeInternalDiff::UnCreate
        } else {
            match (
                is_new_parent_deleted,
                is_old_parent_deleted || old_parent == TreeParentId::Unexist,
            ) {
                (true, true) => TreeInternalDiff::MoveInDelete { parent, position },
                (true, false) => TreeInternalDiff::Delete { parent },
                (false, true) => TreeInternalDiff::Create {
                    parent,
                    position: position.unwrap(),
                },
                (false, false) => TreeInternalDiff::Move {
                    parent,
                    position: position.unwrap(),
                },
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

// TODO: we moved the `TreeValue` to `fuzz` crate,
//       when we clean up the `fuzz` in `internal` crate, we can remove this.
#[derive(Debug)]
pub(crate) struct TreeValue<'a>(pub(crate) &'a mut Vec<LoroValue>);

impl<'a> TreeValue<'a> {
    pub(crate) fn apply_diff(&mut self, diff: &TreeDiff) {
        for d in diff.diff.iter() {
            let target = d.target;
            match &d.action {
                TreeExternalDiff::Create { parent, position } => {
                    self.create_target(target);
                    self.mov(target, *parent, position.clone());
                }
                TreeExternalDiff::Delete => self.delete_target(target),
                TreeExternalDiff::Move { parent, position } => {
                    self.mov(target, *parent, position.clone())
                }
            }
        }
    }

    fn mov(&mut self, target: TreeID, parent: Option<TreeID>, position: FracIndex) {
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
        map_mut.insert("position".to_string(), (position.to_string()).into());
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

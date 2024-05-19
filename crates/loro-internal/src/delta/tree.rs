use fractional_index::FractionalIndex;
use fxhash::FxHashMap;
use itertools::Itertools;
use loro_common::{IdFull, TreeID};
use std::fmt::Debug;
use std::ops::{Deref, DerefMut};

use crate::state::TreeParentId;

#[derive(Debug, Clone, Default)]
pub struct TreeDiff {
    pub diff: Vec<TreeDiffItem>,
}

#[derive(Debug, Clone)]
pub struct TreeDiffItem {
    pub target: TreeID,
    pub action: TreeExternalDiff,
}

#[derive(Debug, Clone)]
pub enum TreeExternalDiff {
    Create {
        parent: Option<TreeID>,
        index: usize,
        position: FractionalIndex,
    },
    Move {
        parent: Option<TreeID>,
        index: usize,
        position: FractionalIndex,
        old_parent: TreeParentId,
        old_index: usize,
    },
    Delete {
        old_parent: TreeParentId,
        old_index: usize,
    },
}

impl TreeDiff {
    pub(crate) fn compose(mut self, other: Self) -> Self {
        // TODO: better compose
        self.diff.extend(other.diff);
        self
    }

    pub(crate) fn extend<I: IntoIterator<Item = TreeDiffItem>>(mut self, other: I) -> Self {
        self.diff.extend(other);
        self
    }

    pub(crate) fn transform(&mut self, b: &TreeDiff, left_priority: bool) {
        // println!("transforming {:?} with {:?}", self, b);
        // We need to cooperate with handler's apply_diff
        // 1. If the node is created/moved in the left, and the parent is removed in the right (maybe an ancestor), we should remove the creation
        // 2. If the movement of left causes the cycle, we should remove the movement

        let b_update: FxHashMap<_, _> = b.diff.iter().map(|d| (d.target, &d.action)).collect();
        let mut self_update: FxHashMap<_, _> = self
            .diff
            .iter()
            .enumerate()
            .map(|(i, d)| (d.target, (&d.action, i)))
            .collect();
        if !left_priority {
            let mut removes = Vec::new();
            for (target, _) in b_update {
                if let Some((_, i)) = self_update.remove(&target) {
                    removes.push(i);
                }
            }
            for i in removes.into_iter().sorted().rev() {
                self.diff.remove(i);
            }
        }
        let mut b_parent = FxHashMap::default();
        for diff in b.diff.iter() {
            match &diff.action {
                TreeExternalDiff::Create {
                    parent,
                    index,
                    position: _,
                } => {
                    b_parent
                        .entry(TreeParentId::from(*parent))
                        .or_insert_with(Vec::new)
                        .push(*index as i32);
                }
                TreeExternalDiff::Move {
                    parent,
                    index,
                    position: _,
                    old_parent,
                    old_index,
                } => {
                    b_parent
                        .entry(*old_parent)
                        .or_insert_with(Vec::new)
                        .push(-(*old_index as i32));
                    b_parent
                        .entry(TreeParentId::from(*parent))
                        .or_insert_with(Vec::new)
                        .push(*index as i32);
                }
                TreeExternalDiff::Delete {
                    old_index,
                    old_parent,
                } => {
                    b_parent
                        .entry(*old_parent)
                        .or_insert_with(Vec::new)
                        .push(-(*old_index as i32));
                }
            }
        }
        b_parent
            .iter_mut()
            .for_each(|(_, v)| v.sort_unstable_by_key(|i| i.abs()));
        for diff in self.iter_mut() {
            match &mut diff.action {
                TreeExternalDiff::Create {
                    parent,
                    index,
                    position: _,
                } => {
                    if let Some(b_indices) = b_parent.get(&TreeParentId::from(*parent)) {
                        for i in b_indices.iter() {
                            if (i.unsigned_abs() as usize) < *index {
                                if i > &0 {
                                    *index += 1;
                                } else {
                                    *index -= 1;
                                }
                            } else if i.unsigned_abs() as usize == *index && !left_priority {
                                todo!()
                            } else {
                                break;
                            }
                        }
                    }
                }
                TreeExternalDiff::Move {
                    parent,
                    index,
                    position: _,
                    old_parent,
                    old_index,
                } => {
                    if let Some(b_indices) = b_parent.get(&TreeParentId::from(*parent)) {
                        for i in b_indices.iter() {
                            if (i.unsigned_abs() as usize) < *index {
                                if i > &0 {
                                    *index += 1;
                                } else {
                                    *index -= 1;
                                }
                            } else if i.unsigned_abs() as usize == *index && !left_priority {
                                todo!()
                            } else {
                                break;
                            }
                        }
                    }
                    if let Some(b_indices) = b_parent.get(old_parent) {
                        for i in b_indices.iter() {
                            if (i.unsigned_abs() as usize) < *old_index {
                                if i > &0 {
                                    *old_index += 1;
                                } else {
                                    *old_index -= 1;
                                }
                            } else if i.unsigned_abs() as usize == *old_index && !left_priority {
                                todo!()
                            } else {
                                break;
                            }
                        }
                    }
                }
                TreeExternalDiff::Delete {
                    old_index,
                    old_parent,
                } => {
                    if let Some(b_indices) = b_parent.get(old_parent) {
                        for i in b_indices.iter() {
                            if (i.unsigned_abs() as usize) < *old_index {
                                if i > &0 {
                                    *old_index += 1;
                                } else {
                                    *old_index -= 1;
                                }
                            } else if i.unsigned_abs() as usize == *old_index && !left_priority {
                                todo!()
                            } else {
                                break;
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Representation of differences in movable tree. It's an ordered list of [`TreeDiff`].
#[derive(Clone, Default)]
pub struct TreeDelta {
    pub(crate) diff: Vec<TreeDeltaItem>,
}

impl Debug for TreeDelta {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("TreeDelta{ diff: [\n")?;
        for item in self.diff.iter() {
            f.write_fmt(format_args!("\t{:?}, \n", item))?;
        }
        f.write_str("]}")
    }
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
        position: FractionalIndex,
    },
    /// For retreating, if the node is only created, not move it to `DELETED_ROOT` but delete it directly
    UnCreate,
    /// Move the node to the parent, the node exists
    Move {
        parent: TreeParentId,
        position: FractionalIndex,
    },
    /// move under a parent that is deleted
    Delete {
        parent: TreeParentId,
        position: Option<FractionalIndex>,
    },
    /// old parent is deleted, new parent is deleted too
    MoveInDelete {
        parent: TreeParentId,
        position: Option<FractionalIndex>,
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
        position: Option<FractionalIndex>,
    ) -> Self {
        // TODO: check op id
        let action = if matches!(parent, TreeParentId::Unexist) {
            TreeInternalDiff::UnCreate
        } else {
            match (
                is_new_parent_deleted,
                is_old_parent_deleted || old_parent == TreeParentId::Unexist,
            ) {
                (true, true) => TreeInternalDiff::MoveInDelete { parent, position },
                (true, false) => TreeInternalDiff::Delete { parent, position },
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
    pub(crate) fn compose(mut self, x: TreeDelta) -> TreeDelta {
        self.diff.extend(x.diff);
        self
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

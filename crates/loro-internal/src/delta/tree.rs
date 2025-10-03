use fractional_index::FractionalIndex;
use itertools::Itertools;
use loro_common::{IdFull, TreeID};
use rustc_hash::{FxHashMap, FxHashSet};
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

#[derive(Debug, Clone, PartialEq)]
pub enum TreeExternalDiff {
    Create {
        parent: TreeParentId,
        index: usize,
        position: FractionalIndex,
    },
    Move {
        parent: TreeParentId,
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
        self.diff.extend(other.diff);
        // self = compose_tree_diff(&self);
        self
    }

    pub(crate) fn extend<I: IntoIterator<Item = TreeDiffItem>>(mut self, other: I) -> Self {
        self.diff.extend(other);
        self
    }

    fn to_hash_map_mut(&mut self) -> FxHashMap<TreeID, usize> {
        let mut ans = FxHashSet::default();
        for index in (0..self.diff.len()).rev() {
            let target = self.diff[index].target;
            if ans.contains(&target) {
                self.diff.remove(index);
                continue;
            }
            ans.insert(target);
        }
        self.iter()
            .map(|x| x.target)
            .enumerate()
            .map(|(i, x)| (x, i))
            .collect()
    }

    pub(crate) fn transform(&mut self, b: &TreeDiff, left_prior: bool) {
        // println!("\ntransform prior {:?} {:?} \nb {:?}", left_prior, self, b);
        if b.is_empty() || self.is_empty() {
            return;
        }
        if !left_prior {
            let mut self_update = self.to_hash_map_mut();
            for i in b
                .iter()
                .map(|x| x.target)
                .filter_map(|x| self_update.remove(&x))
                .sorted()
                .rev()
            {
                self.remove(i);
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
    ///   It's a creation if the old_parent is deleted but the new parent isn't.
    ///   If it is a creation, we need to emit the `Create` event so that downstream event handler can
    ///   handle the new containers easier.
    pub(crate) fn new(
        target: TreeID,
        parent: TreeParentId,
        old_parent: TreeParentId,
        op_id: IdFull,
        is_new_parent_deleted: bool,
        is_old_parent_deleted: bool,
        position: Option<FractionalIndex>,
    ) -> Self {
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

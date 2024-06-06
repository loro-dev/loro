use fractional_index::FractionalIndex;
use fxhash::FxHashMap;
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

#[derive(Debug, Clone, PartialEq)]
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
        self.diff.extend(other.diff);
        // self = compose_tree_diff(&self);
        self
    }

    pub(crate) fn extend<I: IntoIterator<Item = TreeDiffItem>>(mut self, other: I) -> Self {
        self.diff.extend(other);
        self
    }

    pub(crate) fn transform(&mut self, b: &TreeDiff, left_prior: bool) {
        // println!("\ntransform {:?} \nb {:?}\n", self, b);
        if b.is_empty() || self.is_empty() {
            return;
        }
        if left_prior {
            *self = Forest::default().transform_diff(self.clone(), b.clone());
        } else {
            *self = Forest::default().transform_diff(b.clone(), self.clone());
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

#[derive(Debug, Clone, PartialEq)]
struct Node {
    target: TreeID,
    index: Option<usize>,
    position: Option<FractionalIndex>,
}

#[derive(Debug, Default)]
struct Forest {
    parent: FxHashMap<TreeID, TreeParentId>,
    children: FxHashMap<TreeParentId, Vec<Node>>,
    move_in_out: FxHashMap<TreeParentId, Vec<i32>>,
}

impl Forest {
    fn transform_diff(&mut self, left: TreeDiff, right: TreeDiff) -> TreeDiff {
        // apply right first
        self.apply_diff(right);

        // transform left
        let mut ans = TreeDiff::default();
        for mut diff in left.diff.into_iter() {
            let target = diff.target;
            match &mut diff.action {
                TreeExternalDiff::Create {
                    parent,
                    index,
                    position: _,
                } => {
                    let effect = self
                        .parent
                        .get(&target)
                        .is_some_and(|p| p == &TreeParentId::from(*parent));
                    if !effect {
                        self.reset_index(index, &TreeParentId::from(*parent));
                        ans.push(diff.clone());
                        if self
                            .parent
                            .get(&target)
                            .is_some_and(|p| p == &TreeParentId::Deleted)
                        {
                            if let Some(children) = self.children.get(&TreeParentId::Node(target)) {
                                for child in children.iter().cloned() {
                                    // TODO: index
                                    ans.push(TreeDiffItem {
                                        target: child.target,
                                        action: TreeExternalDiff::Create {
                                            parent: Some(target),
                                            index: child.index.unwrap(),
                                            position: child.position.unwrap(),
                                        },
                                    })
                                }
                            }
                        }
                        self.apply_diff_item(diff, false);
                    }
                }
                TreeExternalDiff::Move {
                    parent,
                    index,
                    position: _,
                    old_parent,
                    old_index,
                } => {
                    let effect = self
                        .parent
                        .get(&target)
                        .is_some_and(|p| p == &TreeParentId::from(*parent));
                    if !effect {
                        self.reset_index(index, &TreeParentId::from(*parent));
                        self.reset_index(old_index, old_parent);
                        ans.push(diff.clone());
                        if self
                            .parent
                            .get(&target)
                            .is_some_and(|p| p == &TreeParentId::Deleted)
                        {
                            if let Some(children) = self.children.get(&TreeParentId::Node(target)) {
                                for child in children.iter().cloned() {
                                    ans.push(TreeDiffItem {
                                        target: child.target,
                                        action: TreeExternalDiff::Create {
                                            parent: Some(target),
                                            index: child.index.unwrap(),
                                            position: child.position.unwrap(),
                                        },
                                    })
                                }
                            }
                        }
                        self.apply_diff_item(diff, false);
                    }
                }
                TreeExternalDiff::Delete {
                    old_index,
                    old_parent,
                } => {
                    if let Some(v) = self.children.get(&TreeParentId::Deleted) {
                        if v.iter().any(|n| n.target == target) {
                            self.reset_index(old_index, old_parent);
                            ans.push(diff.clone());
                            self.apply_diff_item(diff, false);
                        }
                    }
                }
            }
        }
        ans
    }

    fn apply_diff(&mut self, diff: TreeDiff) {
        for item in diff.diff.into_iter() {
            self.apply_diff_item(item, true);
        }
    }

    fn apply_diff_item(&mut self, item: TreeDiffItem, record_index: bool) {
        let target = item.target;
        match item.action {
            TreeExternalDiff::Create {
                parent,
                index,
                position,
            } => {
                let parent = TreeParentId::from(parent);
                self.parent.insert(target, parent);
                // we need not to modify the index of node
                self.children.entry(parent).or_default().push(Node {
                    target,
                    index: Some(index),
                    position: Some(position),
                });
                // index
                if record_index {
                    self.move_in_out
                        .entry(parent)
                        .or_default()
                        .push(index as i32);
                }
            }
            TreeExternalDiff::Move {
                parent,
                index,
                position,
                old_index,
                old_parent,
            } => {
                let parent = TreeParentId::from(parent);
                self.children.entry(parent).or_default().push(Node {
                    target,
                    index: Some(index),
                    position: Some(position),
                });
                if let Some(parent) = self.parent.insert(target, parent) {
                    if let Some(v) = self.children.get_mut(&parent) {
                        let index = v.binary_search_by(|x| x.target.cmp(&target));
                        if let Ok(index) = index {
                            v.remove(index);
                        }
                    }
                }
                if record_index {
                    // position
                    self.move_in_out
                        .entry(old_parent)
                        .or_default()
                        .push(-(old_index as i32));
                    self.move_in_out
                        .entry(parent)
                        .or_default()
                        .push(index as i32);
                }
            }
            TreeExternalDiff::Delete {
                old_index,
                old_parent,
            } => {
                self.children
                    .entry(TreeParentId::Deleted)
                    .or_default()
                    .push(Node {
                        target,
                        index: None,
                        position: None,
                    });
                if let Some(parent) = self.parent.insert(target, TreeParentId::Deleted) {
                    if let Some(v) = self.children.get_mut(&parent) {
                        let index = v.binary_search_by(|x| x.target.cmp(&target));
                        if let Ok(index) = index {
                            v.remove(index);
                        }
                    }
                }
                if record_index {
                    // position
                    self.move_in_out
                        .entry(old_parent)
                        .or_default()
                        .push(-(old_index as i32));
                }
            }
        }
    }

    fn reset_index(&self, index: &mut usize, parent: &TreeParentId) {
        if let Some(b_indices) = self.move_in_out.get(parent) {
            for i in b_indices.iter() {
                if (i.unsigned_abs() as usize) < *index
                // || (i.unsigned_abs() as usize == *index && !left_priority)
                {
                    if i > &0 {
                        *index += 1;
                    } else if *index > (i.unsigned_abs() as usize) {
                        *index = index.saturating_sub(1);
                    }
                } else {
                    break;
                }
            }
        }
    }

    // fn compose_diff(&mut self, diff: &TreeDiff) -> TreeDiff {
    //     for TreeDiffItem { target, action } in diff.diff.iter().cloned() {
    //         match action {
    //             TreeExternalDiff::Create { parent, .. } => {
    //                 let parent = TreeParentId::from(parent);
    //                 self.parent.insert(target, parent);
    //                 // we need not to modify the index of node
    //                 self.children.entry(parent).or_default().push(target);
    //             }
    //             TreeExternalDiff::Move { parent, .. } => {
    //                 let parent = TreeParentId::from(parent);
    //                 self.children.entry(parent).or_default().push(target);
    //                 let old_parent = self.parent.insert(target, parent);
    //                 if let Some(parent) = old_parent {
    //                     if let Some(v) = self.children.get_mut(&parent) {
    //                         v.retain(|n| n != &target)
    //                     }
    //                 }
    //             }
    //             TreeExternalDiff::Delete { .. } => {
    //                 let mut q = VecDeque::from_iter([target]);
    //                 while let Some(node) = q.pop_front() {
    //                     if let Some(children) = self.children.get(&TreeParentId::Node(node)) {
    //                         for child in children.iter() {
    //                             q.push_back(*child);
    //                         }
    //                     }
    //                     let parent = self.parent.remove(&node);
    //                     if let Some(parent) = parent {
    //                         if let Some(v) = self.children.get_mut(&parent) {
    //                             v.retain(|n| n != &node)
    //                         }
    //                     } else {
    //                         self.delete_others.insert(target);
    //                     }
    //                 }
    //             }
    //         }
    //     }

    //     let mut ans = TreeDiff::default();
    //     for diff in diff.diff.iter() {
    //         let target = diff.target;
    //         if matches!(diff.action, TreeExternalDiff::Delete { .. }) {
    //             if self.delete_others.contains(&target) {
    //                 ans.push(diff.clone());
    //             }
    //         } else {
    //             let parent = match diff.action {
    //                 TreeExternalDiff::Create { parent, .. } => parent,
    //                 TreeExternalDiff::Move { parent, .. } => parent,
    //                 _ => unreachable!(),
    //             };
    //             let parent = TreeParentId::from(parent);
    //             if self.parent.get(&target) == Some(&parent) {
    //                 ans.push(diff.clone());
    //             }
    //         }
    //     }
    //     ans
    // }
}

#[cfg(test)]
mod tests {
    use fractional_index::FractionalIndex;
    use loro_common::TreeID;

    use crate::state::TreeParentId;

    use super::{TreeDiff, TreeDiffItem};

    // #[test]
    // fn create_delete() {
    //     let diff = TreeDiff {
    //         diff: vec![
    //             TreeDiffItem {
    //                 target: TreeID {
    //                     peer: 0,
    //                     counter: 0,
    //                 },
    //                 action: super::TreeExternalDiff::Create {
    //                     parent: None,
    //                     index: 0,
    //                     position: FractionalIndex::default(),
    //                 },
    //             },
    //             TreeDiffItem {
    //                 target: TreeID {
    //                     peer: 0,
    //                     counter: 0,
    //                 },
    //                 action: super::TreeExternalDiff::Delete {
    //                     old_parent: TreeParentId::Root,
    //                     old_index: 0,
    //                 },
    //             },
    //         ],
    //     };
    //     let ans = compose_tree_diff(&diff);
    //     assert!(ans.is_empty());
    // }

    // #[test]
    // fn delete_other() {
    //     let diff = TreeDiff {
    //         diff: vec![TreeDiffItem {
    //             target: TreeID {
    //                 peer: 0,
    //                 counter: 2,
    //             },
    //             action: super::TreeExternalDiff::Delete {
    //                 old_parent: TreeParentId::Root,
    //                 old_index: 0,
    //             },
    //         }],
    //     };
    //     let ans = compose_tree_diff(&diff);
    //     assert_eq!(ans.len(), 1);
    // }

    // #[test]
    // fn delete_parent() {
    //     let target = TreeID {
    //         peer: 0,
    //         counter: 0,
    //     };
    //     let child = TreeID {
    //         peer: 0,
    //         counter: 1,
    //     };
    //     let diff = TreeDiff {
    //         diff: vec![
    //             TreeDiffItem {
    //                 target,
    //                 action: super::TreeExternalDiff::Create {
    //                     parent: None,
    //                     index: 0,
    //                     position: FractionalIndex::default(),
    //                 },
    //             },
    //             TreeDiffItem {
    //                 target: child,
    //                 action: super::TreeExternalDiff::Create {
    //                     parent: Some(target),
    //                     index: 0,
    //                     position: FractionalIndex::default(),
    //                 },
    //             },
    //             TreeDiffItem {
    //                 target,
    //                 action: super::TreeExternalDiff::Delete {
    //                     old_parent: TreeParentId::Root,
    //                     old_index: 0,
    //                 },
    //             },
    //         ],
    //     };
    //     let ans = compose_tree_diff(&diff);
    //     assert!(ans.is_empty());
    // }
}

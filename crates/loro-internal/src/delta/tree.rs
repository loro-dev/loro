use fractional_index::FractionalIndex;
use fxhash::{FxHashMap, FxHashSet};
use itertools::Itertools;
use loro_common::{IdFull, TreeID};
use std::fmt::Debug;
use std::ops::{Deref, DerefMut};

use crate::state::TreeParentId;

#[derive(Clone, Default)]
pub struct TreeDiff {
    pub diff: Vec<TreeDiffItem>,
}

impl std::fmt::Debug for TreeDiff {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "TreeDiff{{")?;
        for item in &self.diff {
            writeln!(f, "  {:?},", item)?;
        }
        write!(f, "}}")
    }
}

#[derive(Debug, Clone, PartialEq)]
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
    #[allow(clippy::let_and_return)]
    pub(crate) fn compose(self, other: Self) -> Self {
        println!("\ncompose \n{:?} \n{:?}", self, other);
        let mut temp_tree = compose::TempTree::default();
        for (sort_index, item) in self
            .diff
            .into_iter()
            .chain(other.diff.into_iter())
            .enumerate()
        {
            println!("\napply self {:?}", item);
            temp_tree.apply(item, sort_index);
            println!("\ntemp_tree {:?}\n", temp_tree);
        }
        let ans = temp_tree.into_diff();
        println!("ans {:?}", ans);
        ans
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

mod compose {
    use super::*;
    #[derive(Debug, Default)]
    pub struct TempTree {
        tree: FxHashMap<TreeID, Node>,
        roots: Vec<TreeID>,
        deleted: FxHashMap<TreeID, Node>,
    }

    #[derive(Debug, Clone)]
    struct Node {
        id: TreeID,
        children: Vec<TreeID>,
        sort_index: usize,
        filter: bool,
        diff: Option<TreeExternalDiff>,
    }

    impl Node {
        fn temp(&self) -> bool {
            !matches!(
                &self.diff,
                Some(TreeExternalDiff::Create { .. }) | Some(TreeExternalDiff::Move { .. })
            )
        }

        fn index(&self) -> Option<usize> {
            match &self.diff {
                Some(TreeExternalDiff::Create { index, .. }) => Some(*index),
                Some(TreeExternalDiff::Move { index, .. }) => Some(*index),
                _ => None,
            }
        }

        fn index_mut(&mut self) -> Option<&mut usize> {
            match &mut self.diff {
                Some(TreeExternalDiff::Create { index, .. }) => Some(index),
                Some(TreeExternalDiff::Move { index, .. }) => Some(index),
                _ => None,
            }
        }
    }

    impl TempTree {
        pub fn apply(
            &mut self,
            TreeDiffItem { target, mut action }: TreeDiffItem,
            sort_index: usize,
        ) {
            match action {
                TreeExternalDiff::Create { parent, index, .. } => {
                    self.create(target, parent, index, action, sort_index);
                }
                TreeExternalDiff::Move {
                    parent,
                    index,
                    old_parent,
                    old_index,
                    ..
                } => {
                    if self.tree.contains_key(&target)
                        && self
                            .tree
                            .get(&target)
                            .unwrap()
                            .diff
                            .as_ref()
                            .is_some_and(|d| matches!(d, TreeExternalDiff::Create { .. }))
                    {
                        let position = match &action {
                            TreeExternalDiff::Move { position, .. } => position.clone(),
                            _ => unreachable!(),
                        };
                        action = TreeExternalDiff::Create {
                            parent,
                            index,
                            position,
                        };
                    }
                    self.delete(target, old_parent, old_index, sort_index);

                    self.create(target, parent, index, action, sort_index);
                    // If created in batch, it should be create instead of move
                }
                TreeExternalDiff::Delete {
                    old_parent,
                    old_index,
                } => {
                    self.delete(target, old_parent, old_index, sort_index);
                }
            }
        }

        fn create(
            &mut self,
            target: TreeID,
            parent: TreeParentId,
            index: usize,
            action: TreeExternalDiff,
            sort_index: usize,
        ) {
            let (mut node, need_update) = if self.deleted.contains_key(&target) {
                (self.deleted.remove(&target).unwrap(), true)
            } else {
                (
                    Node {
                        id: target,
                        children: vec![],
                        diff: Some(action.clone()),
                        filter: false,
                        sort_index,
                    },
                    false,
                )
            };

            // insert into parent
            match parent {
                TreeParentId::Root => {
                    let children = &mut self.roots;
                    if children.is_empty() {
                        children.push(target);
                    } else {
                        for (i, id) in children.iter().enumerate().rev() {
                            if self.tree.get(id).unwrap().temp() {
                                continue;
                            }
                            // Traverse backwards to find the first one with index less than create index
                            if self.tree.get(id).unwrap().index().unwrap() < index {
                                children.insert(i + 1, target);
                                break;
                            }
                            // The traversed items need to increment index by 1
                            let other_node = self.tree.get_mut(id).unwrap();
                            if need_update && other_node.sort_index > node.sort_index {
                                *other_node.index_mut().unwrap() += 1;
                            }
                        }
                    }
                }
                TreeParentId::Node(id) => {
                    let children = self
                        .tree
                        .entry(id)
                        .or_insert_with(|| {
                            self.roots.push(id);
                            Node {
                                id,
                                children: vec![],
                                diff: None,
                                filter: true,
                                sort_index,
                            }
                        })
                        .children
                        .clone();
                    if children.is_empty() {
                        self.tree.get_mut(&id).unwrap().children.push(target);
                    } else {
                        for (i, child_id) in children.iter().enumerate().rev() {
                            // Traverse backwards to find the first one with index less than create index
                            if self.tree.get(child_id).unwrap().index().unwrap() < index {
                                self.tree
                                    .get_mut(&id)
                                    .unwrap()
                                    .children
                                    .insert(i + 1, target);
                                break;
                            }
                            // The traversed items need to increment index by 1
                            let other_node = self.tree.get_mut(child_id).unwrap();
                            if need_update && other_node.sort_index > node.sort_index {
                                *other_node.index_mut().unwrap() += 1;
                            }
                        }
                    }
                }
                TreeParentId::Unexist | TreeParentId::Deleted => unreachable!(),
            };

            if need_update {
                node.filter = false;
                node.sort_index = sort_index;
                node.diff = Some(action);
            }

            self.tree.insert(target, node);
        }

        fn delete(
            &mut self,
            target: TreeID,
            old_parent: TreeParentId,
            old_index: usize,
            sort_index: usize,
        ) {
            // Check if previous events are affected. For example, if A was created first, then B was created after target,
            // when A is deleted, B's index needs to be decremented by 1

            match old_parent {
                TreeParentId::Root => {
                    if let Some(index) = self.roots.iter().position(|id| *id == target) {
                        // The node created or moved this time is deleted
                        let node_id = self.roots.remove(index);

                        let mut node = self.tree.remove(&node_id).unwrap();

                        for n in self.roots.iter() {
                            if self.tree.get(n).unwrap().temp() {
                                // temp tree root
                                continue;
                            }
                            let other_node = self.tree.get_mut(n).unwrap();
                            if other_node.sort_index > node.sort_index
                                && other_node.index().unwrap() > old_index
                            {
                                if let Some(index) = other_node.index_mut() {
                                    *index -= 1;
                                }
                            }
                        }

                        node.diff = Some(TreeExternalDiff::Delete {
                            old_parent,
                            old_index,
                        });
                        node.filter = true;
                        node.sort_index = sort_index;
                        node.children = vec![];
                        self.deleted.insert(target, node);
                    } else {
                        self.deleted.insert(
                            target,
                            Node {
                                id: target,
                                children: vec![],
                                filter: false,
                                diff: Some(TreeExternalDiff::Delete {
                                    old_parent,
                                    old_index,
                                }),
                                sort_index,
                            },
                        );
                    }
                }
                TreeParentId::Node(id) => {
                    // just for ownership of parent
                    if let Some(mut parent) = self.tree.remove(&id) {
                        if let Some(index) = parent.children.iter().position(|id| *id == target) {
                            // The node created or moved this time is deleted
                            let node_id = parent.children.remove(index);

                            self.roots.retain(|id| *id != node_id);
                            let mut node = self.tree.remove(&node_id).unwrap();
                            for n in parent.children.iter() {
                                let other_node = self.tree.get_mut(n).unwrap();
                                if other_node.sort_index > node.sort_index
                                    && other_node.index().unwrap() > old_index
                                {
                                    if let Some(index) = other_node.index_mut() {
                                        *index -= 1;
                                    }
                                }
                            }
                            node.diff = Some(TreeExternalDiff::Delete {
                                old_parent,
                                old_index,
                            });
                            node.filter = true;
                            node.sort_index = sort_index;
                            node.children = vec![];
                            self.deleted.insert(target, node);
                        } else {
                            // Parent exists but target is not in batch
                            self.deleted.insert(
                                target,
                                Node {
                                    id: target,
                                    children: vec![],
                                    filter: false,
                                    diff: Some(TreeExternalDiff::Delete {
                                        old_parent,
                                        old_index,
                                    }),
                                    sort_index,
                                },
                            );
                        }
                        self.tree.insert(id, parent);
                    } else {
                        self.deleted.insert(
                            target,
                            Node {
                                id: target,
                                children: vec![],
                                filter: false,
                                diff: Some(TreeExternalDiff::Delete {
                                    old_parent,
                                    old_index,
                                }),
                                sort_index,
                            },
                        );
                    }
                }
                TreeParentId::Unexist | TreeParentId::Deleted => unreachable!(),
            };
        }

        pub fn into_diff(mut self) -> TreeDiff {
            let mut diff = TreeDiff::default();
            self.roots
                .sort_by_key(|id| self.tree.get(id).map(|node| node.index()).unwrap());
            self.pre_order_traverse(|mut node| {
                if node.filter {
                    return;
                }
                if let Some(action) = node.diff.take() {
                    diff.push(TreeDiffItem {
                        target: node.id,
                        action,
                    });
                }
            });
            diff
        }

        fn pre_order_traverse<F>(&mut self, mut f: F)
        where
            F: FnMut(Node),
        {
            let mut ans = std::mem::take(&mut self.deleted)
                .into_values()
                .collect::<Vec<_>>();
            let mut stack = Vec::new();

            // Push all root nodes to stack initially
            for root_id in self.roots.iter().rev() {
                stack.push(*root_id);
            }

            // Process nodes in stack
            while let Some(node_id) = stack.pop() {
                if let Some(node) = self.tree.remove(&node_id) {
                    // Push children to stack in reverse order
                    // so they are processed in correct order
                    for child_id in node.children.iter().rev() {
                        stack.push(*child_id);
                    }
                    ans.push(node);
                }
            }
            for node in ans.into_iter().sorted_by_key(|node| node.sort_index) {
                f(node);
            }
        }
    }
}

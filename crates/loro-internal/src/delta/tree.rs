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
        // println!("\ncompose \n{:?} \n{:?}", self, other);
        let mut temp_tree = compose::TempTree::default();
        for (sort_index, item) in self
            .diff
            .into_iter()
            .chain(other.diff.into_iter())
            .enumerate()
        {
            // println!("\napply self {:?}", item);
            temp_tree.apply(item, sort_index);
            // println!("\ntemp_tree {:?}\n", temp_tree);
        }
        let ans = temp_tree.into_diff();
        // println!("ans {:?}", ans);
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
    use std::{cell::RefCell, cmp::Ordering, collections::BTreeSet};

    use super::*;
    #[derive(Debug, Default)]
    pub struct TempTree {
        tree: FxHashMap<TreeID, RefCell<Node>>,
        roots: Children,
        deleted: FxHashMap<TreeID, Node>,
    }

    #[derive(Debug, Clone)]
    struct Node {
        id: TreeID,
        children: Children,
        sort_index: usize,
        filter: bool,
        exist: bool,
        old_parent_index: Option<(TreeParentId, usize)>,
        diff: Option<TreeExternalDiff>,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct IdAndCausal {
        id: TreeID,
        causal_index: usize,
    }

    impl Ord for IdAndCausal {
        fn cmp(&self, other: &Self) -> Ordering {
            self.causal_index.cmp(&other.causal_index)
        }
    }

    impl PartialOrd for IdAndCausal {
        fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
            Some(self.cmp(other))
        }
    }

    enum Effect {
        Create,
        Delete,
    }

    #[derive(Debug, Clone, Default)]
    struct Children(BTreeSet<IdAndCausal>);

    impl Children {
        fn insert(&mut self, id: TreeID, sort_index: usize) {
            self.0.insert(IdAndCausal {
                id,
                causal_index: sort_index,
            });
        }

        fn remove(&mut self, id: TreeID) -> bool {
            if let Some(&index) = self.0.iter().find(|x| x.id == id) {
                self.0.remove(&index);
                true
            } else {
                false
            }
        }

        fn side_effect(
            &self,
            id: TreeID,
            last_index: usize,
            tree: &FxHashMap<TreeID, RefCell<Node>>,
            last_causal_index: usize,
            effect: Effect,
        ) {
            for child in self.0.range(
                IdAndCausal {
                    id,
                    causal_index: last_causal_index,
                }..,
            ) {
                let other_node = tree.get(&child.id).unwrap();
                if !other_node.borrow().temp() && other_node.borrow().index().unwrap() > last_index
                {
                    match effect {
                        Effect::Create => {
                            *other_node.borrow_mut().index_mut().unwrap() += 1;
                        }
                        Effect::Delete => {
                            *other_node.borrow_mut().index_mut().unwrap() -= 1;
                        }
                    }
                }
            }
        }
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

        fn old_index(&self) -> Option<usize> {
            match &self.diff {
                Some(TreeExternalDiff::Move { old_index, .. }) => Some(*old_index),
                Some(TreeExternalDiff::Delete { old_index, .. }) => Some(*old_index),
                _ => None,
            }
        }

        fn into_diff(self) -> Option<TreeDiffItem> {
            let diff = self.diff?;
            let action = match diff {
                TreeExternalDiff::Create {
                    parent,
                    index,
                    position,
                } => {
                    if self.exist {
                        TreeExternalDiff::Move {
                            parent,
                            index,
                            position,
                            old_parent: self.old_parent_index.unwrap().0,
                            old_index: self.old_parent_index.unwrap().1,
                        }
                    } else {
                        TreeExternalDiff::Create {
                            parent,
                            index,
                            position,
                        }
                    }
                }
                TreeExternalDiff::Move {
                    parent,
                    index,
                    position,
                    old_parent,
                    old_index,
                } => {
                    if self.exist {
                        if parent == old_parent && index == old_index {
                            return None;
                        }
                        TreeExternalDiff::Move {
                            parent,
                            index,
                            position,
                            old_parent: self.old_parent_index.unwrap().0,
                            old_index: self.old_parent_index.unwrap().1,
                        }
                    } else {
                        TreeExternalDiff::Create {
                            parent,
                            index,
                            position,
                        }
                    }
                }
                TreeExternalDiff::Delete {
                    old_parent,
                    old_index,
                } => {
                    if self.exist {
                        TreeExternalDiff::Delete {
                            old_parent: self
                                .old_parent_index
                                .as_ref()
                                .map(|(x, _)| *x)
                                .unwrap_or(old_parent),
                            old_index: self
                                .old_parent_index
                                .as_ref()
                                .map(|(_, y)| *y)
                                .unwrap_or(old_index),
                        }
                    } else {
                        return None;
                    }
                }
            };
            Some(TreeDiffItem {
                target: self.id,
                action,
            })
        }
    }

    impl TempTree {
        pub fn apply(&mut self, TreeDiffItem { target, action }: TreeDiffItem, sort_index: usize) {
            match action {
                TreeExternalDiff::Create { parent, index, .. } => {
                    self.create(target, parent, index, action, sort_index);
                }
                TreeExternalDiff::Move {
                    parent,
                    old_parent,
                    old_index,
                    index,
                    ..
                } => {
                    self.delete(target, old_parent, old_index, sort_index);
                    self.create(target, parent, index, action, sort_index);
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
            let maybe_deleted = self.deleted.remove(&target);
            // insert into parent
            {
                let children = match parent {
                    TreeParentId::Root => &mut self.roots,
                    TreeParentId::Node(id) => {
                        self.tree.entry(id).or_insert_with(|| {
                            self.roots.insert(id, sort_index);
                            RefCell::new(Node {
                                id,
                                children: Children::default(),
                                diff: None,
                                filter: true,
                                old_parent_index: None,
                                sort_index,
                                exist: true,
                            })
                        });
                        &mut self.tree.get(&id).unwrap().borrow_mut().children
                    }
                    TreeParentId::Unexist | TreeParentId::Deleted => unreachable!(),
                };

                if let Some(ref node) = maybe_deleted {
                    children.side_effect(
                        target,
                        node.old_index().unwrap(),
                        &self.tree,
                        node.sort_index,
                        Effect::Create,
                    );
                }
                children.insert(target, sort_index);
            }

            let node = Node {
                id: target,
                children: Children::default(),
                diff: Some(action),
                sort_index,
                filter: maybe_deleted.as_ref().is_some_and(|x| !x.filter)
                    && (maybe_deleted.as_ref().unwrap().old_parent_index == Some((parent, index))),
                exist: maybe_deleted.is_some() && maybe_deleted.as_ref().unwrap().exist,
                old_parent_index: maybe_deleted.and_then(|x| x.old_parent_index),
            };

            self.tree.insert(target, RefCell::new(node));
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
                    if !self.roots.remove(target) {
                        self.deleted.insert(
                            target,
                            Node {
                                id: target,
                                children: Children::default(),
                                filter: false,
                                old_parent_index: Some((old_parent, old_index)),
                                diff: Some(TreeExternalDiff::Delete {
                                    old_parent,
                                    old_index,
                                }),
                                sort_index,
                                exist: true,
                            },
                        );
                        return;
                    }
                    let node = self.tree.remove(&target).unwrap();
                    self.roots.side_effect(
                        target,
                        old_index,
                        &self.tree,
                        node.borrow().sort_index,
                        Effect::Delete,
                    );

                    self.deleted.insert(
                        target,
                        Node {
                            id: target,
                            children: Children::default(),
                            sort_index,
                            filter: true,
                            diff: Some(TreeExternalDiff::Delete {
                                old_parent,
                                old_index,
                            }),
                            exist: node.borrow().exist,
                            old_parent_index: node.borrow().old_parent_index,
                        },
                    );
                }
                TreeParentId::Node(id) => {
                    // just for ownership of parent
                    if let Some(parent) = self.tree.remove(&id) {
                        if !parent.borrow_mut().children.remove(target) {
                            // Parent exists but target is not in batch
                            self.deleted.insert(
                                target,
                                Node {
                                    id: target,
                                    children: Children::default(),
                                    filter: false,
                                    exist: true,
                                    diff: Some(TreeExternalDiff::Delete {
                                        old_parent,
                                        old_index,
                                    }),
                                    sort_index,
                                    old_parent_index: Some((old_parent, old_index)),
                                },
                            );
                            self.tree.insert(id, parent);
                            return;
                        }
                        self.roots.remove(target);
                        let node = self.tree.remove(&target).unwrap();
                        assert!(node.borrow().index().unwrap() == old_index);
                        parent.borrow().children.side_effect(
                            target,
                            old_index,
                            &self.tree,
                            node.borrow().sort_index,
                            Effect::Delete,
                        );

                        self.deleted.insert(
                            target,
                            Node {
                                id: target,
                                children: Children::default(),
                                sort_index,
                                filter: true,
                                diff: Some(TreeExternalDiff::Delete {
                                    old_parent,
                                    old_index,
                                }),
                                old_parent_index: node.borrow().old_parent_index,
                                exist: node.borrow().exist,
                            },
                        );
                        self.tree.insert(id, parent);
                    } else {
                        self.deleted.insert(
                            target,
                            Node {
                                id: target,
                                children: Children::default(),
                                filter: false,
                                exist: true,
                                diff: Some(TreeExternalDiff::Delete {
                                    old_parent,
                                    old_index,
                                }),
                                sort_index,
                                old_parent_index: Some((old_parent, old_index)),
                            },
                        );
                    }
                }
                TreeParentId::Unexist | TreeParentId::Deleted => unreachable!(),
            };
        }

        pub fn into_diff(mut self) -> TreeDiff {
            let mut diff = TreeDiff::default();
            self.traverse(|d| {
                diff.push(d);
            });
            diff
        }

        fn traverse<F>(&mut self, mut f: F)
        where
            F: FnMut(TreeDiffItem),
        {
            let mut ans = std::mem::take(&mut self.deleted)
                .into_values()
                .collect::<Vec<_>>();
            let mut stack = Vec::new();

            // Push all root nodes to stack initially
            for root_id in self.roots.0.iter() {
                stack.push(*root_id);
            }

            // Process nodes in stack
            while let Some(node_id) = stack.pop() {
                let Some(node) = self.tree.remove(&node_id.id) else {
                    continue;
                };

                // Push children to stack in reverse order
                // so they are processed in correct order
                for child_id in node.borrow().children.0.iter() {
                    stack.push(*child_id);
                }
                if node.borrow().filter {
                    continue;
                }
                ans.push(node.into_inner());
            }
            for node in ans
                .into_iter()
                .filter(|x| !x.filter)
                .sorted_by_key(|node| node.sort_index)
            {
                if let Some(diff) = node.into_diff() {
                    f(diff);
                }
            }
        }

        #[allow(dead_code)]
        fn apply_diffs(&mut self, diffs: Vec<TreeDiffItem>) {
            for (sort_index, diff) in diffs.into_iter().enumerate() {
                self.apply(diff, sort_index);
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        trait FromLetter {
            fn from_letter(letter: &str) -> Self;
        }

        impl FromLetter for TreeID {
            fn from_letter(letter: &str) -> Self {
                TreeID {
                    peer: 0,
                    counter: letter.chars().next().unwrap() as i32,
                }
            }
        }

        #[macro_export]
        macro_rules! tree_diff {
            ($target:expr, create($parent:expr, $index:expr)) => {
                TreeDiffItem {
                    target: TreeID::from_letter($target),
                    action: TreeExternalDiff::Create {
                        parent: if $parent == "root" {
                            TreeParentId::Root
                        } else {
                            TreeParentId::Node(TreeID::from_letter($parent))
                        },
                        index: $index,
                        position: FractionalIndex::default(),
                    },
                }
            };
            ($target:expr, mov($parent:expr, $index:expr, $old_parent:expr, $old_index:expr)) => {
                TreeDiffItem {
                    target: TreeID::from_letter($target),
                    action: TreeExternalDiff::Move {
                        parent: if $parent == "root" {
                            TreeParentId::Root
                        } else {
                            TreeParentId::Node(TreeID::from_letter($parent))
                        },
                        index: $index,
                        position: FractionalIndex::default(),
                        old_parent: if $old_parent == "root" {
                            TreeParentId::Root
                        } else {
                            TreeParentId::Node(TreeID::from_letter($old_parent))
                        },
                        old_index: $old_index,
                    },
                }
            };
            ($target:expr, delete($old_parent:expr, $old_index:expr)) => {
                TreeDiffItem {
                    target: TreeID::from_letter($target),
                    action: TreeExternalDiff::Delete {
                        old_parent: if $old_parent == "root" {
                            TreeParentId::Root
                        } else {
                            TreeParentId::Node(TreeID::from_letter($old_parent))
                        },
                        old_index: $old_index,
                    },
                }
            };
        }

        #[macro_export]
        macro_rules! compose_test {
            ([$($input:expr),* $(,)?], [$($expected:expr),* $(,)?]) => {
                {
                    let mut tree = TempTree::default();
                    let diffs = vec![$($input),*];

                    tree.apply_diffs(diffs);
                    let diff = tree.into_diff().diff;
                    let expected = vec![$($expected),*];
                    assert_eq!(diff, expected, "\nExpected: {:#?}\nGot: {:#?}", expected, diff);
                }
            };
        }

        #[test]
        fn test_create_delete() {
            compose_test!(
                [
                    tree_diff!("A", create("B", 0)),
                    tree_diff!("A", delete("B", 0))
                ],
                []
            );
        }

        #[test]
        fn test_move_delete() {
            compose_test!(
                [
                    tree_diff!("A", mov("B", 0, "root", 0)),
                    tree_diff!("A", delete("B", 0))
                ],
                []
            );
        }

        #[test]
        fn test_create_move() {
            compose_test!(
                [
                    tree_diff!("A", create("B", 0)),
                    tree_diff!("A", mov("C", 0, "B", 0))
                ],
                [tree_diff!("A", create("C", 0))]
            );
        }

        #[test]
        fn test_move_move() {
            compose_test!(
                [
                    tree_diff!("A", mov("B", 0, "root", 0)),
                    tree_diff!("A", mov("C", 0, "B", 0))
                ],
                [tree_diff!("A", mov("C", 0, "root", 0))]
            );
        }

        #[test]
        fn test_delete_create_same() {
            compose_test!(
                [
                    tree_diff!("A", delete("B", 0)),
                    tree_diff!("A", create("B", 0))
                ],
                []
            );
        }

        #[test]
        fn test_delete_create() {
            compose_test!(
                [
                    tree_diff!("A", delete("B", 0)),
                    tree_diff!("A", create("B", 1))
                ],
                [tree_diff!("A", mov("B", 1, "B", 0))]
            );
        }

        #[test]
        fn test_delete_create_move() {
            compose_test!(
                [
                    tree_diff!("A", delete("B", 0)),
                    tree_diff!("A", create("root", 1)),
                    tree_diff!("A", mov("C", 0, "B", 0))
                ],
                [tree_diff!("A", mov("C", 0, "B", 0))]
            );
        }

        #[test]
        fn test_create_delete_index() {
            compose_test!(
                [
                    tree_diff!("A", create("root", 0)),
                    tree_diff!("B", create("root", 1)),
                    tree_diff!("C", create("root", 0)),
                    tree_diff!("A", delete("root", 0)),
                ],
                [
                    tree_diff!("B", create("root", 0)),
                    tree_diff!("C", create("root", 0)),
                ]
            );
        }

        #[test]
        fn test_delete_create_index() {
            compose_test!(
                [
                    tree_diff!("A", delete("root", 0)),
                    tree_diff!("B", create("root", 1)),
                    tree_diff!("C", create("root", 1)),
                    tree_diff!("A", create("root", 0))
                ],
                [
                    tree_diff!("B", create("root", 2)),
                    tree_diff!("C", create("root", 2)),
                ]
            );
        }
        #[test]
        fn test_delete_create_index2() {
            compose_test!(
                [
                    tree_diff!("A", delete("root", 0)),
                    tree_diff!("B", create("root", 1)),
                    tree_diff!("C", create("root", 1)),
                    tree_diff!("A", create("root", 3))
                ],
                [
                    tree_diff!("B", create("root", 2)),
                    tree_diff!("C", create("root", 2)),
                    tree_diff!("A", mov("root", 3, "root", 0))
                ]
            );
        }
    }
}

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
        old_position: FractionalIndex,
    },
    Delete {
        old_parent: TreeParentId,
        old_index: usize,
        old_position: FractionalIndex,
    },
}

impl TreeDiff {
    #[allow(clippy::let_and_return)]
    pub(crate) fn compose(self, other: Self) -> Self {
        println!("\ncompose \n{:?} \n{:?}", self, other);
        let mut temp_tree = compose::TempTree::new();
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

/// Since we use undo-do-redo to implement the movable tree, when there are concurrent edits,
/// redundant events are highly likely to occur. For example, concurrent create operations will
/// emit redundant delete-delete-delete-create-create-create events, leading to inefficient event
/// sys at the application layer.
///
/// To solve this problem, we need to detect the redundant events and skip them and avoid the cycle move
/// at the same time.
///
/// We use the following method to solve this problem:
/// -
mod compose {

    use std::{cell::RefCell, cmp::Ordering, collections::BTreeSet};

    use super::*;
    #[derive(Debug)]
    pub struct TempTree {
        tree: FxHashMap<Option<TreeID>, RefCell<Node>>,
        deleted: FxHashMap<TreeID, Node>,
        // record the id that only appears once
        only_once_ids: FxHashSet<TreeID>,
        // record the id that first appears
        first_appear_event_order: FxHashSet<usize>,
        // record the id that move to root or delete finally
        move_to_root_or_delete_ids: FxHashSet<TreeID>,
    }

    #[derive(Debug, Clone)]
    struct Node {
        id: TreeID,
        children: Children,
        // the order of the event in batch
        event_order: usize,
        // continuous delete-create-delete-create will be cancelled out
        // cancel_out: bool,
        // record the old info of the node, we need this to transform the event
        old_info: Option<(TreeParentId, usize, FractionalIndex)>,
        // // record the times of the node appear, if cancel_out is true, we need to reset it to 0
        // appear_times: usize,
        alive: bool,
        old_info_changed: bool,
        diff: Option<TreeExternalDiff>,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct IdAndCausal {
        id: TreeID,
        event_order: usize,
    }

    impl Ord for IdAndCausal {
        fn cmp(&self, other: &Self) -> Ordering {
            self.event_order.cmp(&other.event_order)
        }
    }

    impl PartialOrd for IdAndCausal {
        fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
            Some(self.cmp(other))
        }
    }

    #[derive(Debug, Clone, Default)]
    struct Children {
        ids: BTreeSet<IdAndCausal>,
        index: FxHashMap<TreeID, usize>,
    }

    impl Children {
        fn insert(&mut self, id: TreeID, sort_index: usize) {
            self.ids.insert(IdAndCausal {
                id,
                event_order: sort_index,
            });
            self.index.insert(id, sort_index);
        }

        fn remove(&mut self, id: TreeID) -> bool {
            if let Some(&index) = self.index.get(&id) {
                self.ids.remove(&IdAndCausal {
                    id,
                    event_order: index,
                });
                self.index.remove(&id);
                true
            } else {
                false
            }
        }
    }

    impl Node {
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

        fn old_index_mut(&mut self) -> Option<&mut usize> {
            match &mut self.diff {
                Some(TreeExternalDiff::Move { old_index, .. }) => Some(old_index),
                Some(TreeExternalDiff::Delete { old_index, .. }) => Some(old_index),
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
                    if self.alive {
                        TreeExternalDiff::Move {
                            parent,
                            index,
                            position,
                            old_parent: self.old_info.as_ref().unwrap().0,
                            old_index: self.old_info.as_ref().unwrap().1,
                            old_position: self.old_info.unwrap().2,
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
                    old_position,
                } => {
                    if self.alive {
                        if parent == old_parent && index == old_index && position == old_position {
                            return None;
                        }
                        TreeExternalDiff::Move {
                            parent,
                            index,
                            position,
                            old_parent: self.old_info.as_ref().unwrap().0,
                            old_index: self.old_info.as_ref().unwrap().1,
                            old_position: self.old_info.unwrap().2,
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
                    old_position,
                } => {
                    if self.alive {
                        TreeExternalDiff::Delete {
                            old_parent: self
                                .old_info
                                .as_ref()
                                .map(|(x, _, _)| *x)
                                .unwrap_or(old_parent),
                            old_index: self
                                .old_info
                                .as_ref()
                                .map(|(_, y, _)| *y)
                                .unwrap_or(old_index),
                            old_position: self
                                .old_info
                                .as_ref()
                                .map(|(_, _, z)| z.clone())
                                .unwrap_or(old_position),
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

    enum CancelOutResult {
        CancelOut,
        Fail,
        Merge(TreeDiffItem),
    }

    impl TempTree {
        pub fn new() -> Self {
            let mut tree = FxHashMap::default();
            tree.insert(
                None,
                RefCell::new(Node {
                    // unused
                    id: TreeID::delete_root(),
                    children: Children::default(),
                    event_order: 0,
                    alive: true,
                    diff: None,
                    old_info: None,
                    old_info_changed: false,
                }),
            );
            Self {
                tree,
                deleted: FxHashMap::default(),
                only_once_ids: FxHashSet::default(),
                first_appear_event_order: FxHashSet::default(),
                move_to_root_or_delete_ids: FxHashSet::default(),
            }
        }

        // after apply
        fn create_side_effect(
            &mut self,
            target: TreeID,
            parent: TreeParentId,
            index: usize,
            last_event_order: usize,
        ) {
            if self.only_once_ids.contains(&target) {
                return;
            }
            // 第一次出现，移动到 root 或者 delete 不用消除副作用
            if self.first_appear_event_order.contains(&last_event_order)
                && (parent == TreeParentId::Root)
            {
                return;
            }

            // index
            let parent_id = match parent {
                TreeParentId::Node(tree_id) => Some(tree_id),
                TreeParentId::Root => None,
                TreeParentId::Deleted | TreeParentId::Unexist => unreachable!(),
            };
            let parent_node = self.tree.get(&parent_id).unwrap();
            let children = &parent_node.borrow().children;
            for child in children.ids.range(
                IdAndCausal {
                    id: target,
                    event_order: last_event_order,
                }..,
            ) {
                // 在causal_index之后的事件
                let other_node = self.tree.get(&Some(child.id)).unwrap();
                if other_node.borrow().diff.is_none() {
                    // 过滤临时的节点
                    continue;
                }
                if other_node.borrow().index().unwrap() > index {
                    // 如果index大于当前index，则需要消除副作用，相当于没有创建过节点
                    *other_node.borrow_mut().index_mut().unwrap() -= 1;
                }
            }

            // old index
            // 遍历所有节点，如果 old parent 是 parent，需要消除副作用
            for node in self.tree.values() {
                if node.borrow().event_order > last_event_order
                    || node.borrow().old_info_changed
                    || node.borrow().old_info.is_none()
                    || node.borrow().old_info.as_ref().unwrap().0 != parent
                {
                    // 已经记录 old info 不会影响
                    continue;
                }

                // 如果 old parent 是 parent，需要消除副作用
                if let Some(old_index) = node.borrow_mut().old_index_mut() {
                    if *old_index > index {
                        *old_index -= 1;
                    }
                }
            }

            for node in self.deleted.values_mut() {
                if node.event_order > last_event_order
                    || node.old_info_changed
                    || node.old_info.is_none()
                    || node.old_info.as_ref().unwrap().0 != parent
                {
                    // 已经记录 old info 不会影响
                    continue;
                }

                // 如果 old parent 是 parent，需要消除副作用
                if let Some(old_index) = node.old_index_mut() {
                    if *old_index > index {
                        *old_index -= 1;
                    }
                }
            }
        }

        fn delete_side_effect(
            &mut self,
            target: TreeID,
            old_parent: TreeParentId,
            old_index: usize,
            last_event_order: usize,
        ) {
            if self.only_once_ids.contains(&target)
                || self.first_appear_event_order.contains(&last_event_order)
            {
                return;
            }

            let parent_id = match old_parent {
                TreeParentId::Node(tree_id) => Some(tree_id),
                TreeParentId::Root => None,
                TreeParentId::Deleted | TreeParentId::Unexist => unreachable!(),
            };
            let parent_node = self.tree.get(&parent_id).unwrap();
            let children = &parent_node.borrow().children;
            for child in children.ids.range(
                IdAndCausal {
                    id: target,
                    event_order: last_event_order,
                }..,
            ) {
                // 在causal_index之后的事件
                let other_node = self.tree.get(&Some(child.id)).unwrap();
                if other_node.borrow().diff.is_none() {
                    // 过滤临时的节点
                    continue;
                }
                if other_node.borrow().index().unwrap() > old_index {
                    // 如果index大于当前index，则需要消除副作用，相当于没有创建过节点
                    *other_node.borrow_mut().index_mut().unwrap() += 1;
                }
            }

            // old index
            // 遍历所有节点，如果 old parent 是 parent，需要消除副作用
            for node in self.tree.values() {
                if node.borrow().event_order > last_event_order
                    || node.borrow().old_info_changed
                    || node.borrow().old_info.is_none()
                    || node.borrow().old_info.as_ref().unwrap().0 != old_parent
                {
                    // 已经记录 old info 不会影响
                    continue;
                }

                // 如果 old parent 是 parent，需要消除副作用
                if let Some(i) = node.borrow_mut().old_index_mut() {
                    if *i > old_index {
                        *i -= 1;
                    }
                }
            }

            for node in self.deleted.values_mut() {
                if node.event_order > last_event_order
                    || node.old_info_changed
                    || node.old_info.is_none()
                    || node.old_info.as_ref().unwrap().0 != old_parent
                {
                    // 已经记录 old info 不会影响
                    continue;
                }

                // 如果 old parent 是 parent，需要消除副作用
                if let Some(i) = node.old_index_mut() {
                    if *i > old_index {
                        *i += 1;
                    }
                }
            }
        }

        pub fn apply(&mut self, TreeDiffItem { target, action }: TreeDiffItem, sort_index: usize) {
            match &action {
                TreeExternalDiff::Create { .. } => {
                    self.create(target, action, sort_index);
                }
                TreeExternalDiff::Move {
                    old_parent,
                    old_index,
                    old_position,
                    ..
                } => {
                    self.delete(
                        target,
                        *old_parent,
                        *old_index,
                        old_position.clone(),
                        sort_index,
                    );
                    self.create(target, action, sort_index);
                }
                TreeExternalDiff::Delete {
                    old_parent,
                    old_index,
                    old_position,
                } => {
                    self.delete(
                        target,
                        *old_parent,
                        *old_index,
                        old_position.clone(),
                        sort_index,
                    );
                }
            }
        }

        // action could be create or move, if this is the second time to appear, we can assume the node in deleted
        fn create(&mut self, target: TreeID, action: TreeExternalDiff, sort_index: usize) {
            let (parent, index, position) = match &action {
                TreeExternalDiff::Create {
                    parent,
                    index,
                    position,
                    ..
                } => (*parent, *index, position.clone()),
                TreeExternalDiff::Move {
                    parent,
                    index,
                    position,
                    ..
                } => (*parent, *index, position.clone()),
                _ => unreachable!(),
            };

            let maybe_deleted = self.deleted.remove(&target);
            let side_effect = maybe_deleted.is_some();
            {
                // get the children of the parent
                let children = match parent {
                    TreeParentId::Root => &mut self.tree.get(&None).unwrap().borrow_mut().children,
                    TreeParentId::Node(id) => {
                        if !self.tree.contains_key(&Some(id)) {
                            self.tree
                                .get(&None)
                                .unwrap()
                                .borrow_mut()
                                .children
                                .insert(id, sort_index);
                            self.tree.insert(
                                Some(id),
                                RefCell::new(Node {
                                    id,
                                    children: Children::default(),
                                    diff: None,
                                    old_info: None,
                                    alive: true,
                                    event_order: sort_index,
                                    old_info_changed: false,
                                }),
                            );
                        }
                        &mut self.tree.get(&Some(id)).unwrap().borrow_mut().children
                    }
                    TreeParentId::Unexist | TreeParentId::Deleted => unreachable!(),
                };

                children.insert(target, sort_index);
            }

            let node = Node {
                id: target,
                children: Children::default(),
                diff: Some(action),
                event_order: sort_index,
                old_info_changed: maybe_deleted.is_some(),
                alive: maybe_deleted.is_some() && maybe_deleted.as_ref().unwrap().alive,
                old_info: maybe_deleted.and_then(|x| x.old_info),
            };
            self.tree.insert(Some(target), RefCell::new(node));
            if side_effect {
                self.create_side_effect(target, parent, index, sort_index);
            }
        }

        fn delete(
            &mut self,
            target: TreeID,
            old_parent: TreeParentId,
            old_index: usize,
            old_position: FractionalIndex,
            sort_index: usize,
        ) {
            // Check if previous events are affected. For example, if A was created first, then B was created after target,
            // when A is deleted, B's index needs to be decremented by 1
            match old_parent {
                TreeParentId::Root => {
                    if !self
                        .tree
                        .get(&None)
                        .unwrap()
                        .borrow_mut()
                        .children
                        .remove(target)
                    {
                        self.deleted.insert(
                            target,
                            Node {
                                id: target,
                                children: Children::default(),
                                old_info: Some((old_parent, old_index, old_position.clone())),
                                diff: Some(TreeExternalDiff::Delete {
                                    old_parent,
                                    old_index,
                                    old_position,
                                }),
                                event_order: sort_index,
                                alive: true,
                                old_info_changed: false,
                            },
                        );
                        return;
                    }
                    let node = self.tree.remove(&Some(target)).unwrap();
                    self.deleted.insert(
                        target,
                        Node {
                            id: target,
                            children: Children::default(),
                            event_order: sort_index,
                            diff: Some(TreeExternalDiff::Delete {
                                old_parent,
                                old_index,
                                old_position: old_position.clone(),
                            }),
                            alive: node.borrow().alive,
                            // maybe the node is temporary created, so we need to change the old_info
                            old_info: node.borrow().old_info.clone().or(Some((
                                old_parent,
                                old_index,
                                old_position,
                            ))),
                            old_info_changed: node.borrow().old_info.is_some(),
                        },
                    );
                    self.delete_side_effect(
                        target,
                        old_parent,
                        old_index,
                        node.borrow().event_order,
                    );
                }
                TreeParentId::Node(id) => {
                    // just for ownership of parent
                    if let Some(parent) = self.tree.remove(&Some(id)) {
                        if !parent.borrow_mut().children.remove(target) {
                            // Parent exists but target is not in batch
                            self.deleted.insert(
                                target,
                                Node {
                                    id: target,
                                    children: Children::default(),
                                    alive: true,
                                    diff: Some(TreeExternalDiff::Delete {
                                        old_parent,
                                        old_index,
                                        old_position: old_position.clone(),
                                    }),
                                    event_order: sort_index,
                                    old_info: Some((old_parent, old_index, old_position)),
                                    old_info_changed: false,
                                },
                            );
                            self.tree.insert(Some(id), parent);
                            return;
                        }
                        self.tree
                            .get(&None)
                            .unwrap()
                            .borrow_mut()
                            .children
                            .remove(target);
                        let node = self.tree.remove(&Some(target)).unwrap();
                        assert!(node.borrow().index().unwrap() == old_index);
                        self.deleted.insert(
                            target,
                            Node {
                                id: target,
                                children: Children::default(),
                                event_order: sort_index,
                                diff: Some(TreeExternalDiff::Delete {
                                    old_parent,
                                    old_index,
                                    old_position: old_position.clone(),
                                }),
                                old_info: node.borrow().old_info.clone().or(Some((
                                    old_parent,
                                    old_index,
                                    old_position,
                                ))),
                                alive: node.borrow().alive,
                                old_info_changed: node.borrow().old_info.is_some(),
                            },
                        );
                        self.tree.insert(Some(id), parent);
                        self.delete_side_effect(
                            target,
                            old_parent,
                            old_index,
                            node.borrow().event_order,
                        );
                    } else {
                        self.deleted.insert(
                            target,
                            Node {
                                id: target,
                                children: Children::default(),
                                alive: true,
                                diff: Some(TreeExternalDiff::Delete {
                                    old_parent,
                                    old_index,
                                    old_position: old_position.clone(),
                                }),
                                event_order: sort_index,
                                old_info: Some((old_parent, old_index, old_position)),
                                old_info_changed: false,
                            },
                        );
                    }
                }
                TreeParentId::Unexist | TreeParentId::Deleted => unreachable!(),
            };
        }

        pub fn into_diff(mut self) -> TreeDiff {
            println!(
                "move_to_root_or_delete_ids: {:?}",
                self.move_to_root_or_delete_ids
            );
            let mut diffs = vec![];
            let mut old_info = FxHashMap::default();
            for (_, node) in self
                .move_to_root_or_delete_ids
                .iter()
                .filter_map(|x| {
                    self.tree.remove(&Some(*x)).map(|x| {
                        let node = x.into_inner();
                        (node.event_order, node)
                    })
                })
                .sorted_by_key(|x| x.0)
            {
                if let Some(diff) = node.into_diff() {
                    diffs.push(diff);
                }
            }
            for node in self.deleted.into_values().sorted_by_key(|x| x.event_order) {
                if let Some(diff) = node.into_diff() {
                    diffs.push(diff);
                }
            }

            let mut stack = Vec::new();
            let mut need_move_to_root_first = vec![];
            let mut left_ids = vec![];

            println!("diffs: {:?}", diffs);

            // Push all root nodes to stack initially
            for root_id in self.tree.get(&None).unwrap().borrow().children.ids.iter() {
                stack.push(root_id.id);
            }

            while let Some(node_id) = stack.pop() {
                let Some(node) = self.tree.get(&Some(node_id)) else {
                    continue;
                };

                // Push children to stack in reverse order
                // so they are processed in correct order
                for child_id in node.borrow().children.ids.iter() {
                    stack.push(child_id.id);
                }
                if node.borrow().diff.is_none() {
                    continue;
                }
                if !self.move_to_root_or_delete_ids.contains(&node_id) {
                    left_ids.push(node_id);
                    need_move_to_root_first.push(node_id);
                } else if !self.only_once_ids.contains(&node_id) {
                    need_move_to_root_first.push(node_id);
                }
            }

            println!("need_move_to_root_first: {:?}", need_move_to_root_first);
            println!("left_ids: {:?}", left_ids);
            println!("tree: {:?}", self.tree);

            for (order, id) in need_move_to_root_first.iter().copied().enumerate() {
                let node = self.tree.get(&Some(id)).unwrap();
                diffs.push(TreeDiffItem {
                    target: id,
                    action: if node.borrow().old_info.is_some() {
                        TreeExternalDiff::Move {
                            parent: TreeParentId::Root,
                            index: 0,
                            position: FractionalIndex::default(),
                            old_parent: node.borrow().old_info.as_ref().unwrap().0,
                            old_index: node.borrow().old_info.as_ref().unwrap().1,
                            old_position: node.borrow().old_info.as_ref().unwrap().2.clone(),
                        }
                    } else {
                        node.borrow_mut().alive = true;
                        node.borrow_mut().old_info =
                            Some((TreeParentId::Root, 0, FractionalIndex::default()));
                        TreeExternalDiff::Create {
                            parent: TreeParentId::Root,
                            index: 0,
                            position: FractionalIndex::default(),
                        }
                    },
                });
                old_info.insert(id, need_move_to_root_first.len() - 1 - order);
            }
            for id in left_ids {
                let mut node = self.tree.remove(&Some(id)).unwrap().into_inner();
                if need_move_to_root_first.contains(&id) {
                    match &mut node.diff {
                        Some(TreeExternalDiff::Move {
                            old_parent,
                            old_index,
                            old_position,
                            ..
                        })
                        | Some(TreeExternalDiff::Delete {
                            old_parent,
                            old_index,
                            old_position,
                        }) => {
                            *old_parent = TreeParentId::Root;
                            *old_index = old_info[&id];
                            *old_position = FractionalIndex::default();
                        }

                        _ => {}
                    }
                }
                if let Some(diff) = node.into_diff() {
                    diffs.push(diff);
                }
            }

            TreeDiff { diff: diffs }
        }

        fn filter_cancel_out(mut diffs: Vec<TreeDiffItem>) -> Vec<TreeDiffItem> {
            let mut i = 0;
            let mut j = 1;
            while j < diffs.len() {
                match can_cancel_out(&diffs[i], &diffs[j]) {
                    CancelOutResult::CancelOut => {
                        diffs.remove(j);
                        diffs.remove(i);
                        // maybe deleteA deleteB createB createA, we need to cancel out
                        i = i.saturating_sub(1);
                    }
                    CancelOutResult::Fail => {
                        i += 1;
                    }
                    CancelOutResult::Merge(tree_diff_item) => {
                        diffs.remove(j);
                        diffs[i] = tree_diff_item;
                    }
                }
                j = i + 1;
            }
            println!("#### diffs: {:?}\n", diffs);
            diffs
        }

        #[allow(dead_code)]
        fn apply_diffs(&mut self, diffs: Vec<TreeDiffItem>) {
            let diffs = Self::filter_cancel_out(diffs);
            let mut visited = FxHashSet::default();
            for (order, diff) in diffs.iter().enumerate() {
                if visited.contains(&diff.target) {
                    self.only_once_ids.remove(&diff.target);
                } else {
                    visited.insert(diff.target);
                    self.only_once_ids.insert(diff.target);
                    self.first_appear_event_order.insert(order);
                }
                if let TreeExternalDiff::Move {
                    parent: TreeParentId::Root,
                    ..
                } = &diff.action
                {
                    self.move_to_root_or_delete_ids.insert(diff.target);
                } else if let TreeExternalDiff::Delete { .. } = &diff.action {
                    self.move_to_root_or_delete_ids.insert(diff.target);
                } else {
                    self.move_to_root_or_delete_ids.remove(&diff.target);
                }
            }
            for (sort_index, diff) in diffs.into_iter().enumerate() {
                self.apply(diff, sort_index);
            }
        }
    }

    fn can_cancel_out(a: &TreeDiffItem, b: &TreeDiffItem) -> CancelOutResult {
        if a.target != b.target {
            return CancelOutResult::Fail;
        }
        match (&a.action, &b.action) {
            (
                TreeExternalDiff::Create { .. },
                TreeExternalDiff::Move {
                    parent,
                    index,
                    position,
                    ..
                },
            ) => CancelOutResult::Merge(TreeDiffItem {
                target: a.target,
                action: TreeExternalDiff::Create {
                    parent: *parent,
                    index: *index,
                    position: position.clone(),
                },
            }),
            (TreeExternalDiff::Create { .. }, TreeExternalDiff::Delete { .. }) => {
                CancelOutResult::CancelOut
            }
            (
                TreeExternalDiff::Move {
                    old_parent,
                    old_index,
                    old_position,
                    ..
                },
                TreeExternalDiff::Move {
                    parent,
                    index,
                    position,
                    ..
                },
            ) => CancelOutResult::Merge(TreeDiffItem {
                target: a.target,
                action: TreeExternalDiff::Move {
                    parent: *parent,
                    index: *index,
                    position: position.clone(),
                    old_parent: *old_parent,
                    old_index: *old_index,
                    old_position: old_position.clone(),
                },
            }),
            (
                TreeExternalDiff::Move {
                    old_parent,
                    old_index,
                    old_position,
                    ..
                },
                TreeExternalDiff::Delete { .. },
            ) => CancelOutResult::Merge(TreeDiffItem {
                target: a.target,
                action: TreeExternalDiff::Delete {
                    old_parent: *old_parent,
                    old_index: *old_index,
                    old_position: old_position.clone(),
                },
            }),
            (
                TreeExternalDiff::Delete {
                    old_parent,
                    old_index,
                    old_position,
                },
                TreeExternalDiff::Create {
                    parent,
                    index,
                    position,
                },
            ) => {
                if old_parent == parent && old_index == index {
                    CancelOutResult::CancelOut
                } else {
                    CancelOutResult::Merge(TreeDiffItem {
                        target: a.target,
                        action: TreeExternalDiff::Move {
                            parent: *parent,
                            index: *index,
                            position: position.clone(),
                            old_parent: *old_parent,
                            old_index: *old_index,
                            old_position: old_position.clone(),
                        },
                    })
                }
            }
            _ => CancelOutResult::Fail,
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
                        old_position: FractionalIndex::default(),
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
                        old_position: FractionalIndex::default(),
                    },
                }
            };
        }

        #[macro_export]
        macro_rules! compose_test {
            ([$($input:expr),* $(,)?], [$($expected:expr),* $(,)?]) => {
                {
                    let mut tree = TempTree::new();
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
        fn test_create_delete2() {
            compose_test!(
                [
                    tree_diff!("A", create("B", 0)),
                    tree_diff!("C", create("A", 0)),
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
                [tree_diff!("A", delete("root", 0)),]
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

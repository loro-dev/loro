use fxhash::{FxHashMap, FxHashSet};
use itertools::Itertools;
use loro_common::{ContainerID, LoroError, LoroResult, LoroTreeError, LoroValue, TreeID};
use serde::{Deserialize, Serialize};
use std::collections::{hash_map::Iter, VecDeque};
use std::sync::Arc;

use crate::delta::{TreeDiff, TreeDiffItem, TreeExternalDiff};
use crate::diff_calc::TreeDeletedSetTrait;
use crate::event::InternalDiff;
use crate::DocState;
use crate::{
    arena::SharedArena,
    container::tree::tree_op::TreeOp,
    delta::TreeInternalDiff,
    event::{Index, UnresolvedDiff},
    op::RawOp,
};

use super::ContainerState;

/// The state of movable tree.
///
/// using flat representation
#[derive(Debug, Clone)]
pub struct TreeState {
    pub(crate) trees: FxHashMap<TreeID, Option<TreeID>>,
    pub(crate) deleted: FxHashSet<TreeID>,
    in_txn: bool,
    undo_items: Vec<TreeUndoItem>,
}

#[derive(Debug, Clone, Copy)]
struct TreeUndoItem {
    target: TreeID,
    old_parent: Option<TreeID>,
}

impl TreeState {
    pub fn new() -> Self {
        let mut trees = FxHashMap::default();
        trees.insert(TreeID::delete_root().unwrap(), None);
        trees.insert(TreeID::unexist_root().unwrap(), None);
        let mut deleted = FxHashSet::default();
        deleted.insert(TreeID::delete_root().unwrap());
        Self {
            trees,
            deleted,
            in_txn: false,
            undo_items: Vec::new(),
        }
    }

    pub fn mov(&mut self, target: TreeID, parent: Option<TreeID>) -> Result<(), LoroError> {
        let Some(parent) = parent else {
            // new root node
            let old_parent = self
                .trees
                .insert(target, None)
                .unwrap_or(TreeID::unexist_root());
            self.update_deleted_cache(target, None, old_parent);
            if self.in_txn {
                self.undo_items.push(TreeUndoItem {
                    target,
                    old_parent: TreeID::unexist_root(),
                })
            }
            return Ok(());
        };
        if !self.contains(parent) {
            return Err(LoroTreeError::TreeNodeParentNotFound(parent).into());
        }
        if self.is_ancestor_of(&target, &parent) {
            return Err(LoroTreeError::CyclicMoveError.into());
        }
        if self
            .trees
            .get(&target)
            .copied()
            .unwrap_or(TreeID::unexist_root())
            == Some(parent)
        {
            return Ok(());
        }
        // move or delete or create children node
        let old_parent = self
            .trees
            .insert(target, Some(parent))
            .unwrap_or(TreeID::unexist_root());
        self.update_deleted_cache(target, Some(parent), old_parent);

        if self.in_txn {
            self.undo_items.push(TreeUndoItem { target, old_parent })
        }

        Ok(())
    }

    #[inline(never)]
    fn is_ancestor_of(&self, maybe_ancestor: &TreeID, node_id: &TreeID) -> bool {
        if !self.trees.contains_key(maybe_ancestor) {
            return false;
        }
        if maybe_ancestor == node_id {
            return true;
        }

        let mut node_id = node_id;
        loop {
            let parent = self.trees.get(node_id).unwrap();
            match parent {
                Some(parent_id) if parent_id == maybe_ancestor => return true,
                Some(parent_id) if parent_id == node_id => panic!("loop detected"),
                Some(parent_id) => {
                    node_id = parent_id;
                }
                None => return false,
            }
        }
    }

    pub fn iter(&self) -> Iter<'_, TreeID, Option<TreeID>> {
        self.trees.iter()
    }

    pub fn contains(&self, target: TreeID) -> bool {
        if TreeID::is_deleted_root(Some(target)) {
            return true;
        }
        !self.is_deleted(&target)
    }

    pub fn parent(&self, target: TreeID) -> Option<Option<TreeID>> {
        if self.is_deleted(&target) {
            None
        } else {
            self.trees.get(&target).copied()
        }
    }

    fn is_deleted(&self, target: &TreeID) -> bool {
        self.deleted.contains(target)
    }

    pub fn nodes(&self) -> Vec<TreeID> {
        self.trees
            .keys()
            .filter(|&k| !self.is_deleted(k) && !TreeID::is_unexist_root(Some(*k)))
            .copied()
            .collect::<Vec<_>>()
    }

    #[cfg(feature = "test_utils")]
    pub fn max_counter(&self) -> i32 {
        self.trees
            .keys()
            .filter(|&k| !self.is_deleted(k) && !TreeID::is_unexist_root(Some(*k)))
            .map(|k| k.counter)
            .max()
            .unwrap_or(0)
    }
}

impl ContainerState for TreeState {
    fn apply_diff_and_convert(
        &mut self,
        diff: crate::event::InternalDiff,
        _arena: &SharedArena,
    ) -> UnresolvedDiff {
        if let InternalDiff::Tree(tree) = &diff {
            // assert never cause cycle move
            for diff in tree.diff.iter() {
                let target = diff.target;
                let parent = match diff.action {
                    TreeInternalDiff::Create
                    | TreeInternalDiff::Restore
                    | TreeInternalDiff::AsRoot => None,
                    TreeInternalDiff::Move(parent)
                    | TreeInternalDiff::CreateMove(parent)
                    | TreeInternalDiff::RestoreMove(parent) => Some(parent),
                    TreeInternalDiff::Delete => TreeID::delete_root(),
                    TreeInternalDiff::UnCreate => {
                        // delete it from state
                        self.trees.remove(&target);
                        continue;
                    }
                };
                let old_parent = self
                    .trees
                    .insert(target, parent)
                    .unwrap_or(TreeID::unexist_root());
                if parent != old_parent {
                    self.update_deleted_cache(target, parent, old_parent);
                }
            }
        }
        let ans = diff
            .into_tree()
            .unwrap()
            .diff
            .into_iter()
            .flat_map(TreeDiffItem::from_delta_item)
            .collect_vec();
        UnresolvedDiff::Tree(TreeDiff { diff: ans })
    }

    fn apply_op(
        &mut self,
        raw_op: &RawOp,
        _op: &crate::op::Op,
        _arena: &SharedArena,
    ) -> LoroResult<()> {
        match raw_op.content {
            crate::op::RawOpContent::Tree(tree) => {
                let TreeOp { target, parent, .. } = tree;
                self.mov(target, parent)
            }
            _ => unreachable!(),
        }
    }

    fn to_diff(&mut self) -> UnresolvedDiff {
        let mut diffs = vec![];
        // TODO: perf
        let forest = Forest::from_tree_state(&self.trees);
        let mut q = VecDeque::from(forest.roots);
        while let Some(node) = q.pop_front() {
            let action = if let Some(parent) = node.parent {
                diffs.push(TreeDiffItem {
                    target: node.id,
                    action: TreeExternalDiff::Create,
                });
                TreeExternalDiff::Move(Some(parent))
            } else {
                TreeExternalDiff::Create
            };
            let diff = TreeDiffItem {
                target: node.id,
                action,
            };
            diffs.push(diff);
            q.extend(node.children);
        }

        UnresolvedDiff::Tree(TreeDiff { diff: diffs })
    }

    fn start_txn(&mut self) {
        self.in_txn = true;
    }

    fn abort_txn(&mut self) {
        self.in_txn = false;
        while let Some(op) = self.undo_items.pop() {
            let TreeUndoItem { target, old_parent } = op;
            if TreeID::is_unexist_root(old_parent) {
                self.trees.remove(&target);
            } else {
                let parent = self
                    .trees
                    .insert(target, old_parent)
                    .unwrap_or(TreeID::unexist_root());
                self.update_deleted_cache(target, old_parent, parent);
            }
        }
    }

    fn commit_txn(&mut self) {
        self.undo_items.clear();
        self.in_txn = false;
    }

    fn get_value(&mut self) -> LoroValue {
        let mut ans = vec![];
        #[cfg(feature = "test_utils")]
        // The order keep consistent
        let iter = self.trees.iter().sorted();
        #[cfg(not(feature = "test_utils"))]
        let iter = self.trees.iter();
        for (target, parent) in iter {
            if !self.deleted.contains(target) && !TreeID::is_unexist_root(Some(*target)) {
                let mut t = FxHashMap::default();
                t.insert("id".to_string(), target.id().to_string().into());
                let p = parent
                    .map(|p| p.to_string().into())
                    .unwrap_or(LoroValue::Null);
                t.insert("parent".to_string(), p);
                t.insert(
                    "meta".to_string(),
                    target.associated_meta_container().into(),
                );
                ans.push(t.into());
            }
        }
        ans.into()
    }

    /// Get the index of the child container
    fn get_child_index(&self, id: &ContainerID) -> Option<Index> {
        let id = id.as_normal().unwrap();
        Some(Index::Node(TreeID {
            peer: *id.0,
            counter: *id.1,
        }))
    }

    fn get_child_containers(&self) -> Vec<ContainerID> {
        self.nodes()
            .into_iter()
            .map(|n| n.associated_meta_container())
            .collect_vec()
    }
}

impl TreeDeletedSetTrait for TreeState {
    fn deleted(&self) -> &FxHashSet<TreeID> {
        &self.deleted
    }

    fn deleted_mut(&mut self) -> &mut FxHashSet<TreeID> {
        &mut self.deleted
    }

    fn get_children(&self, target: TreeID) -> Vec<TreeID> {
        let mut ans = Vec::new();
        for (t, parent) in self.trees.iter() {
            if let Some(p) = parent {
                if p == &target {
                    ans.push(*t);
                }
            }
        }
        ans
    }
}

/// Convert flatten tree structure to hierarchy for user interface.
///
/// ```json
/// {
///     "roots": [......],
///     "deleted": [......]
/// }
/// ```
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Forest {
    pub roots: Vec<TreeNode>,
    deleted: Vec<TreeNode>,
}

/// The node with metadata in hierarchy tree structure.
#[derive(Debug, Serialize, Deserialize)]
pub struct TreeNode {
    id: TreeID,
    meta: LoroValue,
    parent: Option<TreeID>,
    children: Vec<TreeNode>,
}

impl Forest {
    pub(crate) fn from_tree_state(state: &FxHashMap<TreeID, Option<TreeID>>) -> Self {
        let mut forest = Self::default();
        let mut node_to_children = FxHashMap::default();

        for (id, parent) in state.iter().sorted() {
            if let Some(parent) = parent {
                node_to_children
                    .entry(*parent)
                    .or_insert_with(Vec::new)
                    .push(*id)
            }
        }

        for root in state
            .iter()
            .filter(|(_, parent)| parent.is_none())
            .map(|(id, _)| *id)
            .sorted()
        {
            if root == TreeID::unexist_root().unwrap() {
                continue;
            }
            let mut stack = vec![(
                root,
                TreeNode {
                    id: root,
                    parent: None,
                    meta: LoroValue::Container(root.associated_meta_container()),
                    children: vec![],
                },
            )];
            let mut id_to_node = FxHashMap::default();
            while let Some((id, mut node)) = stack.pop() {
                if let Some(children) = node_to_children.get(&id) {
                    let mut children_to_stack = Vec::new();
                    for child in children {
                        if let Some(child_node) = id_to_node.remove(child) {
                            node.children.push(child_node);
                        } else {
                            children_to_stack.push((
                                *child,
                                TreeNode {
                                    id: *child,
                                    parent: Some(id),
                                    meta: LoroValue::Container(child.associated_meta_container()),
                                    children: vec![],
                                },
                            ));
                        }
                    }
                    if !children_to_stack.is_empty() {
                        stack.push((id, node));
                        stack.extend(children_to_stack);
                    } else {
                        id_to_node.insert(id, node);
                    }
                } else {
                    id_to_node.insert(id, node);
                }
            }
            let root_node = id_to_node.remove(&root).unwrap();
            if root_node.id == TreeID::delete_root().unwrap() {
                forest.deleted = root_node.children;
            } else {
                forest.roots.push(root_node);
            }
        }
        forest
    }
}

// convert map container to LoroValue
#[allow(clippy::ptr_arg)]
pub(crate) fn get_meta_value(nodes: &mut Vec<LoroValue>, state: &mut DocState) {
    for node in nodes.iter_mut() {
        let map = Arc::make_mut(node.as_map_mut().unwrap());
        let meta = map.get_mut("meta").unwrap();
        let id = meta.as_container().unwrap();
        *meta = state.get_container_deep_value(state.arena.id_to_idx(id).unwrap());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const ID1: TreeID = TreeID {
        peer: 0,
        counter: 0,
    };
    const ID2: TreeID = TreeID {
        peer: 0,
        counter: 1,
    };
    const ID3: TreeID = TreeID {
        peer: 0,
        counter: 2,
    };
    const ID4: TreeID = TreeID {
        peer: 0,
        counter: 3,
    };

    #[test]
    fn test_tree_state() {
        let mut state = TreeState::new();
        state.mov(ID1, None).unwrap();
        state.mov(ID2, Some(ID1)).unwrap();
    }

    #[test]
    fn tree_convert() {
        let mut state = TreeState::new();
        state.mov(ID1, None).unwrap();
        state.mov(ID2, Some(ID1)).unwrap();
        let roots = Forest::from_tree_state(&state.trees);
        let json = serde_json::to_string(&roots).unwrap();
        assert_eq!(
            json,
            r#"{"roots":[{"id":{"peer":0,"counter":0},"meta":{"Container":{"Normal":{"peer":0,"counter":0,"container_type":"Map"}}},"parent":null,"children":[{"id":{"peer":0,"counter":1},"meta":{"Container":{"Normal":{"peer":0,"counter":1,"container_type":"Map"}}},"parent":{"peer":0,"counter":0},"children":[]}]}],"deleted":[]}"#
        )
    }

    #[test]
    fn delete_node() {
        let mut state = TreeState::new();
        state.mov(ID1, None).unwrap();
        state.mov(ID2, Some(ID1)).unwrap();
        state.mov(ID3, Some(ID2)).unwrap();
        state.mov(ID4, Some(ID1)).unwrap();
        state.mov(ID2, TreeID::delete_root()).unwrap();
        let roots = Forest::from_tree_state(&state.trees);
        let json = serde_json::to_string(&roots).unwrap();
        assert_eq!(
            json,
            r#"{"roots":[{"id":{"peer":0,"counter":0},"meta":{"Container":{"Normal":{"peer":0,"counter":0,"container_type":"Map"}}},"parent":null,"children":[{"id":{"peer":0,"counter":3},"meta":{"Container":{"Normal":{"peer":0,"counter":3,"container_type":"Map"}}},"parent":{"peer":0,"counter":0},"children":[]}]}],"deleted":[{"id":{"peer":0,"counter":1},"meta":{"Container":{"Normal":{"peer":0,"counter":1,"container_type":"Map"}}},"parent":{"peer":18446744073709551615,"counter":2147483647},"children":[{"id":{"peer":0,"counter":2},"meta":{"Container":{"Normal":{"peer":0,"counter":2,"container_type":"Map"}}},"parent":{"peer":0,"counter":1},"children":[]}]}]}"#
        )
    }
}

use fxhash::{FxHashMap, FxHashSet};
use itertools::Itertools;
use loro_common::{
    ContainerID, ContainerType, LoroError, LoroResult, LoroTreeError, LoroValue, TreeID, ID,
};
use serde::{Deserialize, Serialize};
use std::collections::{hash_map::Iter, VecDeque};
use std::sync::Arc;

use crate::delta::{TreeDelta, TreeDiff};
use crate::DocState;
use crate::{
    arena::SharedArena,
    container::tree::tree_op::TreeOp,
    delta::TreeDiffItem,
    event::{Diff, Index},
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
        let Some(parent) = parent else{
            // new root node
            let old_parent  = self.trees.insert(target, None);
            self.update_deleted_cache(target, None, old_parent);
            if self.in_txn {
                self.undo_items.push(TreeUndoItem {
                    target,
                    // TODO: use UnExistRoot?
                    old_parent: TreeID::delete_root(),
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
        if *self.trees.get(&target).unwrap() == Some(parent) {
            return Ok(());
        }
        // move or delete or create children node
        let old_parent = self.trees.insert(target, Some(parent));
        self.update_deleted_cache(target, Some(parent), old_parent);

        if self.in_txn {
            self.undo_items.push(TreeUndoItem {
                target,
                // TODO: use UnExistRoot?
                old_parent: old_parent.unwrap_or(TreeID::delete_root()),
            })
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

    /// Get the first-level children of the target node
    pub fn children(&self, target: TreeID) -> Vec<TreeID> {
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

    fn is_deleted(&self, target: &TreeID) -> bool {
        self.deleted.contains(target)
    }

    fn update_deleted_cache(
        &mut self,
        target: TreeID,
        parent: Option<TreeID>,
        old_parent: Option<Option<TreeID>>,
    ) {
        if parent.is_some() && self.is_deleted(&parent.unwrap()) {
            self.update_deleted_cache_inner(target, true);
        } else if let Some(old_parent) = old_parent.flatten() {
            if self.is_deleted(&old_parent) {
                self.update_deleted_cache_inner(target, false);
            }
        }
    }

    fn update_deleted_cache_inner(&mut self, target: TreeID, set_children_deleted: bool) {
        let mut s = self.children(target);
        while let Some(child) = s.pop() {
            if set_children_deleted {
                self.deleted.insert(child);
            } else {
                self.deleted.remove(&child);
            }
            s.extend(self.children(child))
        }
    }

    pub fn nodes(&self) -> Vec<TreeID> {
        self.trees
            .keys()
            .filter(|&k| !self.is_deleted(k))
            .copied()
            .collect::<Vec<_>>()
    }

    #[cfg(feature = "test_utils")]
    pub fn max_counter(&self) -> i32 {
        self.trees
            .keys()
            .filter(|&k| !self.is_deleted(k))
            .map(|k| k.counter)
            .max()
            .unwrap_or(0)
    }
}

impl ContainerState for TreeState {
    fn apply_diff(&mut self, diff: &mut Diff, _arena: &SharedArena) -> LoroResult<()> {
        if let Diff::Tree(tree) = diff {
            // assert never cause cycle move
            for diff in tree.diff.iter() {
                let target = diff.target;
                let parent = match diff.action {
                    TreeDiffItem::CreateOrRestore => None,
                    TreeDiffItem::Move(parent) => Some(parent),
                    TreeDiffItem::Delete => TreeID::delete_root(),
                };
                let old_parent = self.trees.insert(target, parent);
                if Some(parent) != old_parent {
                    self.update_deleted_cache(target, parent, old_parent);
                }
            }
        }
        Ok(())
    }

    fn apply_op(&mut self, op: RawOp, _arena: &SharedArena) -> LoroResult<()> {
        match op.content {
            crate::op::RawOpContent::Tree(tree) => {
                let TreeOp { target, parent, .. } = tree;
                self.mov(target, parent)
            }
            _ => unreachable!(),
        }
    }

    fn to_diff(&self) -> Diff {
        let mut diffs = vec![];
        // TODO: perf
        let forest = Forest::from_tree_state(&self.trees);
        let mut q = VecDeque::from(forest.roots);
        while let Some(node) = q.pop_front() {
            let action = if let Some(parent) = node.parent {
                TreeDiffItem::Move(parent)
            } else {
                TreeDiffItem::CreateOrRestore
            };
            let diff = TreeDiff {
                target: node.id,
                action,
            };
            diffs.push(diff);
            q.extend(node.children);
        }

        let mut q = VecDeque::from(forest.deleted);
        while let Some(node) = q.pop_front() {
            let action = if let Some(parent) = node.parent {
                TreeDiffItem::Move(parent)
            } else {
                unreachable!()
            };
            let diff = TreeDiff {
                target: node.id,
                action,
            };
            diffs.push(diff);
            q.extend(node.children);
        }

        Diff::Tree(TreeDelta { diff: diffs })
    }

    fn start_txn(&mut self) {
        self.in_txn = true;
    }

    fn abort_txn(&mut self) {
        self.in_txn = false;
        while let Some(op) = self.undo_items.pop() {
            let TreeUndoItem { target, old_parent } = op;
            self.mov(target, old_parent).unwrap();
        }
    }

    fn commit_txn(&mut self) {
        self.undo_items.clear();
        self.in_txn = false;
    }

    // TODO: whether the node in deleted exists in the current version
    // when checkout to a past version, deleted may have some nodes from the future.
    fn get_value(&self) -> LoroValue {
        let forest = Forest::from_tree_state(&self.trees);
        forest.to_value()
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
            .map(|n| ContainerID::new_normal(n.id(), ContainerType::Map))
            .collect_vec()
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
    pub(crate) fn from_tree_state_and_meta(
        state: FxHashMap<TreeID, (Option<TreeID>, LoroValue)>,
    ) -> Self {
        let mut forest = Self::default();
        let mut node_to_children = FxHashMap::default();
        let mut node_to_meta = FxHashMap::default();
        let mut node_to_parent = FxHashMap::default();

        for (id, (parent, meta)) in state.into_iter().sorted_by_key(|(k, _)| *k) {
            node_to_meta.insert(id, meta);
            node_to_parent.insert(id, parent);
            if let Some(parent) = parent {
                node_to_children
                    .entry(parent)
                    .or_insert_with(Vec::new)
                    .push(id)
            }
        }

        for root in node_to_parent
            .iter()
            .filter(|(_, parent)| parent.is_none())
            .map(|(id, _)| *id)
            .sorted()
        {
            let mut stack = vec![(
                root,
                TreeNode {
                    id: root,
                    parent: None,
                    meta: node_to_meta.remove(&root).unwrap().clone(),
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
                                    meta: node_to_meta.remove(child).unwrap().clone(),
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
            let mut stack = vec![(
                root,
                TreeNode {
                    id: root,
                    parent: None,
                    meta: LoroValue::Container(ContainerID::new_normal(
                        root.id(),
                        ContainerType::Map,
                    )),
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
                                    meta: LoroValue::Container(ContainerID::new_normal(
                                        child.id(),
                                        ContainerType::Map,
                                    )),
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

    fn to_state(&self) -> FxHashMap<TreeID, (Option<TreeID>, LoroValue)> {
        let mut ans = FxHashMap::default();
        for root in self.roots.iter() {
            let mut stack = vec![root];
            while let Some(node) = stack.pop() {
                ans.insert(node.id, (node.parent, node.meta.clone()));
                stack.extend(node.children.iter())
            }
        }
        ans.insert(TreeID::delete_root().unwrap(), (None, LoroValue::Null));
        for root in self.deleted.iter() {
            let mut stack = vec![root];
            while let Some(node) = stack.pop() {
                ans.insert(node.id, (node.parent, node.meta.clone()));
                stack.extend(node.children.iter())
            }
        }
        ans
    }

    // for test only
    pub(crate) fn apply_diffs(&self, diff: &[Diff]) -> Self {
        let mut state = self.to_state();
        for item in diff {
            for diff in item.as_tree().unwrap().diff.iter() {
                let target = diff.target;
                let meta = if let Some((_, meta)) = state.remove(&target) {
                    meta
                } else {
                    ContainerType::Map.default_value()
                };
                match diff.action {
                    TreeDiffItem::CreateOrRestore => {
                        state.insert(target, (None, meta));
                    }
                    TreeDiffItem::Move(parent) => {
                        state.insert(target, (Some(parent), meta));
                    }
                    TreeDiffItem::Delete => {
                        state.insert(target, (TreeID::delete_root(), meta));
                    }
                }
            }
        }
        Self::from_tree_state_and_meta(state)
    }

    pub(crate) fn to_value(&self) -> LoroValue {
        let mut ans = FxHashMap::default();
        ans.insert(
            "roots".to_string(),
            self.roots.iter().map(|r| r.to_value()).collect_vec().into(),
        );
        ans.insert(
            "deleted".to_string(),
            self.deleted
                .iter()
                .map(|r| r.to_value())
                .collect_vec()
                .into(),
        );
        ans.into()
    }

    // for test only
    pub(crate) fn from_value(value: LoroValue) -> LoroResult<Self> {
        let mut map = Arc::try_unwrap(value.into_map().unwrap()).unwrap();
        // TODO: perf
        let roots = map
            .remove("roots")
            .unwrap()
            .into_list()
            .unwrap()
            .as_ref()
            .iter()
            .cloned()
            .map(TreeNode::from_value)
            .collect_vec();
        let deleted = if let Some(deleted) = map.remove("deleted") {
            deleted
                .into_list()
                .unwrap()
                .iter()
                .cloned()
                .map(TreeNode::from_value)
                .collect_vec()
        } else {
            vec![]
        };
        Ok(Self { roots, deleted })
    }
}

impl TreeNode {
    // for test only
    fn from_value(value: LoroValue) -> Self {
        let map = value.into_map().unwrap();
        let id = map.get("id").unwrap().clone().into_string().unwrap();
        let id = TreeID::from_id(ID::try_from(id.as_str()).unwrap());
        let parent = {
            match map.get("parent").unwrap() {
                LoroValue::Null => None,
                LoroValue::String(str) => {
                    Some(TreeID::from_id(ID::try_from(str.as_str()).unwrap()))
                }
                _ => unreachable!(),
            }
        };
        let meta = map.get("meta").unwrap().clone();
        let children = map
            .get("children")
            .unwrap()
            .clone()
            .into_list()
            .unwrap()
            .iter()
            .cloned()
            .map(TreeNode::from_value)
            .collect_vec();
        Self {
            id,
            meta,
            parent,
            children,
        }
    }

    fn to_value(&self) -> LoroValue {
        let mut ans = FxHashMap::default();
        ans.insert("id".to_string(), self.id.id().to_string().into());
        if let Some(p) = &self.parent {
            ans.insert("parent".to_string(), p.id().to_string().into());
        } else {
            ans.insert("parent".to_string(), LoroValue::Null);
        }
        ans.insert("meta".to_string(), self.meta.clone());

        ans.insert(
            "children".to_string(),
            self.children
                .iter()
                .map(|c| c.to_value())
                .collect_vec()
                .into(),
        );
        LoroValue::Map(Arc::new(ans))
    }
}

// convert map container to LoroValue
pub(crate) fn get_meta_value(nodes: &mut LoroValue, state: &DocState) {
    for node in Arc::make_mut(nodes.as_list_mut().unwrap()).iter_mut() {
        let map = Arc::make_mut(node.as_map_mut().unwrap());
        let meta = map.get_mut("meta").unwrap();
        let id = meta.as_container().unwrap();
        *meta = state.get_container_deep_value(state.arena.id_to_idx(id).unwrap());
        let children = map.get_mut("children").unwrap();
        get_meta_value(children, state)
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

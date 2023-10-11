use std::{
    collections::hash_map::Iter,
    ops::{Deref, DerefMut},
};

use fxhash::FxHashMap;
use loro_common::{LoroError, LoroResult, LoroValue, TreeID, DELETED_TREE_ROOT};
use serde::{Deserialize, Serialize};

use crate::{
    arena::SharedArena,
    container::{idx::ContainerIdx, tree::tree_op::TreeOp},
    event::Diff,
    op::RawOp,
};

use super::ContainerState;

// TODO: use arena save TreeID
#[derive(Debug, Clone)]
pub struct TreeState {
    idx: ContainerIdx,
    trees: FxHashMap<TreeID, Option<TreeID>>,
    in_txn: bool,
    undo_items: Vec<TreeUndoItem>,
}

#[derive(Debug, Clone)]
struct TreeUndoItem {
    target: TreeID,
    old_parent: Option<TreeID>,
}

impl TreeState {
    pub fn new(idx: ContainerIdx) -> Self {
        let mut trees = FxHashMap::default();
        trees.insert(DELETED_TREE_ROOT.unwrap(), None);
        Self {
            idx,
            trees,
            in_txn: false,
            undo_items: Vec::new(),
        }
    }

    pub fn mov(&mut self, target: TreeID, parent: Option<TreeID>) -> Result<(), LoroError> {
        let mut deleted = false;
        let mut contained = false;

        if let Some(old_parent) = self.trees.get_mut(&target) {
            contained = true;
            if old_parent
                .map(|p| self.is_deleted(p).unwrap())
                .unwrap_or(false)
            {
                deleted = true;
            }
        }

        let Some(parent) = parent else{
            if deleted{
                // the node exists but is deleted, now want to create it.
                // keep the deleted state
                return Ok(());
            }
            // new root node
            self.trees.insert(target, parent);
            if self.in_txn {
                self.undo_items.push(TreeUndoItem {
                    target,
                    old_parent: DELETED_TREE_ROOT,
                })
            }
            return Ok(());
        };

        if contained {
            if self.is_ancestor_of(&target, &parent) {
                return Err(LoroError::CyclicMoveError);
            }
            if *self.trees.get(&target).unwrap() == Some(parent) {
                return Ok(());
            }
            // move or delete
            let old_parent = self.trees.get_mut(&target).unwrap().replace(parent);
            if self.in_txn {
                self.undo_items.push(TreeUndoItem { target, old_parent })
            }
        } else {
            // new children node
            self.trees.insert(target, Some(parent));
            if self.in_txn {
                self.undo_items.push(TreeUndoItem {
                    target,
                    old_parent: DELETED_TREE_ROOT,
                })
            }
        }

        Ok(())
    }

    #[inline(never)]
    fn is_ancestor_of(&self, maybe_ancestor: &TreeID, node_id: &TreeID) -> bool {
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

    pub fn delete(&mut self, target: TreeID) {
        // deletion never occurs CycleMoveError
        self.mov(target, DELETED_TREE_ROOT).unwrap()
    }

    pub fn is_deleted(&self, target: TreeID) -> Option<bool> {
        self.trees.get(&target).map(|&p| p == DELETED_TREE_ROOT)
    }

    pub fn iter(&self) -> Iter<'_, TreeID, Option<TreeID>> {
        self.trees.iter()
    }

    pub fn contains(&self, target: TreeID) -> bool {
        self.trees.contains_key(&target)
    }

    #[cfg(feature = "test_utils")]
    pub fn max_counter(&self) -> i32 {
        self.trees.keys().map(|k| k.counter).max().unwrap_or(0)
    }
}

impl ContainerState for TreeState {
    fn apply_diff(&mut self, diff: &mut Diff, _arena: &SharedArena) -> LoroResult<()> {
        if let Diff::Tree(tree) = diff {
            for (target, parent) in &tree.diff {
                if let Some(p) = parent {
                    self.mov(*target, *p)?
                } else {
                    self.delete(*target);
                }
            }
        }
        Ok(())
    }

    fn apply_op(&mut self, op: RawOp, _arena: &SharedArena) -> LoroResult<()> {
        match op.content {
            crate::op::RawOpContent::Tree(tree) => {
                let TreeOp { target, parent } = tree;
                self.mov(target, parent)
            }
            _ => unreachable!(),
        }
    }

    fn to_diff(&self) -> Diff {
        todo!()
    }

    fn start_txn(&mut self) {
        self.in_txn = true;
    }

    fn abort_txn(&mut self) {
        self.in_txn = false;
        while let Some(op) = self.undo_items.pop() {
            let TreeUndoItem { target, old_parent } = op;
            // TODO: not delete? new node?
            if old_parent == DELETED_TREE_ROOT {
                self.trees.remove(&target);
                continue;
            }
            self.mov(target, old_parent).unwrap();
        }
    }

    fn commit_txn(&mut self) {
        self.undo_items.clear();
        self.in_txn = false;
    }

    fn get_value(&self) -> LoroValue {
        let forest = Forest::from_tree_state(&self.trees);
        forest.to_json().into()
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Forest(Vec<TreeNode>);

#[derive(Debug, Serialize, Deserialize)]
pub struct TreeNode {
    id: TreeID,
    parent: Option<TreeID>,
    children: Vec<TreeNode>,
}

impl Forest {
    pub(crate) fn from_tree_state(state: &FxHashMap<TreeID, Option<TreeID>>) -> Self {
        let mut forest = Self::default();
        let mut node_to_children = FxHashMap::default();

        for (id, parent) in state
            .iter()
            .filter(|(_, &parent)| parent != DELETED_TREE_ROOT)
        {
            if let Some(parent) = parent {
                node_to_children
                    .entry(*parent)
                    .or_insert_with(Vec::new)
                    .push(*id)
            }
        }

        for root in state
            .iter()
            .filter(|(&id, parent)| parent.is_none() && id != DELETED_TREE_ROOT.unwrap())
            .map(|(id, _)| *id)
        {
            let mut stack = vec![(
                root,
                TreeNode {
                    id: root,
                    parent: None,
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
            forest.push(id_to_node.remove(&root).unwrap());
        }
        forest
    }

    fn to_state(&self) -> FxHashMap<TreeID, Option<TreeID>> {
        let mut ans = FxHashMap::default();
        for root in self.0.iter() {
            let mut stack = vec![root];
            while let Some(node) = stack.pop() {
                ans.insert(node.id, node.parent);
                stack.extend(node.children.iter())
            }
        }
        ans
    }

    // for test
    pub(crate) fn apply_diffs(&self, diff: &[Diff]) -> Self {
        let mut state = self.to_state();
        for item in diff {
            for (&id, &parent) in item.as_tree().unwrap().diff.iter() {
                if let Some(p) = parent {
                    state.insert(id, p);
                } else {
                    state.remove(&id);
                }
            }
        }
        Self::from_tree_state(&state)
    }

    #[cfg(feature = "json")]
    pub(crate) fn from_json(json: &str) -> LoroResult<Self> {
        serde_json::from_str(json).map_err(|_| LoroError::DeserializeJsonStringError)
    }

    #[cfg(feature = "json")]
    pub(crate) fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap()
    }
}

impl Deref for Forest {
    type Target = Vec<TreeNode>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for Forest {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
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
        let mut state = TreeState::new(ContainerIdx::from_index_and_type(
            0,
            loro_common::ContainerType::Tree,
        ));
        state.mov(ID1, None).unwrap();
        state.mov(ID2, Some(ID1)).unwrap();
    }

    #[test]
    fn tree_convert() {
        let mut state = TreeState::new(ContainerIdx::from_index_and_type(
            0,
            loro_common::ContainerType::Tree,
        ));
        state.mov(ID1, None).unwrap();
        state.mov(ID2, Some(ID1)).unwrap();
        let roots = Forest::from_tree_state(&state.trees);
        let json = serde_json::to_string(&roots).unwrap();
        assert_eq!(
            json,
            r#"[{"id":{"peer":0,"counter":0},"parent":null,"children":[{"id":{"peer":0,"counter":1},"parent":{"peer":0,"counter":0},"children":[]}]}]"#
        )
    }

    #[test]
    fn delete_node() {
        let mut state = TreeState::new(ContainerIdx::from_index_and_type(
            0,
            loro_common::ContainerType::Tree,
        ));
        state.mov(ID1, None).unwrap();
        state.mov(ID2, Some(ID1)).unwrap();
        state.mov(ID3, Some(ID2)).unwrap();
        state.mov(ID4, Some(ID1)).unwrap();
        state.delete(ID2);
        let roots = Forest::from_tree_state(&state.trees);
        let json = serde_json::to_string(&roots).unwrap();
        assert_eq!(
            json,
            r#"[{"id":{"peer":0,"counter":0},"parent":null,"children":[{"id":{"peer":0,"counter":3},"parent":{"peer":0,"counter":0},"children":[]}]}]"#
        )
    }
}

use std::collections::BTreeSet;

use fxhash::FxHashMap;
use itertools::Itertools;
use loro_common::{ContainerID, HasId, IdSpan, Lamport, TreeID, ID};

use crate::{
    container::idx::ContainerIdx,
    dag::DagUtils,
    delta::{TreeDelta, TreeDeltaItem, TreeInternalDiff},
    event::InternalDiff,
    state::TreeParentId,
    version::Frontiers,
    OpLog, VersionVector,
};

use super::DiffCalculatorTrait;

#[derive(Debug)]
pub(crate) struct TreeDiffCalculator {
    container: ContainerIdx,
}

impl DiffCalculatorTrait for TreeDiffCalculator {
    fn start_tracking(&mut self, _oplog: &OpLog, _vv: &crate::VersionVector) {}

    fn apply_change(
        &mut self,
        _oplog: &OpLog,
        _op: crate::op::RichOp,
        _vv: Option<&crate::VersionVector>,
    ) {
    }

    fn stop_tracking(&mut self, _oplog: &OpLog, _vv: &crate::VersionVector) {}

    fn calculate_diff(
        &mut self,
        oplog: &OpLog,
        from: &crate::VersionVector,
        to: &crate::VersionVector,
        mut on_new_container: impl FnMut(&ContainerID),
    ) -> InternalDiff {
        let diff = self.diff(oplog, from, to);
        diff.diff.iter().for_each(|d| {
            // the metadata could be modified before, so (re)create a node need emit the map container diffs
            // `Create` here is because maybe in a diff calc uncreate and then create back
            if matches!(d.action, TreeInternalDiff::Create(_)) {
                on_new_container(&d.target.associated_meta_container())
            }
        });

        debug_log::debug_log!("\ndiff {:?}", diff);

        InternalDiff::Tree(diff)
    }
}

impl TreeDiffCalculator {
    pub(crate) fn new(container: ContainerIdx) -> Self {
        Self { container }
    }

    fn diff(&mut self, oplog: &OpLog, from: &VersionVector, to: &VersionVector) -> TreeDelta {
        self.checkout(from, oplog);
        self.checkout_diff(from, to, oplog)
    }

    fn checkout(&mut self, to: &VersionVector, oplog: &OpLog) {
        let tree_ops = oplog.op_groups.get_tree(&self.container).unwrap();
        let mut tree_cache = tree_ops.tree_for_diff.lock().unwrap();

        debug_log::group!("checkout current {:?} to {:?}", tree_cache.current_vv, to);
        if to == &tree_cache.current_vv {
            debug_log::debug_log!("checkout: to == current_vv");
            return;
        }
        let to_frontiers = to.to_frontiers(&oplog.dag);
        let min_lamport = self.get_min_lamport_by_frontiers(&to_frontiers, oplog);
        // retreat
        let mut retreat_ops = vec![];
        for (_target, ops) in tree_cache.tree.iter() {
            for op in ops.iter().rev() {
                if op.lamport < min_lamport {
                    break;
                }
                if !to.includes_id(op.id) {
                    retreat_ops.push(*op);
                }
            }
        }
        debug_log::debug_log!("retreat ops {:?}", retreat_ops);
        for op in retreat_ops {
            tree_cache.tree.get_mut(&op.target).unwrap().remove(&op);
            tree_cache.current_vv.shrink_to_exclude(IdSpan::new(
                op.id.peer,
                op.id.counter,
                op.id.counter + 1,
            ));
        }
        // forward and apply
        let current_frontiers = tree_cache.current_vv.to_frontiers(&oplog.dag);
        let forward_min_lamport = self
            .get_min_lamport_by_frontiers(&current_frontiers, oplog)
            .min(min_lamport);
        let max_lamport = self.get_max_lamport_by_frontiers(&to_frontiers, oplog);
        let mut forward_ops = vec![];
        let group = oplog
            .op_groups
            .get(&self.container)
            .unwrap()
            .as_tree()
            .unwrap();
        for (lamport, ops) in group.ops.range(forward_min_lamport..=max_lamport) {
            for op in ops {
                if !tree_cache.current_vv.includes_id(op.id_start())
                    && to.includes_id(op.id_start())
                {
                    forward_ops.push((*lamport, *op));
                }
            }
        }
        debug_log::debug_log!("forward ops {:?}", forward_ops);
        for (lamport, op) in forward_ops {
            let op = MoveLamportAndID {
                target: op.value.target,
                parent: op.value.parent_id(),
                id: op.id_start(),
                lamport,
                effected: false,
            };
            tree_cache.apply(op);
        }
    }

    fn checkout_diff(
        &mut self,
        from: &VersionVector,
        to: &VersionVector,
        oplog: &OpLog,
    ) -> TreeDelta {
        let tree_ops = oplog.op_groups.get_tree(&self.container).unwrap();
        let mut tree_cache = tree_ops.tree_for_diff.lock().unwrap();

        debug_log::group!("checkout_diff");
        let to_frontiers = to.to_frontiers(&oplog.dag);
        let from_frontiers = from.to_frontiers(&oplog.dag);
        let common_ancestors = oplog
            .dag
            .find_common_ancestor(&from_frontiers, &to_frontiers);
        let lca_vv = oplog.dag.frontiers_to_vv(&common_ancestors).unwrap();
        let lca_frontiers = lca_vv.to_frontiers(&oplog.dag);
        debug_log::debug_log!(
            "from vv {:?} to vv {:?} current vv {:?} lca vv {:?}",
            from,
            to,
            tree_cache.current_vv,
            lca_vv
        );

        let to_max_lamport = self.get_max_lamport_by_frontiers(&to_frontiers, oplog);
        let lca_min_lamport = self.get_min_lamport_by_frontiers(&lca_frontiers, oplog);

        // retreat for diff
        debug_log::debug_log!("start retreat");
        let mut diffs = vec![];
        let mut retreat_ops = vec![];
        for (_target, ops) in tree_cache.tree.iter() {
            for op in ops.iter().rev() {
                if op.lamport < lca_min_lamport {
                    break;
                }
                if !lca_vv.includes_id(op.id) {
                    retreat_ops.push(*op);
                }
            }
        }
        debug_log::debug_log!("retreat ops {:?}", retreat_ops);
        for op in retreat_ops.into_iter().sorted().rev() {
            tree_cache.tree.get_mut(&op.target).unwrap().remove(&op);
            tree_cache.current_vv.shrink_to_exclude(IdSpan::new(
                op.id.peer,
                op.id.counter,
                op.id.counter + 1,
            ));
            let (old_parent, last_effective_move_op_id) = tree_cache.get_parent_with_id(op.target);
            if op.effected {
                // we need to know whether old_parent is deleted
                let is_parent_deleted = tree_cache.is_parent_deleted(op.parent);
                let is_old_parent_deleted = tree_cache.is_parent_deleted(old_parent);
                let this_diff = TreeDeltaItem::new(
                    op.target,
                    old_parent,
                    op.parent,
                    last_effective_move_op_id,
                    is_old_parent_deleted,
                    is_parent_deleted,
                );
                diffs.push(this_diff);
                if matches!(this_diff.action, TreeInternalDiff::Create(_)) {
                    let mut s = vec![op.target];
                    while let Some(t) = s.pop() {
                        let children = tree_cache.get_children_with_id(TreeParentId::Node(t));
                        children.iter().for_each(|c| {
                            diffs.push(TreeDeltaItem {
                                target: c.0,
                                action: TreeInternalDiff::Create(TreeParentId::Node(t)),
                                last_effective_move_op_id: c.1,
                            })
                        });
                        s.extend(children.iter().map(|c| c.0));
                    }
                }
            }
        }

        // forward
        debug_log::debug_log!("forward");
        let group = oplog
            .op_groups
            .get(&self.container)
            .unwrap()
            .as_tree()
            .unwrap();
        for (lamport, ops) in group.ops.range(lca_min_lamport..=to_max_lamport) {
            for op in ops {
                if !tree_cache.current_vv.includes_id(op.id_start())
                    && to.includes_id(op.id_start())
                {
                    let op = MoveLamportAndID {
                        target: op.value.target,
                        parent: op.value.parent_id(),
                        id: op.id_start(),
                        lamport: *lamport,
                        effected: false,
                    };
                    let (old_parent, _id) = tree_cache.get_parent_with_id(op.target);
                    let is_parent_deleted = tree_cache.is_parent_deleted(op.parent);
                    let is_old_parent_deleted = tree_cache.is_parent_deleted(old_parent);
                    let effected = tree_cache.apply(op);
                    if effected {
                        let this_diff = TreeDeltaItem::new(
                            op.target,
                            op.parent,
                            old_parent,
                            op.id,
                            is_parent_deleted,
                            is_old_parent_deleted,
                        );
                        diffs.push(this_diff);
                        if matches!(this_diff.action, TreeInternalDiff::Create(_)) {
                            // TODO: per
                            let mut s = vec![op.target];
                            while let Some(t) = s.pop() {
                                let children =
                                    tree_cache.get_children_with_id(TreeParentId::Node(t));
                                children.iter().for_each(|c| {
                                    diffs.push(TreeDeltaItem {
                                        target: c.0,
                                        action: TreeInternalDiff::Create(TreeParentId::Node(t)),
                                        last_effective_move_op_id: c.1,
                                    })
                                });
                                s.extend(children.iter().map(|x| x.0));
                            }
                        }
                    }
                }
            }
        }
        TreeDelta { diff: diffs }
    }

    fn get_min_lamport_by_frontiers(&self, frontiers: &Frontiers, oplog: &OpLog) -> Lamport {
        frontiers
            .iter()
            .map(|id| oplog.get_min_lamport_at(*id))
            .min()
            .unwrap_or(0)
    }

    fn get_max_lamport_by_frontiers(&self, frontiers: &Frontiers, oplog: &OpLog) -> Lamport {
        frontiers
            .iter()
            .map(|id| oplog.get_max_lamport_at(*id))
            .max()
            .unwrap_or(Lamport::MAX)
    }
}

/// All information of an operation for diff calculating of movable tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MoveLamportAndID {
    pub(crate) lamport: Lamport,
    pub(crate) id: ID,
    pub(crate) target: TreeID,
    pub(crate) parent: TreeParentId,
    /// Whether this action is applied in the current version.
    /// If this action will cause a circular reference, then this action will not be applied.
    pub(crate) effected: bool,
}

impl PartialOrd for MoveLamportAndID {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for MoveLamportAndID {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.lamport
            .cmp(&other.lamport)
            .then_with(|| self.id.cmp(&other.id))
    }
}

impl core::hash::Hash for MoveLamportAndID {
    fn hash<H: core::hash::Hasher>(&self, ra_expand_state: &mut H) {
        let MoveLamportAndID { lamport, id, .. } = self;
        {
            lamport.hash(ra_expand_state);
            id.hash(ra_expand_state);
        }
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct TreeCacheForDiff {
    tree: FxHashMap<TreeID, BTreeSet<MoveLamportAndID>>,
    current_vv: VersionVector,
}

impl TreeCacheForDiff {
    fn is_ancestor_of(&self, maybe_ancestor: &TreeID, node_id: &TreeParentId) -> bool {
        if !self.tree.contains_key(maybe_ancestor) {
            return false;
        }
        if let TreeParentId::Node(id) = node_id {
            if id == maybe_ancestor {
                return true;
            }
        }
        match node_id {
            TreeParentId::Node(id) => {
                let (parent, _) = &self.get_parent_with_id(*id);
                if parent == node_id {
                    panic!("is_ancestor_of loop")
                }
                self.is_ancestor_of(maybe_ancestor, parent)
            }
            TreeParentId::Deleted | TreeParentId::None => false,
            TreeParentId::Unexist => unreachable!(),
        }
    }

    fn apply(&mut self, mut node: MoveLamportAndID) -> bool {
        let mut effected = true;
        if self.is_ancestor_of(&node.target, &node.parent) {
            effected = false;
        }
        node.effected = effected;
        self.tree.entry(node.target).or_default().insert(node);
        self.current_vv.set_last(node.id);
        effected
    }

    fn is_parent_deleted(&self, parent: TreeParentId) -> bool {
        match parent {
            TreeParentId::Deleted => true,
            TreeParentId::Node(id) => self.is_parent_deleted(self.get_parent_with_id(id).0),
            TreeParentId::None => false,
            TreeParentId::Unexist => false,
        }
    }

    /// get the parent of the first effected op and its id
    fn get_parent_with_id(&self, tree_id: TreeID) -> (TreeParentId, ID) {
        let mut ans = (TreeParentId::Unexist, ID::NONE_ID);
        if let Some(cache) = self.tree.get(&tree_id) {
            for op in cache.iter().rev() {
                if op.effected {
                    ans = (op.parent, op.id);
                    break;
                }
            }
        }
        ans
    }

    /// get the parent of the last effected op
    fn get_last_effective_move(&self, tree_id: TreeID) -> Option<&MoveLamportAndID> {
        if TreeID::is_deleted_root(&tree_id) {
            return None;
        }

        let mut ans = None;
        if let Some(set) = self.tree.get(&tree_id) {
            for op in set.iter().rev() {
                if op.effected {
                    ans = Some(op);
                    break;
                }
            }
        }

        ans
    }

    fn get_children_with_id(&self, parent: TreeParentId) -> Vec<(TreeID, ID)> {
        let mut ans = vec![];
        for (tree_id, _) in self.tree.iter() {
            let Some(op) = self.get_last_effective_move(*tree_id) else {
                continue;
            };

            if op.parent == parent {
                ans.push((*tree_id, op.id));
            }
        }

        ans
    }
}

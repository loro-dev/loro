use std::{
    collections::BTreeSet,
    ops::{Deref, DerefMut},
};

use fxhash::{FxHashMap, FxHashSet};
use itertools::Itertools;
use loro_common::{CounterSpan, IdSpan, TreeID, ID};

use crate::{
    change::Lamport,
    delta::{TreeDelta, TreeDeltaItem, TreeInternalDiff},
    VersionVector,
};

/// All information of an operation for diff calculating of movable tree.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Eq, Ord)]
pub struct MoveLamportAndID {
    pub(crate) lamport: Lamport,
    pub(crate) id: ID,
    pub(crate) target: TreeID,
    pub(crate) parent: Option<TreeID>,
    /// Whether this action is applied in the current version.
    /// If this action will cause a circular reference, then this action will not be applied.
    pub(crate) effected: bool,
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

/// We need cache all actions of movable tree to calculate diffs between any two versions.
///
///
#[derive(Debug, Default)]
pub struct TreeDiffCache {
    cache: Cache,
    pending: BTreeSet<MoveLamportAndID>,
    deleted: FxHashSet<TreeID>,
    all_version: VersionVector,
    current_version: VersionVector,
}

#[derive(Default)]
struct Cache(FxHashMap<TreeID, BTreeSet<MoveLamportAndID>>);

impl core::fmt::Debug for Cache {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        f.write_str("{\n")?;
        for (k, v) in self.0.iter() {
            f.write_str(&format!("  {:?}:", k))?;
            f.write_str("{\n")?;
            for m in v.iter() {
                f.write_str(&format!("    {:?}\n", m))?;
            }
            f.write_str("  }\n")?;
        }
        f.write_str("}")?;
        Ok(())
    }
}

impl Deref for Cache {
    type Target = FxHashMap<TreeID, BTreeSet<MoveLamportAndID>>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for Cache {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl TreeDiffCache {
    pub(crate) fn add_node(&mut self, node: MoveLamportAndID) {
        if !self.all_version.includes_id(node.id) {
            self.pending.insert(node);
            // assert len == 1
            self.all_version.set_last(node.id);
        }
    }

    // When we cache local ops, we can apply these directly.
    // Because importing the local op must not cause circular references, it has been checked.
    pub(crate) fn add_node_from_local(&mut self, node: MoveLamportAndID) {
        if !self.all_version.includes_id(node.id) {
            let (old_parent, _id) = self.get_parent(node.target);

            self.update_deleted_cache(node.target, node.parent, old_parent);

            self.cache.entry(node.target).or_default().insert(node);
            self.current_version.set_last(node.id);
            self.all_version.set_last(node.id);
        }
    }

    /// To calculate diff of movable tree.
    ///
    /// Firstly, Switch the cache version to the from version.
    /// And then, retreat to the `lca version` of `from version` and `to version` and record every op at the same time.
    /// Finally, apply the ops in the lamport id order. If the op will cause circular references, its `effected` will be marked false.
    pub(super) fn diff(
        &mut self,
        from: &VersionVector,
        to: &VersionVector,
        lca: &VersionVector,
        to_max_lamport: Lamport,
        lca_min_lamport: Lamport,
        from_min_max_lamport: (Lamport, Lamport),
    ) -> TreeDelta {
        self.checkout(from, from_min_max_lamport.0, from_min_max_lamport.1);
        // println!(
        //     "current vv {:?}  all vv {:?}",
        //     self.current_version, self.all_version
        // );
        // println!("cache {:?}", self.cache);
        // println!("pending {:?}", self.pending);
        // println!(
        //     "to_max_lamport {} lca_min_lamport {}",
        //     to_max_lamport, lca_min_lamport
        // );
        self.calc_diff(to, lca, to_max_lamport, lca_min_lamport)
    }

    fn calc_diff(
        &mut self,
        to: &VersionVector,
        lca: &VersionVector,
        to_max_lamport: Lamport,
        lca_min_lamport: Lamport,
    ) -> TreeDelta {
        debug_log::group!("tree calc diff");
        let mut diff = self.retreat_for_diff(lca, lca_min_lamport);

        debug_log::debug_log!("revert diff:");
        for d in diff.iter() {
            debug_log::debug_log!("    {:?}", d);
        }
        let apply_ops = self.forward(to, to_max_lamport);
        debug_log::debug_log!("apply ops {:?}", apply_ops);
        for op in apply_ops.into_iter() {
            let (old_parent, _id) = self.get_parent(op.target);
            let is_parent_deleted =
                op.parent.is_some() && self.is_deleted(op.parent.as_ref().unwrap());
            let is_old_parent_deleted =
                old_parent.is_some() && self.is_deleted(old_parent.as_ref().unwrap());
            let effected = self.apply(op);
            if effected {
                // we need to know whether op.parent is deleted
                let this_diff = TreeDeltaItem::new(
                    op.target,
                    op.parent,
                    old_parent,
                    op.id,
                    is_parent_deleted,
                    is_old_parent_deleted,
                );
                debug_log::debug_log!("    {:?}", this_diff);
                diff.push(this_diff);
                if matches!(
                    this_diff.action,
                    TreeInternalDiff::Restore | TreeInternalDiff::RestoreMove(_)
                ) {
                    // TODO: per
                    let mut s = vec![op.target];
                    while let Some(t) = s.pop() {
                        let children = self.get_children(t);
                        children.iter().for_each(|c| {
                            diff.push(TreeDeltaItem {
                                target: c.0,
                                action: TreeInternalDiff::CreateMove(t),
                                last_effective_move_op_id: c.1,
                            })
                        });
                        s.extend(children.iter().map(|x| x.0));
                    }
                }
            }
        }
        debug_log::debug_log!("diff {:?}", diff);
        TreeDelta { diff }
    }

    fn checkout(&mut self, vv: &VersionVector, min_lamport: Lamport, max_lamport: Lamport) {
        // TODO: use all as max, because from version may contain `meta op`, current version != from version
        if vv == &self.current_version {
            return;
        }
        self.retreat(vv, min_lamport);
        let apply_ops = self.forward(vv, max_lamport);
        for op in apply_ops {
            let _effected = self.apply(op);
        }
        self.current_version = vv.clone();
    }

    /// return true if this apply op has effect on the tree
    ///
    /// This method assumes that `node` has the greatest lamport value
    fn apply(&mut self, mut node: MoveLamportAndID) -> bool {
        let mut effected = true;
        if node.parent.is_some() && self.is_ancestor_of(node.target, node.parent.unwrap()) {
            effected = false;
        }
        node.effected = effected;
        let (old_parent, _id) = self.get_parent(node.target);
        self.update_deleted_cache(node.target, node.parent, old_parent);
        self.cache.entry(node.target).or_default().insert(node);
        self.current_version.set_last(node.id);
        effected
    }

    fn forward(&mut self, vv: &VersionVector, max_lamport: Lamport) -> Vec<MoveLamportAndID> {
        let mut apply_ops = Vec::new();
        // remove ops from pending, and then apply to cache
        for op in self.pending.iter().copied() {
            if op.lamport > max_lamport {
                break;
            }
            if vv.includes_id(op.id) {
                apply_ops.push(op);
            }
        }
        for op in apply_ops.iter() {
            self.pending.remove(op);
        }
        apply_ops
    }

    fn retreat(&mut self, _vv: &VersionVector, min_lamport: Lamport) {
        // remove ops from cache, and then insert to pending
        let mut retreat_ops = Vec::new();
        for (_, ops) in self.cache.iter() {
            for op in ops.iter().rev() {
                if op.lamport < min_lamport {
                    break;
                }
                // for checkout
                retreat_ops.push(*op)
            }
        }
        for op in retreat_ops.iter() {
            self.cache.get_mut(&op.target).unwrap().remove(op);
            self.pending.insert(*op);
            self.current_version.shrink_to_exclude(IdSpan {
                client_id: op.id.peer,
                counter: CounterSpan::new(op.id.counter, op.id.counter + 1),
            });
            if op.effected {
                // update deleted cache
                let (old_parent, _id) = self.get_parent(op.target);
                self.update_deleted_cache(op.target, old_parent, op.parent);
            }
        }
    }

    fn retreat_for_diff(&mut self, vv: &VersionVector, min_lamport: Lamport) -> Vec<TreeDeltaItem> {
        let mut diffs = vec![];
        // remove ops from cache, and then insert to pending
        let mut retreat_ops = Vec::new();
        for (_, ops) in self.cache.iter() {
            for op in ops.iter().rev() {
                if op.lamport < min_lamport {
                    break;
                }
                if !vv.includes_id(op.id) {
                    retreat_ops.push(*op)
                }
            }
        }
        for op in retreat_ops.iter_mut().sorted().rev() {
            let btree_set = &mut self.cache.get_mut(&op.target).unwrap();
            btree_set.remove(op);
            self.pending.insert(*op);
            self.current_version.shrink_to_exclude(IdSpan {
                client_id: op.id.peer,
                counter: CounterSpan::new(op.id.counter, op.id.counter + 1),
            });
            // calc old parent
            let (old_parent, last_effective_move_op_id) = self.get_parent(op.target);
            if op.effected {
                // we need to know whether old_parent is deleted
                let is_parent_deleted =
                    op.parent.is_some() && self.is_deleted(op.parent.as_ref().unwrap());
                let is_old_parent_deleted =
                    old_parent.is_some() && self.is_deleted(old_parent.as_ref().unwrap());
                let this_diff = TreeDeltaItem::new(
                    op.target,
                    old_parent,
                    op.parent,
                    last_effective_move_op_id,
                    is_old_parent_deleted,
                    is_parent_deleted,
                );
                self.update_deleted_cache(op.target, old_parent, op.parent);
                diffs.push(this_diff);
                if matches!(
                    this_diff.action,
                    TreeInternalDiff::Restore | TreeInternalDiff::RestoreMove(_)
                ) {
                    let mut s = vec![op.target];
                    while let Some(t) = s.pop() {
                        let children = self.get_children(t);
                        children.iter().for_each(|c| {
                            diffs.push(TreeDeltaItem {
                                target: c.0,
                                action: TreeInternalDiff::CreateMove(t),
                                last_effective_move_op_id: c.1,
                            })
                        });
                        s.extend(children.iter().map(|c| c.0));
                    }
                }
            }
        }

        diffs
    }

    /// get the parent of the first effected op
    fn get_parent(&self, tree_id: TreeID) -> (Option<TreeID>, ID) {
        if TreeID::is_deleted_root(Some(tree_id)) {
            return (None, ID::NONE_ID);
        }
        let mut ans = (TreeID::unexist_root(), ID::NONE_ID);
        if let Some(cache) = self.cache.get(&tree_id) {
            for op in cache.iter().rev() {
                if op.effected {
                    ans = (op.parent, op.id);
                    break;
                }
            }
        }

        ans
    }

    /// get the parent of the first effected op
    fn get_last_effective_move(&self, tree_id: TreeID) -> Option<&MoveLamportAndID> {
        if TreeID::is_deleted_root(Some(tree_id)) {
            return None;
        }

        let mut ans = None;
        if let Some(cache) = self.cache.get(&tree_id) {
            for op in cache.iter().rev() {
                if op.effected {
                    ans = Some(op);
                    break;
                }
            }
        }

        ans
    }

    fn is_ancestor_of(&self, maybe_ancestor: TreeID, mut node_id: TreeID) -> bool {
        if maybe_ancestor == node_id {
            return true;
        }

        loop {
            let (parent, _id) = self.get_parent(node_id);
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
}

pub(crate) trait TreeDeletedSetTrait {
    fn deleted(&self) -> &FxHashSet<TreeID>;
    fn deleted_mut(&mut self) -> &mut FxHashSet<TreeID>;
    fn get_children(&self, target: TreeID) -> Vec<(TreeID, ID)>;
    fn get_children_recursively(&self, target: TreeID) -> Vec<(TreeID, ID)> {
        let mut ans = vec![];
        let mut s = vec![target];
        while let Some(t) = s.pop() {
            let children = self.get_children(t);
            ans.extend(children.clone());
            s.extend(children.iter().map(|x| x.0));
        }
        ans
    }
    fn is_deleted(&self, target: &TreeID) -> bool {
        self.deleted().contains(target) || TreeID::is_deleted_root(Some(*target))
    }
    fn update_deleted_cache(
        &mut self,
        target: TreeID,
        parent: Option<TreeID>,
        old_parent: Option<TreeID>,
    ) {
        if parent.is_some() && self.is_deleted(&parent.unwrap()) {
            self.update_deleted_cache_inner(target, true);
        } else if let Some(old_parent) = old_parent {
            if self.is_deleted(&old_parent) {
                self.update_deleted_cache_inner(target, false);
            }
        }
    }
    fn update_deleted_cache_inner(&mut self, target: TreeID, set_children_deleted: bool) {
        if set_children_deleted {
            self.deleted_mut().insert(target);
        } else {
            self.deleted_mut().remove(&target);
        }
        let mut s = self.get_children(target);
        while let Some((child, _)) = s.pop() {
            if child == target {
                continue;
            }
            if set_children_deleted {
                self.deleted_mut().insert(child);
            } else {
                self.deleted_mut().remove(&child);
            }
            s.extend(self.get_children(child))
        }
    }
}

impl TreeDeletedSetTrait for TreeDiffCache {
    fn deleted(&self) -> &FxHashSet<TreeID> {
        &self.deleted
    }

    fn deleted_mut(&mut self) -> &mut FxHashSet<TreeID> {
        &mut self.deleted
    }

    fn get_children(&self, target: TreeID) -> Vec<(TreeID, ID)> {
        let mut ans = vec![];
        for (tree_id, _) in self.cache.iter() {
            if tree_id == &target {
                continue;
            }
            let Some(op) = self.get_last_effective_move(*tree_id) else {
                continue;
            };

            if op.parent == Some(target) {
                ans.push((*tree_id, op.id));
            }
        }

        ans
    }
}

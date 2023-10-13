use std::{
    collections::BTreeSet,
    ops::{Deref, DerefMut},
};

use fxhash::FxHashMap;
use itertools::Itertools;
use loro_common::{CounterSpan, IdSpan, TreeID, ID};

use crate::{change::Lamport, delta::TreeDelta, VersionVector};

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
    pub(crate) fn add_node_uncheck(&mut self, node: MoveLamportAndID) {
        if !self.all_version.includes_id(node.id) {
            self.cache
                .entry(node.target)
                .or_insert_with(Default::default)
                .insert(node);
            // assert len == 1
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
        // println!("\nFROM {:?} TO {:?} LCA {:?}", from, to, lca);
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
        let mut diff = Vec::new();
        let revert_ops = self.retreat_for_diff(lca, lca_min_lamport);
        for (op, old_parent) in revert_ops.iter().sorted().rev() {
            if op.effected {
                diff.push((op.target, *old_parent).into());
            }
        }
        debug_log::debug_log!("revert diff:");
        for d in diff.iter() {
            debug_log::debug_log!("    {:?}", d);
        }
        let apply_ops = self.forward(to, to_max_lamport);
        debug_log::debug_log!("apply ops {:?}", apply_ops);
        for op in apply_ops.into_iter() {
            let effected = self.apply(op);
            if effected {
                debug_log::debug_log!("    target {:?} to {:?}", op.target, op.parent);

                diff.push((op.target, op.parent).into())
            }
        }
        debug_log::debug_log!("diff {:?}", diff);
        TreeDelta { diff }
    }

    fn checkout(&mut self, vv: &VersionVector, min_lamport: Lamport, max_lamport: Lamport) {
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

    /// return true if it can be effected
    fn apply(&mut self, mut node: MoveLamportAndID) -> bool {
        let mut ans = true;
        if node.parent.is_some() && self.is_ancestor_of(node.target, node.parent.unwrap()) {
            ans = false;
        }
        node.effected = ans;
        self.cache
            .entry(node.target)
            .or_insert_with(Default::default)
            .insert(node);
        self.current_version.set_last(node.id);
        ans
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
            })
        }
    }

    fn retreat_for_diff(
        &mut self,
        vv: &VersionVector,
        min_lamport: Lamport,
    ) -> Vec<(MoveLamportAndID, Option<TreeID>)> {
        // remove ops from cache, and then insert to pending
        let mut retreat_ops = Vec::new();
        for (_, ops) in self.cache.iter() {
            for op in ops.iter().rev() {
                if op.lamport < min_lamport {
                    break;
                }
                if !vv.includes_id(op.id) {
                    retreat_ops.push((*op, None))
                }
            }
        }
        for (op, old_parent) in retreat_ops.iter_mut() {
            self.cache.get_mut(&op.target).unwrap().remove(op);
            self.pending.insert(*op);
            self.current_version.shrink_to_exclude(IdSpan {
                client_id: op.id.peer,
                counter: CounterSpan::new(op.id.counter, op.id.counter + 1),
            });
            // calc old parent
            *old_parent = self.get_parent(op.target);
        }
        retreat_ops
    }

    /// get the parent of the first effected op
    fn get_parent(&self, tree_id: TreeID) -> Option<TreeID> {
        if TreeID::is_deleted_root(Some(tree_id)) {
            return None;
        }
        let mut ans = TreeID::delete_root();
        for op in self.cache.get(&tree_id).unwrap().iter().rev() {
            if op.effected {
                ans = op.parent;
                break;
            }
        }
        ans
    }

    fn is_ancestor_of(&self, maybe_ancestor: TreeID, mut node_id: TreeID) -> bool {
        if maybe_ancestor == node_id {
            return true;
        }

        loop {
            let parent = self.get_parent(node_id);
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

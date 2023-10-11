use std::{
    collections::BTreeSet,
    ops::{Deref, DerefMut},
};

use fxhash::FxHashMap;
use itertools::Itertools;
use loro_common::{CounterSpan, IdSpan, TreeID, DELETED_TREE_ROOT, ID};

use crate::{change::Lamport, delta::TreeDelta, VersionVector};

use super::CompactTreeNode;

#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Eq, Ord)]
pub struct MoveLamportAndID {
    pub(super) lamport: Lamport,
    pub(super) id: ID,
    pub(super) target: TreeID,
    pub(super) parent: Option<TreeID>,
    pub(super) effected: bool,
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

impl CompactTreeNode {
    fn move_lamport_id(&self) -> MoveLamportAndID {
        MoveLamportAndID {
            lamport: self.lamport,
            id: self.id(),
            target: self.target,
            parent: self.parent,
            effected: true,
        }
    }

    fn id(&self) -> ID {
        ID {
            peer: self.peer,
            counter: self.counter,
        }
    }
}

impl TreeDiffCache {
    pub(crate) fn add_node(&mut self, node: &CompactTreeNode) {
        if !self.all_version.includes_id(node.id()) {
            self.apply(node.move_lamport_id());
            // assert len == 1
            self.all_version.set_last(node.id());
        }
    }

    pub(crate) fn add_node_uncheck(&mut self, node: &CompactTreeNode) {
        if !self.all_version.includes_id(node.id()) {
            self.cache
                .entry(node.target)
                .or_insert_with(Default::default)
                .insert(node.move_lamport_id());
            // assert len == 1
            self.current_version.set_last(node.id());
            self.all_version.set_last(node.id());
        }
    }

    pub(super) fn diff(
        &mut self,
        from: &VersionVector,
        to: &VersionVector,
        lca: &VersionVector,
        to_max_lamport: Lamport,
        lca_min_lamport: Lamport,
        from_min_lamport: Lamport,
        from_max_lamport: Lamport,
    ) -> TreeDelta {
        // TODO: calc min max lamport
        // println!("\nFROM {:?} TO {:?} LCA {:?}", from, to, lca);
        self.checkout(from, 0, Lamport::MAX);
        // println!(
        //     "current vv {:?}  all vv {:?}",
        //     self.current_version, self.all_version
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
        let debug = false;

        let mut diff = Vec::new();
        let revert_ops = self.retreat(lca, lca_min_lamport);
        for op in revert_ops.iter().sorted().rev() {
            if op.effected {
                let old_parent = self.get_parent(op.target);
                diff.push((op.target, old_parent));
            }
        }
        assert_eq!(&self.current_version, lca);
        if debug {
            println!("revert diff {:?}", diff);
        }

        let apply_ops = self.forward(to, to_max_lamport);
        if debug {
            println!("apply ops {:?}", apply_ops);
        }
        for op in apply_ops.into_iter() {
            let effected = self.apply(op);
            if effected {
                diff.push((op.target, op.parent))
            }
        }
        if debug {
            println!("diff {:?}", diff);
        }

        TreeDelta { diff }
    }

    // return true if it can be effected
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

    fn checkout(&mut self, vv: &VersionVector, min_lamport: Lamport, max_lamport: Lamport) {
        if vv == &self.current_version {
            return;
        }
        let _retreat_ops = self.retreat(vv, min_lamport);
        let apply_ops = self.forward(vv, max_lamport);
        for op in apply_ops {
            let _effected = self.apply(op);
        }
        self.current_version = vv.clone();
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

    fn retreat(&mut self, vv: &VersionVector, min_lamport: Lamport) -> Vec<MoveLamportAndID> {
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
        // TODO: perf
        for op in retreat_ops.iter() {
            self.cache.get_mut(&op.target).unwrap().remove(op);
            self.pending.insert(*op);
            self.current_version.shrink_to_exclude(IdSpan {
                client_id: op.id.peer,
                counter: CounterSpan::new(op.id.counter, op.id.counter + 1),
            })
        }
        retreat_ops
    }

    fn get_parent(&self, tree_id: TreeID) -> Option<TreeID> {
        if tree_id == DELETED_TREE_ROOT.unwrap() {
            return None;
        }
        let mut ans = DELETED_TREE_ROOT;
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

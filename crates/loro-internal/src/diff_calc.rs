use std::{
    collections::{BTreeMap, BTreeSet},
    ops::{Deref, DerefMut},
};

use enum_dispatch::enum_dispatch;
use fxhash::{FxHashMap, FxHashSet};
use loro_common::{HasIdSpan, PeerID, TreeID, DELETED_TREE_ROOT, ID};

use crate::{
    change::Lamport,
    container::{idx::ContainerIdx, tree::tree_op::TreeOp},
    dag::DagUtils,
    delta::{MapDelta, MapValue, TreeDelta},
    event::Diff,
    id::Counter,
    op::RichOp,
    span::{HasId, HasLamport},
    text::tracker::Tracker,
    version::Frontiers,
    InternalString, VersionVector,
};

use super::{event::InternalContainerDiff, oplog::OpLog};

/// Calculate the diff between two versions. given [OpLog][super::oplog::OpLog]
/// and [AppState][super::state::AppState].
///
/// TODO: persist diffCalculator and skip processed version
#[derive(Debug, Default)]
pub struct DiffCalculator {
    calculators: FxHashMap<ContainerIdx, ContainerDiffCalculator>,
    last_vv: VersionVector,
    has_all: bool,
}

impl DiffCalculator {
    pub fn new() -> Self {
        Self {
            calculators: Default::default(),
            last_vv: Default::default(),
            has_all: false,
        }
    }

    // PERF: if the causal order is linear, we can skip some of the calculation
    #[allow(unused)]
    pub(crate) fn calc_diff(
        &mut self,
        oplog: &super::oplog::OpLog,
        before: &crate::VersionVector,
        after: &crate::VersionVector,
    ) -> Vec<InternalContainerDiff> {
        self.calc_diff_internal(oplog, before, None, after, None)
    }

    pub(crate) fn calc_diff_internal(
        &mut self,
        oplog: &super::oplog::OpLog,
        before: &crate::VersionVector,
        before_frontiers: Option<&Frontiers>,
        after: &crate::VersionVector,
        after_frontiers: Option<&Frontiers>,
    ) -> Vec<InternalContainerDiff> {
        if self.has_all {
            let include_before = self.last_vv.includes_vv(before);
            let include_after = self.last_vv.includes_vv(after);
            if !include_after || !include_before {
                self.has_all = false;
                self.last_vv = Default::default();
            }
        }
        let affected_set = if !self.has_all {
            // if we don't have all the ops, we need to calculate the diff by tracing back
            let mut after = after;
            let mut before = before;
            let mut merged = before.clone();
            let mut before_frontiers = before_frontiers;
            let mut after_frontiers = after_frontiers;
            merged.merge(after);
            let empty_vv: VersionVector = Default::default();
            if !after.includes_vv(before) {
                // If after is not after before, we need to calculate the diff from the beginning
                //
                // This is required because of [MapDiffCalculator]. It can be removed with
                // a better data structure. See #114.
                before = &empty_vv;
                after = &merged;
                before_frontiers = None;
                after_frontiers = None;
                self.has_all = true;
                self.last_vv = Default::default();
            } else if before.is_empty() {
                self.has_all = true;
                self.last_vv = Default::default();
            }

            let (lca, iter) =
                oplog.iter_from_lca_causally(before, before_frontiers, after, after_frontiers);

            let mut started_set = FxHashSet::default();
            for (change, vv) in iter {
                if change.id.counter > 0 && self.has_all {
                    assert!(
                        self.last_vv.includes_id(change.id.inc(-1)),
                        "{:?} {}",
                        &self.last_vv,
                        change.id
                    );
                }

                if self.has_all {
                    self.last_vv.extend_to_include_end_id(change.id_end());
                }

                let mut visited = FxHashSet::default();
                for op in change.ops.iter() {
                    let calculator =
                        self.calculators.entry(op.container).or_insert_with(|| {
                            match op.container.get_type() {
                                crate::ContainerType::Text => {
                                    ContainerDiffCalculator::Text(TextDiffCalculator::default())
                                }
                                crate::ContainerType::Map => {
                                    ContainerDiffCalculator::Map(MapDiffCalculator::new())
                                }
                                crate::ContainerType::List => {
                                    ContainerDiffCalculator::List(ListDiffCalculator::default())
                                }
                                crate::ContainerType::Tree => {
                                    ContainerDiffCalculator::Tree(TreeDiffCalculator::default())
                                }
                            }
                        });

                    if !started_set.contains(&op.container) {
                        started_set.insert(op.container);
                        calculator.start_tracking(oplog, &lca);
                    }

                    if visited.contains(&op.container) {
                        // don't checkout if we have already checked out this container in this round
                        calculator.apply_change(oplog, RichOp::new_by_change(change, op), None);
                    } else {
                        calculator.apply_change(
                            oplog,
                            RichOp::new_by_change(change, op),
                            Some(&vv.borrow()),
                        );
                        visited.insert(op.container);
                    }
                }
            }
            for (_, calculator) in self.calculators.iter_mut() {
                calculator.stop_tracking(oplog, after);
            }

            Some(started_set)
        } else {
            // We can calculate the diff by the current calculators.

            // Find a set of affected containers idx, if it's relatively cheap
            if before.distance_to(after) < self.calculators.len() {
                let mut set = FxHashSet::default();
                oplog.for_each_change_within(before, after, |change| {
                    for op in change.ops.iter() {
                        set.insert(op.container);
                    }
                });
                Some(set)
            } else {
                None
            }
        };
        let mut diffs = Vec::with_capacity(self.calculators.len());
        if let Some(set) = affected_set {
            // only visit the affected containers
            for idx in set {
                let calc = self.calculators.get_mut(&idx).unwrap();
                diffs.push(InternalContainerDiff {
                    idx,
                    diff: calc.calculate_diff(oplog, before, after),
                });
            }
        } else {
            for (&idx, calculator) in self.calculators.iter_mut() {
                diffs.push(InternalContainerDiff {
                    idx,
                    diff: calculator.calculate_diff(oplog, before, after),
                });
            }
        }
        diffs
    }
}

/// DiffCalculator should track the history first before it can calculate the difference.
///
/// So we need it to first apply all the ops between the two versions.
///
/// NOTE: not every op between two versions are included in a certain container.
/// So there may be some ops that cannot be seen by the container.
///
#[enum_dispatch]
pub trait DiffCalculatorTrait {
    fn start_tracking(&mut self, oplog: &OpLog, vv: &crate::VersionVector);
    fn apply_change(
        &mut self,
        oplog: &OpLog,
        op: crate::op::RichOp,
        vv: Option<&crate::VersionVector>,
    );
    fn stop_tracking(&mut self, oplog: &OpLog, vv: &crate::VersionVector);
    fn calculate_diff(
        &mut self,
        oplog: &OpLog,
        from: &crate::VersionVector,
        to: &crate::VersionVector,
    ) -> Diff;
}

#[enum_dispatch(DiffCalculatorTrait)]
#[derive(Debug)]
enum ContainerDiffCalculator {
    Text(TextDiffCalculator),
    Map(MapDiffCalculator),
    List(ListDiffCalculator),
    Tree(TreeDiffCalculator),
}

#[derive(Default)]
struct TextDiffCalculator {
    tracker: Tracker,
}

impl std::fmt::Debug for TextDiffCalculator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TextDiffCalculator")
            // .field("tracker", &self.tracker)
            .finish()
    }
}

#[derive(Debug, Default)]
struct MapDiffCalculator {
    grouped: FxHashMap<InternalString, CompactRegister>,
}

impl MapDiffCalculator {
    pub(crate) fn new() -> Self {
        Self {
            grouped: Default::default(),
        }
    }
}

impl DiffCalculatorTrait for MapDiffCalculator {
    fn start_tracking(&mut self, _oplog: &crate::OpLog, _vv: &crate::VersionVector) {}

    fn apply_change(
        &mut self,
        _oplog: &crate::OpLog,
        op: crate::op::RichOp,
        _vv: Option<&crate::VersionVector>,
    ) {
        let map = op.op().content.as_map().unwrap();
        self.grouped
            .entry(map.key.clone())
            .or_default()
            .push(CompactMapValue {
                lamport: op.lamport(),
                peer: op.client_id(),
                counter: op.id_start().counter,
                value: op.op().content.as_map().unwrap().value,
            });
    }

    fn stop_tracking(&mut self, _oplog: &super::oplog::OpLog, _vv: &crate::VersionVector) {}

    fn calculate_diff(
        &mut self,
        oplog: &super::oplog::OpLog,
        from: &crate::VersionVector,
        to: &crate::VersionVector,
    ) -> Diff {
        let mut changed = Vec::new();
        for (k, g) in self.grouped.iter_mut() {
            let (peek_from, peek_to) = g.peek_at_ab(from, to);
            match (peek_from, peek_to) {
                (None, None) => {}
                (None, Some(_)) => changed.push((k.clone(), peek_to)),
                (Some(_), None) => changed.push((k.clone(), peek_to)),
                (Some(a), Some(b)) => {
                    if a != b {
                        changed.push((k.clone(), peek_to))
                    }
                }
            }
        }

        let mut updated = FxHashMap::with_capacity_and_hasher(changed.len(), Default::default());
        for (key, value) in changed {
            let value = value
                .map(|v| {
                    let value = v.value.and_then(|v| oplog.arena.get_value(v as usize));
                    MapValue {
                        counter: v.counter,
                        value,
                        lamport: (v.lamport, v.peer),
                    }
                })
                .unwrap_or_else(|| MapValue {
                    counter: 0,
                    value: None,
                    lamport: (0, 0),
                });
            updated.insert(key, value);
        }

        Diff::NewMap(MapDelta { updated })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct CompactMapValue {
    lamport: Lamport,
    peer: PeerID,
    counter: Counter,
    value: Option<u32>,
}

impl HasId for CompactMapValue {
    fn id_start(&self) -> ID {
        ID::new(self.peer, self.counter)
    }
}

use compact_register::CompactRegister;
mod compact_register {
    use std::collections::BTreeSet;

    use super::*;
    #[derive(Debug, Default)]
    pub(super) struct CompactRegister {
        tree: BTreeSet<CompactMapValue>,
    }

    impl CompactRegister {
        pub fn push(&mut self, value: CompactMapValue) {
            self.tree.insert(value);
        }

        pub fn peek_at_ab(
            &self,
            a: &VersionVector,
            b: &VersionVector,
        ) -> (Option<CompactMapValue>, Option<CompactMapValue>) {
            let mut max_a: Option<CompactMapValue> = None;
            let mut max_b: Option<CompactMapValue> = None;
            for v in self.tree.iter().rev() {
                if b.get(&v.peer).copied().unwrap_or(0) > v.counter {
                    max_b = Some(*v);
                    break;
                }
            }

            for v in self.tree.iter().rev() {
                if a.get(&v.peer).copied().unwrap_or(0) > v.counter {
                    max_a = Some(*v);
                    break;
                }
            }

            (max_a, max_b)
        }
    }
}

#[derive(Default)]
struct ListDiffCalculator {
    tracker: Tracker,
}

impl std::fmt::Debug for ListDiffCalculator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ListDiffCalculator")
            // .field("tracker", &self.tracker)
            .finish()
    }
}

impl DiffCalculatorTrait for ListDiffCalculator {
    fn start_tracking(&mut self, _oplog: &OpLog, vv: &crate::VersionVector) {
        if !vv.includes_vv(self.tracker.start_vv()) || !self.tracker.all_vv().includes_vv(vv) {
            self.tracker = Tracker::new(vv.clone(), Counter::MAX / 2);
        }

        self.tracker.checkout(vv);
    }

    fn apply_change(
        &mut self,
        _oplog: &OpLog,
        op: crate::op::RichOp,
        vv: Option<&crate::VersionVector>,
    ) {
        if let Some(vv) = vv {
            self.tracker.checkout(vv);
        }
        self.tracker.track_apply(&op);
    }

    fn stop_tracking(&mut self, _oplog: &OpLog, _vv: &crate::VersionVector) {}

    fn calculate_diff(
        &mut self,
        _oplog: &OpLog,
        from: &crate::VersionVector,
        to: &crate::VersionVector,
    ) -> Diff {
        Diff::SeqRaw(self.tracker.diff(from, to))
    }
}

impl DiffCalculatorTrait for TextDiffCalculator {
    fn start_tracking(&mut self, _oplog: &super::oplog::OpLog, vv: &crate::VersionVector) {
        if !vv.includes_vv(self.tracker.start_vv()) || !self.tracker.all_vv().includes_vv(vv) {
            self.tracker = Tracker::new(vv.clone(), Counter::MAX / 2);
        }

        self.tracker.checkout(vv);
    }

    fn apply_change(
        &mut self,
        _oplog: &super::oplog::OpLog,
        op: crate::op::RichOp,
        vv: Option<&crate::VersionVector>,
    ) {
        if let Some(vv) = vv {
            self.tracker.checkout(vv);
        }

        self.tracker.track_apply(&op);
    }

    fn stop_tracking(&mut self, _oplog: &super::oplog::OpLog, _vv: &crate::VersionVector) {}

    fn calculate_diff(
        &mut self,
        _oplog: &OpLog,
        from: &crate::VersionVector,
        to: &crate::VersionVector,
    ) -> Diff {
        Diff::SeqRaw(self.tracker.diff(from, to))
    }
}

#[derive(Debug, Default)]
struct TreeDiffCalculator {
    nodes: BTreeSet<CompactTreeNode>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct CompactTreeNode {
    pub(super) lamport: Lamport,
    pub(super) peer: PeerID,
    pub(super) counter: Counter,
    pub(super) target: TreeID,
    pub(super) parent: Option<TreeID>,
}

// TODO: tree
impl DiffCalculatorTrait for TreeDiffCalculator {
    fn start_tracking(&mut self, _oplog: &OpLog, _vv: &crate::VersionVector) {}

    fn apply_change(
        &mut self,
        oplog: &OpLog,
        op: crate::op::RichOp,
        _vv: Option<&crate::VersionVector>,
    ) {
        let TreeOp { target, parent } = op.op().content.as_tree().unwrap();
        let node = CompactTreeNode {
            lamport: op.lamport(),
            peer: op.client_id(),
            counter: op.id_start().counter,
            target: *target,
            parent: *parent,
        };
        let mut cache_lock = oplog.tree_parent_cache.lock().unwrap();
        match cache_lock.add_cache(&node) {
            Ok(_) => {}
            Err(_) => {
                // println!("gg {:?}", node);
            }
        }
        self.nodes.insert(node);
    }

    fn stop_tracking(&mut self, _oplog: &OpLog, _vv: &crate::VersionVector) {}

    // TODO: tree
    fn calculate_diff(
        &mut self,
        oplog: &OpLog,
        from: &crate::VersionVector,
        to: &crate::VersionVector,
    ) -> Diff {
        let tree_cache = oplog.tree_parent_cache.lock().unwrap();
        let debug = false;

        if debug {
            println!("from {:?} to {:?}", from, to);
        }
        let mut merged_vv = from.clone();
        merged_vv.merge(to);
        let from_frontiers_inner;
        let to_frontiers_inner;
        let from_frontiers = {
            from_frontiers_inner = Some(from.to_frontiers(&oplog.dag));
            from_frontiers_inner.as_ref().unwrap()
        };
        let to_frontiers = {
            to_frontiers_inner = Some(to.to_frontiers(&oplog.dag));
            to_frontiers_inner.as_ref().unwrap()
        };
        let common_ancestors = oplog.dag.find_common_ancestor(from_frontiers, to_frontiers);
        let lca_vv = oplog.dag.frontiers_to_vv(&common_ancestors).unwrap();
        if debug {
            println!("lca vv {:?}", lca_vv);
        }
        let mut latest_vv = lca_vv.clone();
        let mut need_revert_ops = Vec::new();
        let mut apply_ops = Vec::new();
        for node in self.nodes.iter() {
            let id = ID {
                peer: node.peer,
                counter: node.counter,
            };

            if from.includes_id(id) && !lca_vv.includes_id(id) {
                need_revert_ops.push(node);
                latest_vv.set_end(id);
            }
            if to.includes_id(id) && !lca_vv.includes_id(id) {
                apply_ops.push(node)
            }
        }

        let mut diff = Vec::new();

        if debug {
            println!("cache {:?}", tree_cache);
        }
        while let Some(node) = need_revert_ops.pop() {
            let target = node.target;
            let old_parent = tree_cache.get_old_parent(node, from);
            if debug {
                println!(
                    "{:?} old parent {:?}   lamport {}",
                    target, old_parent, node.lamport
                );
            }
            diff.push((target, old_parent));
        }
        if debug {
            println!("\nrevert op {:?}", diff);
        }

        for node in apply_ops {
            diff.push((node.target, node.parent));
        }
        if debug {
            println!("\ndiff {:?}", diff);
        }

        Diff::Tree(TreeDelta { diff })
    }
}

#[derive(Debug, Default)]
pub struct TreeParentCache {
    cache: Cache,
    visited: FxHashSet<(Lamport, PeerID, Counter)>,
}

#[derive(Default)]
struct Cache(FxHashMap<TreeID, BTreeMap<(Lamport, PeerID, Counter), Option<TreeID>>>);

impl core::fmt::Debug for Cache {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        f.write_str("{\n")?;
        for (k, v) in self.0.iter() {
            f.write_str(&format!("  {:?}:", k))?;
            f.write_str("{\n")?;
            for (k, v) in v.iter() {
                f.write_str(&format!("    {:?}:{:?}\n", k, v))?;
            }
            f.write_str("  }\n")?;
        }
        f.write_str("}")?;
        Ok(())
    }
}

impl Deref for Cache {
    type Target = FxHashMap<TreeID, BTreeMap<(Lamport, PeerID, Counter), Option<TreeID>>>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for Cache {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl TreeParentCache {
    pub(super) fn add_cache(&mut self, node: &CompactTreeNode) -> Result<(), ()> {
        if self
            .visited
            .contains(&(node.lamport, node.peer, node.counter))
        {
            return Ok(());
        }
        self.visited.insert((node.lamport, node.peer, node.counter));
        if let Some(parent) = node.parent {
            let nodes = self.is_ancestor_of(node.target, parent);
            if !nodes.is_empty() {
                let (id, n) = nodes
                    .into_iter()
                    .map(|n| {
                        let id = self
                            .cache
                            .get(&n)
                            .and_then(|n| n.last_key_value())
                            .map(|(k, _)| *k)
                            .unwrap();
                        (id, n)
                    })
                    .max()
                    .unwrap();
                if id > (node.lamport, node.peer, node.counter) {
                    // the last one will cause cycle move, add new old parent for this lamport time
                    let len = self.cache.get_mut(&n).unwrap().len();
                    let p = if len > 1 {
                        let (_, old_parent) =
                            self.cache.get(&n).unwrap().iter().rev().nth(1).unwrap();
                        *old_parent
                    } else {
                        DELETED_TREE_ROOT
                    };
                    self.cache
                        .get_mut(&n)
                        .unwrap()
                        .insert((node.lamport, node.peer, node.counter), p);
                    // println!("REMOVE PARENT {:?}", p.unwrap().1)
                } else {
                    return Err(());
                }
            }
        }

        let entry = self
            .cache
            .entry(node.target)
            .or_insert_with(BTreeMap::default);
        if let Some((_, v)) = entry.last_key_value() {
            if *v != node.parent {
                entry.insert((node.lamport, node.peer, node.counter), node.parent);
            }
        } else {
            entry.insert((node.lamport, node.peer, node.counter), node.parent);
        }

        Ok(())
    }

    fn get_old_parent(&self, node: &CompactTreeNode, from: &VersionVector) -> Option<TreeID> {
        let mut old_parent = DELETED_TREE_ROOT;
        if let Some(cache) = self.cache.get(&node.target) {
            for (id, parent) in cache.iter().rev() {
                if from.includes_id(ID {
                    peer: id.1,
                    counter: id.2,
                }) && *id < (node.lamport, node.peer, node.counter)
                {
                    old_parent = *parent;
                    break;
                }
            }
        }
        old_parent
    }

    fn is_ancestor_of(&self, maybe_ancestor: TreeID, mut node_id: TreeID) -> Vec<TreeID> {
        if maybe_ancestor == node_id {
            return vec![node_id];
        }

        let mut ans = vec![node_id];
        loop {
            let parent = self
                .cache
                .get(&node_id)
                .and_then(|n| n.last_key_value())
                .and_then(|(_, p)| *p);

            match parent {
                Some(parent_id) if parent_id == maybe_ancestor => return ans,
                Some(parent_id) if parent_id == node_id => panic!("loop detected"),
                Some(parent_id) => {
                    node_id = parent_id;
                    ans.push(parent_id);
                }
                None => return vec![],
            }
        }
    }
}

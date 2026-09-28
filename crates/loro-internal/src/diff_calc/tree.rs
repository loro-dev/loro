use std::{collections::BTreeSet, ops::RangeInclusive, sync::Arc};

use fractional_index::FractionalIndex;
use loro_common::{ContainerID, IdFull, IdLp, Lamport, PeerID, TreeID, ID};
use rustc_hash::FxHashMap;

use crate::{
    container::{idx::ContainerIdx, tree::tree_op::TreeOp},
    delta::{TreeDelta, TreeDeltaItem, TreeInternalDiff},
    event::InternalDiff,
    state::TreeParentId,
    version::Frontiers,
    OpLog, VersionVector,
};

use super::{DiffCalcVersionInfo, DiffCalculatorTrait, DiffMode};

#[derive(Debug)]
pub(crate) struct TreeDiffCalculator {
    container: ContainerIdx,
    mode: TreeDiffCalculatorMode,
}

#[derive(Debug)]
enum TreeDiffCalculatorMode {
    Crdt,
    Linear(TreeDelta),
    ImportGreaterUpdates(TreeDelta),
}

impl DiffCalculatorTrait for TreeDiffCalculator {
    fn start_tracking(&mut self, _oplog: &OpLog, _vv: &crate::VersionVector, mode: DiffMode) {
        match mode {
            DiffMode::Checkout => {
                self.mode = TreeDiffCalculatorMode::Crdt;
            }
            DiffMode::Import => {
                self.mode = TreeDiffCalculatorMode::Crdt;
            }
            DiffMode::ImportGreaterUpdates => {
                self.mode = TreeDiffCalculatorMode::ImportGreaterUpdates(TreeDelta::default());
            }
            DiffMode::Linear => {
                self.mode = TreeDiffCalculatorMode::Linear(TreeDelta::default());
            }
        }
    }

    fn apply_change(
        &mut self,
        _oplog: &OpLog,
        op: crate::op::RichOp,
        _vv: Option<&crate::VersionVector>,
    ) {
        match &mut self.mode {
            TreeDiffCalculatorMode::Crdt => {}
            TreeDiffCalculatorMode::Linear(ref mut delta)
            | TreeDiffCalculatorMode::ImportGreaterUpdates(ref mut delta) => {
                let id_full = op.id_full();
                let op = op.op();
                let content = op.content.as_tree().unwrap();

                let item: TreeDeltaItem = match &**content {
                    crate::container::tree::tree_op::TreeOp::Create {
                        target,
                        parent,
                        position,
                    } => TreeDeltaItem {
                        target: *target,
                        action: TreeInternalDiff::Create {
                            parent: (*parent).into(),
                            position: position.clone(),
                        },
                        last_effective_move_op_id: id_full,
                    },
                    crate::container::tree::tree_op::TreeOp::Move {
                        target,
                        parent,
                        position,
                    } => TreeDeltaItem {
                        target: *target,
                        action: TreeInternalDiff::Move {
                            parent: (*parent).into(),
                            position: position.clone(),
                        },
                        last_effective_move_op_id: id_full,
                    },
                    crate::container::tree::tree_op::TreeOp::Delete { target } => TreeDeltaItem {
                        target: *target,
                        action: TreeInternalDiff::Delete {
                            parent: TreeParentId::Deleted,
                            position: None,
                        },
                        last_effective_move_op_id: id_full,
                    },
                };

                delta.diff.push(item);
            }
        }
    }

    fn finish_this_round(&mut self) {
        self.mode = TreeDiffCalculatorMode::Crdt;
    }

    fn calculate_diff(
        &mut self,
        _idx: ContainerIdx,
        oplog: &OpLog,
        info: DiffCalcVersionInfo,
        mut on_new_container: impl FnMut(&ContainerID),
    ) -> (InternalDiff, DiffMode) {
        match &mut self.mode {
            TreeDiffCalculatorMode::Crdt => {
                let diff = self.diff(oplog, info);
                diff.diff.iter().for_each(|d| {
                    // the metadata could be modified before, so (re)create a node need emit the map container diffs
                    // `Create` here is because maybe in a diff calc uncreate and then create back
                    if matches!(d.action, TreeInternalDiff::Create { .. }) {
                        on_new_container(&d.target.associated_meta_container())
                    }
                });

                (InternalDiff::Tree(diff), DiffMode::Checkout)
            }
            TreeDiffCalculatorMode::Linear(ans) => {
                (InternalDiff::Tree(std::mem::take(ans)), DiffMode::Linear)
            }
            TreeDiffCalculatorMode::ImportGreaterUpdates(ans) => {
                let mut ans = std::mem::take(ans);
                ans.diff.sort_unstable_by(|a, b| {
                    a.last_effective_move_op_id
                        .lamport
                        .cmp(&b.last_effective_move_op_id.lamport)
                        .then_with(|| {
                            a.last_effective_move_op_id
                                .peer
                                .cmp(&b.last_effective_move_op_id.peer)
                        })
                });
                (InternalDiff::Tree(ans), DiffMode::ImportGreaterUpdates)
            }
        }
    }
}

impl TreeDiffCalculator {
    pub(crate) fn new(container: ContainerIdx) -> Self {
        Self {
            container,
            mode: TreeDiffCalculatorMode::Crdt,
        }
    }

    fn diff(&mut self, oplog: &OpLog, info: DiffCalcVersionInfo) -> TreeDelta {
        self.checkout(info.from_vv, info.from_frontiers, oplog);
        self.checkout_diff(info, oplog)
    }

    /// Moves the diff cache to `to` without recording diffs.
    fn checkout(&mut self, to: &VersionVector, to_frontiers: &Frontiers, oplog: &OpLog) {
        oplog.with_history_cache(|h| {
            let mark = h.ensure_importing_caches_exist();
            let tree_ops = h.get_tree(&self.container, mark).unwrap();
            let mut tree_cache = tree_ops.tree().lock();
            tree_cache.init_current_vv(oplog);
            let s = format!("checkout current {:?} to {:?}", &tree_cache.current_vv, &to);
            let s = tracing::span!(tracing::Level::INFO, "checkout", s = s);
            let _e = s.enter();
            if to == &tree_cache.current_vv {
                return;
            }
            let Some(min_lamport) = min_lamport_of_version_diff(&tree_cache.current_vv, to, oplog)
            else {
                tree_cache.current_vv = to.clone();
                return;
            };

            let ops = tree_ops.ops();
            let retreat = tree_cache.retreat_range(min_lamport);
            for (idlp, op) in retreat.map(|r| ops.range(r)).into_iter().flatten() {
                tree_cache.take(IdFull::new(idlp.peer, op.counter, idlp.lamport), &op.value);
            }
            tree_cache.retreated_from(min_lamport);

            let max_lamport = self
                .get_max_lamport_by_frontiers(to_frontiers, oplog)
                .max(min_lamport);
            for (idlp, op) in
                ops.range(IdLp::new(0, min_lamport)..=IdLp::new(PeerID::MAX, max_lamport))
            {
                if to.includes_id(ID::new(idlp.peer, op.counter)) {
                    tree_cache.apply(MoveLamportAndID {
                        id: IdFull::new(idlp.peer, op.counter, idlp.lamport),
                        op: op.value.clone(),
                        effected: false,
                    });
                }
            }
            tree_cache.current_vv = to.clone();
        });
    }

    fn checkout_diff(&mut self, info: DiffCalcVersionInfo, oplog: &OpLog) -> TreeDelta {
        oplog.with_history_cache(|h| {
            let mark = h.ensure_importing_caches_exist();
            let tree_ops = h.get_tree(&self.container, mark).unwrap();
            let mut tree_cache = tree_ops.tree().lock();
            debug_assert_eq!(&tree_cache.current_vv, info.from_vv);
            let mut parent_to_children_cache =
                TreeParentToChildrenCache::init_from_tree_cache(&tree_cache);
            let s = tracing::span!(tracing::Level::INFO, "checkout_diff");
            let _e = s.enter();
            // CORRECTNESS: `effected` (the cycle check) of each cached op
            // depends on every op ordered before it, so the cache must always
            // hold ops applied in (lamport, peer) order. Ops below
            // `min_lamport` are the same in `from` and `to`; everything at or
            // above it is retreated from `from` and replayed for `to` in that
            // order. The window is taken from the versions themselves, not
            // from a replay base: in `Checkout` mode `find_replay_base` may
            // return a meet that is not a critical version, and ops concurrent
            // with that meet can have lower lamports than it. See
            // `context/tree-checkout-window.md`.
            let Some(min_lamport) = min_lamport_of_version_diff(info.from_vv, info.to_vv, oplog)
            else {
                tree_cache.current_vv = info.to_vv.clone();
                return TreeDelta::default();
            };
            // `max` keeps the forward range well-formed when `to` only drops ops.
            let to_max_lamport = self
                .get_max_lamport_by_frontiers(info.to_frontiers, oplog)
                .max(min_lamport);
            let ops = tree_ops.ops();

            let mut diffs = vec![];
            // retreat, newest first
            let retreat = tree_cache.retreat_range(min_lamport);
            for (idlp, op) in retreat.map(|r| ops.range(r)).into_iter().flatten().rev() {
                let Some(op) =
                    tree_cache.take(IdFull::new(idlp.peer, op.counter, idlp.lamport), &op.value)
                else {
                    continue;
                };
                if !op.effected {
                    continue;
                }
                let (old_parent, position, last_effective_move_op_id) =
                    tree_cache.get_parent_with_id(op.op.target());
                // we need to know whether old_parent is deleted
                let is_parent_deleted = tree_cache.is_parent_deleted(op.op.parent_id());
                let is_old_parent_deleted = tree_cache.is_parent_deleted(old_parent);
                if op.op.target().id() == op.id.id() {
                    assert_eq!(
                        old_parent,
                        TreeParentId::Unexist,
                        "old_parent = {:?} instead",
                        &old_parent
                    );
                }
                parent_to_children_cache.record_change(
                    op.op.target(),
                    op.op.parent_id(),
                    old_parent,
                );
                let this_diff = TreeDeltaItem::new(
                    op.op.target(),
                    old_parent,
                    op.op.parent_id(),
                    last_effective_move_op_id,
                    is_old_parent_deleted,
                    is_parent_deleted,
                    position,
                );
                let is_create = matches!(this_diff.action, TreeInternalDiff::Create { .. });
                diffs.push(this_diff);
                if is_create {
                    tree_cache.push_children_creation(
                        op.op.target(),
                        &parent_to_children_cache,
                        &mut diffs,
                    );
                }
            }

            tree_cache.retreated_from(min_lamport);

            // forward, oldest first
            for (idlp, op) in
                ops.range(IdLp::new(0, min_lamport)..=IdLp::new(PeerID::MAX, to_max_lamport))
            {
                let id = ID::new(idlp.peer, op.counter);
                if !info.to_vv.includes_id(id) {
                    continue;
                }
                let op = MoveLamportAndID {
                    id: IdFull {
                        peer: id.peer,
                        lamport: idlp.lamport,
                        counter: id.counter,
                    },
                    op: op.value.clone(),
                    effected: false,
                };
                let (old_parent, _position, _id) = tree_cache.get_parent_with_id(op.op.target());
                let is_parent_deleted = tree_cache.is_parent_deleted(op.op.parent_id());
                let is_old_parent_deleted = tree_cache.is_parent_deleted(old_parent);
                let effected = tree_cache.apply(op.clone());
                if effected {
                    let this_diff = TreeDeltaItem::new(
                        op.op.target(),
                        op.op.parent_id(),
                        old_parent,
                        op.id_full(),
                        is_parent_deleted,
                        is_old_parent_deleted,
                        op.op.fractional_index(),
                    );
                    parent_to_children_cache.record_change(
                        op.op.target(),
                        old_parent,
                        op.op.parent_id(),
                    );
                    let is_create = matches!(this_diff.action, TreeInternalDiff::Create { .. });
                    diffs.push(this_diff);
                    if is_create {
                        tree_cache.push_children_creation(
                            op.op.target(),
                            &parent_to_children_cache,
                            &mut diffs,
                        );
                    }
                }
            }

            tree_cache.current_vv = info.to_vv.clone();
            TreeDelta { diff: diffs }
        })
    }

    fn get_max_lamport_by_frontiers(&self, frontiers: &Frontiers, oplog: &OpLog) -> Lamport {
        frontiers
            .iter()
            .map(|id| oplog.get_max_lamport_at(id))
            .max()
            .unwrap_or(Lamport::MAX)
    }
}

/// The smallest lamport among the ops that are in exactly one of `a` and `b`,
/// or `None` when both contain the same ops.
///
/// Every tree op below it is in both versions, so moving the diff cache
/// between them only has to retreat and replay the ops at or above it.
fn min_lamport_of_version_diff(
    a: &VersionVector,
    b: &VersionVector,
    oplog: &OpLog,
) -> Option<Lamport> {
    a.sub_iter(b)
        .chain(b.sub_iter(a))
        .map(|span| {
            let id = ID::new(span.peer, span.counter.start);
            oplog
                .dag
                .get_lamport(&id)
                .unwrap_or_else(|| panic!("op {id} of a tree diff version is not in the DAG"))
        })
        .min()
}

/// All information of an operation for diff calculating of movable tree.
#[derive(Debug, Clone)]
pub struct MoveLamportAndID {
    pub(crate) id: IdFull,
    pub(crate) op: Arc<TreeOp>,
    /// Whether this action is applied in the current version.
    /// If this action will cause a circular reference, then this action will not be applied.
    pub(crate) effected: bool,
}

impl MoveLamportAndID {
    fn id_full(&self) -> IdFull {
        self.id
    }
}

impl PartialEq for MoveLamportAndID {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for MoveLamportAndID {}

impl PartialOrd for MoveLamportAndID {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for MoveLamportAndID {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.id
            .lamport
            .cmp(&other.id.lamport)
            .then_with(|| self.id.peer.cmp(&other.id.peer))
    }
}

#[derive(Clone, Default)]
pub(crate) struct TreeCacheForDiff {
    tree: FxHashMap<TreeID, BTreeSet<MoveLamportAndID>>,
    current_vv: VersionVector,
    /// `false` until the first transition. In a shallow doc the cache starts
    /// at the shallow root (seeded with its state, or empty when the tree had
    /// no nodes there), which `current_vv` does not express yet.
    current_vv_initialized: bool,
    /// No cached op has a greater lamport. Bounds the retreat so it does not
    /// walk the ops above the cached version, which can be most of the
    /// history when checking out between two old versions.
    max_lamport: Lamport,
}

impl std::fmt::Debug for TreeCacheForDiff {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "TreeCacheForDiff {{ tree: ")?;
        for (id, ops) in self.tree.iter() {
            writeln!(f, "  {} -> {:?}", id, ops)?;
        }
        writeln!(f, "  current_vv: {:?}", self.current_vv)?;
        Ok(())
    }
}

impl TreeCacheForDiff {
    /// Removes the op from the cache if it is there, returning it with its
    /// `effected` flag.
    fn take(&mut self, id: IdFull, op: &Arc<TreeOp>) -> Option<MoveLamportAndID> {
        self.tree.get_mut(&op.target())?.take(&MoveLamportAndID {
            id,
            op: op.clone(),
            effected: false,
        })
    }

    /// The `IdLp` range holding every cached op whose lamport is at least
    /// `min_lamport`, or `None` when there is none.
    fn retreat_range(&self, min_lamport: Lamport) -> Option<RangeInclusive<IdLp>> {
        (self.max_lamport >= min_lamport)
            .then(|| IdLp::new(0, min_lamport)..=IdLp::new(PeerID::MAX, self.max_lamport))
    }

    /// Records that every cached op whose lamport is at least `min_lamport`
    /// has been retreated.
    fn retreated_from(&mut self, min_lamport: Lamport) {
        self.max_lamport = self.max_lamport.min(min_lamport.saturating_sub(1));
    }

    fn init_current_vv(&mut self, oplog: &OpLog) {
        if self.current_vv_initialized {
            return;
        }

        self.current_vv_initialized = true;
        if oplog.is_shallow() {
            self.current_vv = oplog
                .dag
                .frontiers_to_vv(oplog.dag.shallow_since_frontiers())
                .expect("the shallow root version must be in the DAG");
        }
    }

    /// After `target` is (re)created, pushes a `Create` for each node of its
    /// current subtree.
    fn push_children_creation(
        &self,
        target: TreeID,
        cache: &TreeParentToChildrenCache,
        diffs: &mut Vec<TreeDeltaItem>,
    ) {
        let mut s = vec![target];
        while let Some(t) = s.pop() {
            let children = self.get_children_with_id(TreeParentId::Node(t), cache);
            diffs.extend(children.iter().map(|c| TreeDeltaItem {
                target: c.0,
                action: TreeInternalDiff::Create {
                    parent: TreeParentId::Node(t),
                    position: c.1.clone().unwrap(),
                },
                last_effective_move_op_id: c.2,
            }));
            s.extend(children.iter().map(|c| c.0));
        }
    }

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
                let (parent, _, _) = &self.get_parent_with_id(*id);
                if parent == node_id {
                    panic!("is_ancestor_of loop")
                }
                self.is_ancestor_of(maybe_ancestor, parent)
            }
            TreeParentId::Deleted | TreeParentId::Root => false,
            TreeParentId::Unexist => false,
        }
    }

    fn apply(&mut self, mut node: MoveLamportAndID) -> bool {
        let mut effected = true;
        if self.is_ancestor_of(&node.op.target(), &node.op.parent_id()) {
            effected = false;
        }
        node.effected = effected;
        self.max_lamport = self.max_lamport.max(node.id.lamport);
        self.current_vv.set_last(node.id.id());
        self.tree.entry(node.op.target()).or_default().insert(node);
        effected
    }

    pub(crate) fn init_tree_with_shallow_root_version(&mut self, nodes: Vec<MoveLamportAndID>) {
        if nodes.is_empty() {
            return;
        }

        debug_assert!(self.tree.is_empty());
        for node in nodes.into_iter() {
            self.max_lamport = self.max_lamport.max(node.id.lamport);
            self.current_vv.extend_to_include_last_id(node.id.id());
            self.current_vv
                .extend_to_include_last_id(node.op.target().id());
            self.tree.entry(node.op.target()).or_default().insert(node);
        }
    }

    fn is_parent_deleted(&self, parent: TreeParentId) -> bool {
        match parent {
            TreeParentId::Deleted => true,
            TreeParentId::Node(id) => self.is_parent_deleted(self.get_parent_with_id(id).0),
            TreeParentId::Root => false,
            TreeParentId::Unexist => false,
        }
    }

    /// get the parent of the first effected op and its id
    fn get_parent_with_id(
        &self,
        tree_id: TreeID,
    ) -> (TreeParentId, Option<FractionalIndex>, IdFull) {
        let mut ans = (TreeParentId::Unexist, None, IdFull::NONE_ID);
        if let Some(cache) = self.tree.get(&tree_id) {
            for op in cache.iter().rev() {
                if op.effected {
                    ans = (
                        op.op.parent_id(),
                        op.op.fractional_index().clone(),
                        op.id_full(),
                    );
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

    fn get_children_with_id(
        &self,
        parent: TreeParentId,
        cache: &TreeParentToChildrenCache,
    ) -> Vec<(TreeID, Option<FractionalIndex>, IdFull)> {
        let Some(children_ids) = cache.get_children(parent) else {
            return vec![];
        };
        let mut ans = Vec::with_capacity(children_ids.len());
        for child in children_ids.iter() {
            let Some(op) = self.get_last_effective_move(*child) else {
                panic!("child {:?} has no last effective move", child);
            };

            assert_eq!(op.op.parent_id(), parent);
            ans.push((*child, op.op.fractional_index().clone(), op.id_full()));
        }
        // The children should be sorted by the position.
        // If the fractional index is the same, then sort by the lamport and peer.
        ans.sort_by(|a, b| {
            a.1.cmp(&b.1)
                .then(a.2.lamport.cmp(&b.2.lamport).then(a.2.peer.cmp(&b.2.peer)))
        });
        ans
    }
}

#[derive(Debug)]
struct TreeParentToChildrenCache {
    cache: FxHashMap<TreeParentId, BTreeSet<TreeID>>,
}

impl TreeParentToChildrenCache {
    fn get_children(&self, parent: TreeParentId) -> Option<&BTreeSet<TreeID>> {
        self.cache.get(&parent)
    }

    fn init_from_tree_cache(tree_cache: &TreeCacheForDiff) -> Self {
        let mut cache = Self {
            cache: FxHashMap::default(),
        };
        for (tree_id, _) in tree_cache.tree.iter() {
            let Some(op) = tree_cache.get_last_effective_move(*tree_id) else {
                continue;
            };

            cache
                .cache
                .entry(op.op.parent_id())
                .or_default()
                .insert(op.op.target());
        }
        cache
    }

    fn record_change(
        &mut self,
        target: TreeID,
        old_parent: TreeParentId,
        new_parent: TreeParentId,
    ) {
        if !old_parent.is_unexist() {
            let removed = if let Some(children) = self.cache.get_mut(&old_parent) {
                children.remove(&target)
            } else {
                false
            };

            if !removed {
                let removed = self
                    .cache
                    .values_mut()
                    .any(|children| children.remove(&target));
                assert!(
                    removed,
                    "target {target:?} should be present in TreeParentToChildrenCache before moving from {old_parent:?} to {new_parent:?}",
                );
            }
        }
        self.cache.entry(new_parent).or_default().insert(target);
    }
}

#[cfg(test)]
mod test {
    use loro_common::{TreeID, ID};

    use crate::{state::TreeParentId, version::Frontiers, HandlerTrait, LoroDoc};

    /// Checking out between two old versions must retreat only what the cache
    /// holds: `max_lamport` bounds every cached op, and stays at the cached
    /// version instead of the end of the history.
    #[test]
    fn retreat_bound_follows_the_cached_version() {
        let doc = LoroDoc::new_auto_commit();
        doc.set_peer_id(1).unwrap();
        let tree = doc.get_tree("tree");
        let nodes: Vec<TreeID> = (0..20)
            .map(|_| tree.create(TreeParentId::Root).unwrap())
            .collect();
        doc.commit_then_renew();
        let mut versions = vec![];
        for i in 0..400 {
            let _ = tree.mov(nodes[i % 20], TreeParentId::Node(nodes[(i * 7 + 3) % 20]));
            doc.commit_then_renew();
            versions.push(doc.oplog_frontiers());
        }

        let (v1, v2) = (&versions[10], &versions[20]);
        let lamport_of = |f: &Frontiers| {
            let id: ID = f.as_single().unwrap();
            doc.oplog().lock().dag.get_lamport(&id).unwrap()
        };
        for v in [v1, v2, v1, v2] {
            doc.checkout(v).unwrap();
            let (cached_max, bound) = doc.oplog().lock().with_history_cache(|h| {
                let mark = h.ensure_importing_caches_exist();
                let cache = h.get_tree(&tree.idx(), mark).unwrap().tree().lock();
                let cached_max = cache
                    .tree
                    .values()
                    .flat_map(|ops| ops.iter().map(|op| op.id.lamport))
                    .max()
                    .unwrap();
                (cached_max, cache.max_lamport)
            });
            assert!(cached_max <= bound);
            assert!(bound <= lamport_of(v));
        }
    }
}

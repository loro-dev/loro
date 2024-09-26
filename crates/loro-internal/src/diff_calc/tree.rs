use std::{collections::BTreeSet, sync::Arc};

use fractional_index::FractionalIndex;
use fxhash::FxHashMap;
use itertools::Itertools;
use loro_common::{ContainerID, IdFull, IdLp, Lamport, PeerID, TreeID, ID};

use crate::{
    container::{idx::ContainerIdx, tree::tree_op::TreeOp},
    dag::DagUtils,
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
        idx: ContainerIdx,
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

    fn checkout(&mut self, to: &VersionVector, to_frontiers: &Frontiers, oplog: &OpLog) {
        oplog.with_history_cache(|h| {
            let mark = h.ensure_importing_caches_exist();
            let tree_ops = h.get_tree(&self.container, mark).unwrap();
            let mut tree_cache = tree_ops.tree().try_lock().unwrap();
            let s = format!("checkout current {:?} to {:?}", &tree_cache.current_vv, &to);
            let s = tracing::span!(tracing::Level::INFO, "checkout", s = s);
            let _e = s.enter();
            if to == &tree_cache.current_vv {
                tracing::info!("checkout: to == current_vv");
                return;
            }
            let min_lamport = self.get_min_lamport_by_frontiers(&to_frontiers, oplog);
            // retreat
            let mut retreat_ops = vec![];
            for (_target, ops) in tree_cache.tree.iter() {
                for op in ops.iter().rev() {
                    if op.id.lamport < min_lamport {
                        break;
                    }
                    if !to.includes_id(op.id.id()) {
                        retreat_ops.push(op.clone());
                    }
                }
            }
            tracing::info!(msg="retreat ops", retreat_ops=?retreat_ops);
            for op in retreat_ops {
                tree_cache.retreat_op(&op);
            }

            // forward and apply
            let max_lamport = self.get_max_lamport_by_frontiers(&to_frontiers, oplog);
            let mut forward_ops = vec![];
            let group = h
                .get_importing_cache(&self.container, mark)
                .unwrap()
                .as_tree()
                .unwrap();
            for (idlp, op) in group.ops().range(
                IdLp {
                    lamport: 0,
                    peer: 0,
                }..=IdLp {
                    lamport: max_lamport,
                    peer: PeerID::MAX,
                },
            ) {
                if !tree_cache
                    .current_vv
                    .includes_id(ID::new(idlp.peer, op.counter))
                    && to.includes_id(ID::new(idlp.peer, op.counter))
                {
                    forward_ops.push((IdFull::new(idlp.peer, op.counter, idlp.lamport), op));
                }
            }

            // tracing::info!("forward ops {:?}", forward_ops);
            for (id, op) in forward_ops {
                let op = MoveLamportAndID {
                    id,
                    op: op.value.clone(),
                    effected: false,
                };
                tree_cache.apply(op);
            }
            tree_cache.current_vv = to.clone();
        });
    }

    fn checkout_diff(&mut self, info: DiffCalcVersionInfo, oplog: &OpLog) -> TreeDelta {
        oplog.with_history_cache(|h| {
            let mark = h.ensure_importing_caches_exist();
            let tree_ops = h.get_tree(&self.container, mark).unwrap();
            let mut tree_cache = tree_ops.tree().try_lock().unwrap();
            let mut parent_to_children_cache =
                TreeParentToChildrenCache::init_from_tree_cache(&tree_cache);
            let s = tracing::span!(tracing::Level::INFO, "checkout_diff");
            let _e = s.enter();
            let to_frontiers = info.to_frontiers;
            let from_frontiers = info.from_frontiers;
            let (common_ancestors, _mode) =
                oplog.dag.find_common_ancestor(from_frontiers, to_frontiers);
            let lca_vv = oplog.dag.frontiers_to_vv(&common_ancestors).unwrap();
            let lca_frontiers = common_ancestors;
            tracing::info!(
                "from vv {:?} to vv {:?} current vv {:?} lca vv {:?}",
                info.from_vv,
                info.to_vv,
                tree_cache.current_vv,
                lca_vv
            );

            let to_max_lamport = self.get_max_lamport_by_frontiers(to_frontiers, oplog);
            let lca_min_lamport = self.get_min_lamport_by_frontiers(&lca_frontiers, oplog);

            // retreat for diff
            tracing::info!("start retreat");
            let mut diffs = vec![];

            if !(tree_cache.current_vv == lca_vv && &lca_vv == info.from_vv) {
                let mut retreat_ops = vec![];
                for (_target, ops) in tree_cache.tree.iter() {
                    for op in ops.iter().rev() {
                        if op.id.lamport < lca_min_lamport {
                            break;
                        }
                        if !lca_vv.includes_id(op.id.id()) {
                            retreat_ops.push(op.clone());
                        }
                    }
                }

                // tracing::info!("retreat ops {:?}", retreat_ops);
                for op in retreat_ops.into_iter().sorted().rev() {
                    tree_cache.retreat_op(&op);
                    let (old_parent, position, last_effective_move_op_id) =
                        tree_cache.get_parent_with_id(op.op.target());
                    if op.effected {
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
                            let mut s = vec![op.op.target()];
                            while let Some(t) = s.pop() {
                                let children = tree_cache.get_children_with_id(
                                    TreeParentId::Node(t),
                                    &parent_to_children_cache,
                                );
                                children.iter().for_each(|c| {
                                    diffs.push(TreeDeltaItem {
                                        target: c.0,
                                        action: TreeInternalDiff::Create {
                                            parent: TreeParentId::Node(t),
                                            position: c.1.clone().unwrap(),
                                        },
                                        last_effective_move_op_id: c.2,
                                    })
                                });
                                s.extend(children.iter().map(|c| c.0));
                            }
                        }
                    }
                }
            }
            tree_cache.current_vv = lca_vv;
            // forward
            tracing::info!("forward");
            let group = h
                .get_importing_cache(&self.container, mark)
                .unwrap()
                .as_tree()
                .unwrap();
            for (idlp, op) in group.ops().range(
                IdLp {
                    lamport: lca_min_lamport,
                    peer: 0,
                }..=IdLp {
                    lamport: to_max_lamport,
                    peer: PeerID::MAX,
                },
            ) {
                let id = ID::new(idlp.peer, op.counter);
                if !tree_cache.current_vv.includes_id(id) && info.to_vv.includes_id(id) {
                    let op = MoveLamportAndID {
                        id: IdFull {
                            peer: id.peer,
                            lamport: idlp.lamport,
                            counter: id.counter,
                        },
                        op: op.value.clone(),
                        effected: false,
                    };
                    let (old_parent, _position, _id) =
                        tree_cache.get_parent_with_id(op.op.target());
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
                            // TODO: per
                            let mut s = vec![op.op.target()];
                            while let Some(t) = s.pop() {
                                let children = tree_cache.get_children_with_id(
                                    TreeParentId::Node(t),
                                    &parent_to_children_cache,
                                );
                                children.iter().for_each(|c| {
                                    diffs.push(TreeDeltaItem {
                                        target: c.0,
                                        action: TreeInternalDiff::Create {
                                            parent: TreeParentId::Node(t),
                                            position: c.1.clone().unwrap(),
                                        },
                                        last_effective_move_op_id: c.2,
                                    })
                                });
                                s.extend(children.iter().map(|x| x.0));
                            }
                        }
                    }
                }
            }

            tree_cache.current_vv = info.to_vv.clone();
            TreeDelta { diff: diffs }
        })
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
    fn retreat_op(&mut self, op: &MoveLamportAndID) {
        self.tree.get_mut(&op.op.target()).unwrap().remove(op);
        self.current_vv.set_end(op.id.id());
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
            TreeParentId::Unexist => unreachable!(),
        }
    }

    fn apply(&mut self, mut node: MoveLamportAndID) -> bool {
        let mut effected = true;
        if self.is_ancestor_of(&node.op.target(), &node.op.parent_id()) {
            effected = false;
        }
        node.effected = effected;
        self.current_vv.set_last(node.id.id());
        self.tree.entry(node.op.target()).or_default().insert(node);
        effected
    }

    pub(crate) fn init_tree_with_trimmed_version(&mut self, nodes: Vec<MoveLamportAndID>) {
        if nodes.is_empty() {
            return;
        }

        debug_assert!(self.tree.is_empty());
        for node in nodes.into_iter() {
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
            self.cache.get_mut(&old_parent).unwrap().remove(&target);
        }
        self.cache.entry(new_parent).or_default().insert(target);
    }
}

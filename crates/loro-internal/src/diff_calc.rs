use std::sync::Arc;

pub(super) mod tree;
use itertools::Itertools;
pub(super) use tree::TreeDiffCache;

use enum_dispatch::enum_dispatch;
use fxhash::{FxHashMap, FxHashSet};
use loro_common::{ContainerID, ContainerType, HasIdSpan, LoroValue, PeerID, ID};

use crate::{
    change::Lamport,
    container::{
        idx::ContainerIdx,
        richtext::{
            richtext_state::RichtextStateChunk, AnchorType, CrdtRopeDelta, RichtextChunk,
            RichtextChunkValue, RichtextTracker, StyleOp,
        },
        text::tracker::Tracker,
        tree::tree_op::TreeOp,
    },
    dag::DagUtils,
    delta::{Delta, MapDelta, MapValue, TreeDiffItem},
    event::InternalDiff,
    id::Counter,
    op::RichOp,
    span::{HasId, HasLamport},
    version::Frontiers,
    InternalString, VersionVector,
};

use self::tree::MoveLamportAndID;

use super::{event::InternalContainerDiff, oplog::OpLog};

/// Calculate the diff between two versions. given [OpLog][super::oplog::OpLog]
/// and [AppState][super::state::AppState].
///
/// TODO: persist diffCalculator and skip processed version
#[derive(Debug, Default)]
pub struct DiffCalculator {
    /// ContainerIdx -> (depth, calculator)
    ///
    /// if depth == u16::MAX, we need to calculate it again
    calculators: FxHashMap<ContainerIdx, (u16, ContainerDiffCalculator)>,
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
                    let depth = oplog.arena.get_depth(op.container).unwrap_or(u16::MAX);
                    let (_, calculator) =
                        self.calculators.entry(op.container).or_insert_with(|| {
                            match op.container.get_type() {
                                crate::ContainerType::Text => (
                                    depth,
                                    ContainerDiffCalculator::Richtext(
                                        RichtextDiffCalculator::default(),
                                    ),
                                ),
                                crate::ContainerType::Map => (
                                    depth,
                                    ContainerDiffCalculator::Map(MapDiffCalculator::new()),
                                ),
                                crate::ContainerType::List => (
                                    depth,
                                    ContainerDiffCalculator::List(ListDiffCalculator::default()),
                                ),
                                crate::ContainerType::Tree => {
                                    (depth, ContainerDiffCalculator::Tree(TreeDiffCalculator))
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
            for (_, (_, calculator)) in self.calculators.iter_mut() {
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

        // Because we need to get correct `reset` value that indicates container is created during this round of diff calc,
        // we need to iterate from parents to children. i.e. from smaller depth to larger depth.
        let mut new_containers: FxHashSet<ContainerID> = FxHashSet::default();
        let empty_vv: VersionVector = Default::default();
        let mut all: Vec<(u16, ContainerIdx)> = if let Some(set) = affected_set {
            // only visit the affected containers
            set.into_iter()
                .map(|x| {
                    let (depth, _) = self.calculators.get_mut(&x).unwrap();
                    (*depth, x)
                })
                .collect()
        } else {
            self.calculators
                .iter_mut()
                .map(|(x, (depth, _))| (*depth, *x))
                .collect()
        };

        let mut are_rest_containers_deleted = false;
        let mut ans = FxHashMap::default();
        while !all.is_empty() {
            // sort by depth and lamport, ensure we iterate from top to bottom
            all.sort_by_key(|x| x.0);
            debug_log::debug_dbg!(&all);
            let len = all.len();
            for (_, idx) in std::mem::take(&mut all) {
                if ans.contains_key(&idx) {
                    continue;
                }

                let (depth, calc) = self.calculators.get_mut(&idx).unwrap();
                if *depth == u16::MAX && !are_rest_containers_deleted {
                    if let Some(d) = oplog.arena.get_depth(idx) {
                        *depth = d;
                    }

                    all.push((*depth, idx));
                    continue;
                }

                let (from, reset) = if new_containers.remove(&oplog.arena.idx_to_id(idx).unwrap()) {
                    // if the container is new, we need to calculate the diff from the beginning
                    (&empty_vv, true)
                } else {
                    (before, false)
                };

                let diff = calc.calculate_diff(oplog, from, after, |c| {
                    new_containers.insert(c.clone());
                    let child_idx = oplog.arena.register_container(c);
                    oplog.arena.set_parent(child_idx, Some(idx));
                });
                if !diff.is_empty() || reset {
                    ans.insert(
                        idx,
                        InternalContainerDiff {
                            idx,
                            reset,
                            is_container_deleted: are_rest_containers_deleted,
                            diff: diff.into(),
                        },
                    );
                }
            }

            debug_log::debug_dbg!(&new_containers);
            // reset left new_containers
            while !new_containers.is_empty() {
                for id in std::mem::take(&mut new_containers) {
                    let Some(idx) = oplog.arena.id_to_idx(&id) else {
                        continue;
                    };
                    let Some((_, calc)) = self.calculators.get_mut(&idx) else {
                        continue;
                    };
                    let diff = calc.calculate_diff(oplog, &empty_vv, after, |c| {
                        new_containers.insert(c.clone());
                        let child_idx = oplog.arena.register_container(c);
                        oplog.arena.set_parent(child_idx, Some(idx));
                    });
                    // this can override the previous diff with `reset = false`
                    // otherwise, the diff event will be incorrect
                    ans.insert(
                        idx,
                        InternalContainerDiff {
                            idx,
                            reset: true,
                            is_container_deleted: false,
                            diff: diff.into(),
                        },
                    );
                }
            }

            if len == all.len() {
                debug_log::debug_log!("Container might be deleted");
                debug_log::debug_dbg!(&all);
                for (_, idx) in all.iter() {
                    debug_log::debug_dbg!(oplog.arena.get_container_id(*idx));
                }
                // we still emit the event of deleted container
                are_rest_containers_deleted = true;
            }
        }

        debug_log::debug_dbg!(&ans);
        ans.into_iter().map(|x| x.1).collect_vec()
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
        on_new_container: impl FnMut(&ContainerID),
    ) -> InternalDiff;
}

#[enum_dispatch(DiffCalculatorTrait)]
#[derive(Debug)]
enum ContainerDiffCalculator {
    Map(MapDiffCalculator),
    List(ListDiffCalculator),
    Richtext(RichtextDiffCalculator),
    Tree(TreeDiffCalculator),
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
        mut on_new_container: impl FnMut(&ContainerID),
    ) -> InternalDiff {
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
                    if let Some(LoroValue::Container(c)) = &value {
                        on_new_container(c);
                    }

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

        InternalDiff::Map(MapDelta { updated })
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
use rle::HasLength;

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
        oplog: &OpLog,
        from: &crate::VersionVector,
        to: &crate::VersionVector,
        mut on_new_container: impl FnMut(&ContainerID),
    ) -> InternalDiff {
        let ans = self.tracker.diff(from, to);
        // PERF: We may simplify list to avoid these getting
        for v in ans.iter() {
            if let crate::delta::DeltaItem::Insert { value, meta: _ } = &v {
                for range in &value.0 {
                    for i in range.0.clone() {
                        let v = oplog.arena.get_value(i as usize);
                        if let Some(LoroValue::Container(c)) = &v {
                            on_new_container(c);
                        }
                    }
                }
            }
        }

        InternalDiff::SeqRaw(ans)
    }
}

#[derive(Debug, Default)]
struct RichtextDiffCalculator {
    start_vv: VersionVector,
    tracker: Box<RichtextTracker>,
    styles: Vec<StyleOp>,
}

impl DiffCalculatorTrait for RichtextDiffCalculator {
    fn start_tracking(&mut self, _oplog: &super::oplog::OpLog, vv: &crate::VersionVector) {
        if !vv.includes_vv(&self.start_vv) || !self.tracker.all_vv().includes_vv(vv) {
            self.tracker = Box::new(RichtextTracker::new_with_unknown());
            self.styles.clear();
            self.start_vv = vv.clone();
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

        match &op.op().content {
            crate::op::InnerContent::List(l) => match l {
                crate::container::list::list_op::InnerListOp::Insert { slice, pos } => {
                    self.tracker.insert(
                        op.id_start(),
                        *pos,
                        RichtextChunk::new_text(slice.0.clone()),
                    );
                }
                crate::container::list::list_op::InnerListOp::InsertText {
                    slice: _,
                    unicode_start,
                    unicode_len: len,
                    pos,
                } => {
                    self.tracker.insert(
                        op.id_start(),
                        *pos as usize,
                        RichtextChunk::new_text(*unicode_start..*unicode_start + *len),
                    );
                }
                crate::container::list::list_op::InnerListOp::Delete(del) => {
                    self.tracker.delete(
                        op.id_start(),
                        del.start() as usize,
                        del.atom_len(),
                        del.pos < 0,
                    );
                }
                crate::container::list::list_op::InnerListOp::StyleStart {
                    start,
                    end,
                    key,
                    info,
                } => {
                    debug_assert!(start < end, "start: {}, end: {}", start, end);
                    let style_id = self.styles.len();
                    self.styles.push(StyleOp {
                        lamport: op.lamport(),
                        peer: op.peer,
                        cnt: op.id_start().counter,
                        key: key.clone(),
                        info: *info,
                    });
                    self.tracker.insert(
                        op.id_start(),
                        *start as usize,
                        RichtextChunk::new_style_anchor(style_id as u32, AnchorType::Start),
                    );
                    self.tracker.insert(
                        op.id_start().inc(1),
                        // need to shift 1 because we insert the start style anchor before this pos
                        *end as usize + 1,
                        RichtextChunk::new_style_anchor(style_id as u32, AnchorType::End),
                    );
                }
                crate::container::list::list_op::InnerListOp::StyleEnd => {}
            },
            crate::op::InnerContent::Map(_) => unreachable!(),
            crate::op::InnerContent::Tree(_) => unreachable!(),
        }
    }

    fn stop_tracking(&mut self, _oplog: &super::oplog::OpLog, _vv: &crate::VersionVector) {}

    fn calculate_diff(
        &mut self,
        oplog: &OpLog,
        from: &crate::VersionVector,
        to: &crate::VersionVector,
        _: impl FnMut(&ContainerID),
    ) -> InternalDiff {
        let mut delta = Delta::new();
        for item in self.tracker.diff(from, to) {
            match item {
                CrdtRopeDelta::Retain(len) => {
                    delta = delta.retain(len);
                }
                CrdtRopeDelta::Insert(value) => match value.value() {
                    RichtextChunkValue::Text(text) => {
                        delta = delta.insert(RichtextStateChunk::Text {
                            unicode_len: text.len() as i32,
                            // PERF: can be speedup by acquiring lock on arena
                            text: oplog
                                .arena
                                .slice_by_unicode(text.start as usize..text.end as usize),
                        });
                    }
                    RichtextChunkValue::StyleAnchor { id, anchor_type } => {
                        delta = delta.insert(RichtextStateChunk::Style {
                            style: Arc::new(self.styles[id as usize].clone()),
                            anchor_type,
                        });
                    }
                    RichtextChunkValue::Unknown(_) => unreachable!(),
                },
                CrdtRopeDelta::Delete(len) => {
                    delta = delta.delete(len);
                }
            }
        }

        // FIXME: handle new containers when inserting richtext style like comments

        // debug_log::debug_dbg!(&delta, from, to);
        // debug_log::debug_dbg!(&self.tracker);
        InternalDiff::RichtextRaw(delta)
    }
}

#[derive(Debug, Default)]
struct TreeDiffCalculator;

impl TreeDiffCalculator {
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

impl DiffCalculatorTrait for TreeDiffCalculator {
    fn start_tracking(&mut self, _oplog: &OpLog, _vv: &crate::VersionVector) {}

    fn apply_change(
        &mut self,
        oplog: &OpLog,
        op: crate::op::RichOp,
        _vv: Option<&crate::VersionVector>,
    ) {
        let TreeOp { target, parent } = op.op().content.as_tree().unwrap();
        let node = MoveLamportAndID {
            lamport: op.lamport(),
            id: ID {
                peer: op.client_id(),
                counter: op.id_start().counter,
            },
            target: *target,
            parent: *parent,
            effected: true,
        };
        let mut tree_cache = oplog.tree_parent_cache.lock().unwrap();
        tree_cache.add_node(node);
    }

    fn stop_tracking(&mut self, _oplog: &OpLog, _vv: &crate::VersionVector) {}

    fn calculate_diff(
        &mut self,
        oplog: &OpLog,
        from: &crate::VersionVector,
        to: &crate::VersionVector,
        mut on_new_container: impl FnMut(&ContainerID),
    ) -> InternalDiff {
        debug_log::debug_log!("from {:?} to {:?}", from, to);
        let mut merged_vv = from.clone();
        merged_vv.merge(to);
        let from_frontiers = from.to_frontiers(&oplog.dag);
        let to_frontiers = to.to_frontiers(&oplog.dag);
        let common_ancestors = oplog
            .dag
            .find_common_ancestor(&from_frontiers, &to_frontiers);
        let lca_vv = oplog.dag.frontiers_to_vv(&common_ancestors).unwrap();
        let lca_frontiers = lca_vv.to_frontiers(&oplog.dag);
        debug_log::debug_log!("lca vv {:?}", lca_vv);

        let mut tree_cache = oplog.tree_parent_cache.lock().unwrap();
        let to_max_lamport = self.get_max_lamport_by_frontiers(&to_frontiers, oplog);
        let lca_min_lamport = self.get_min_lamport_by_frontiers(&lca_frontiers, oplog);
        let from_min_lamport = self.get_min_lamport_by_frontiers(&from_frontiers, oplog);
        let from_max_lamport = self.get_max_lamport_by_frontiers(&from_frontiers, oplog);
        let diff = tree_cache.diff(
            from,
            to,
            &lca_vv,
            to_max_lamport,
            lca_min_lamport,
            (from_min_lamport, from_max_lamport),
        );

        diff.diff.iter().for_each(|d| {
            if matches!(d.action, TreeDiffItem::Create) {
                on_new_container(&d.target.associated_meta_container())
            }
        });

        debug_log::debug_log!("\ndiff {:?}", diff);

        InternalDiff::Tree(diff)
    }
}

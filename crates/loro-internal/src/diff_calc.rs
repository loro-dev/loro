use std::collections::BinaryHeap;

use enum_dispatch::enum_dispatch;
use fxhash::{FxHashMap, FxHashSet};
use loro_common::{HasIdSpan, PeerID, ID};

use crate::{
    change::Lamport,
    container::idx::ContainerIdx,
    delta::{MapDelta, MapValue},
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
        let affected_set = if !self.has_all
            || !self.last_vv.includes_vv(before)
            || !self.last_vv.includes_vv(after)
        {
            // if we don't have all the ops, we need to calculate the diff by tracing back
            let mut after = after;
            let mut before = before;
            let mut merged = before.clone();
            let mut before_frontiers = before_frontiers;
            let mut after_frontiers = after_frontiers;
            merged.merge(after);
            let empty_vv: VersionVector = Default::default();
            if !after.includes_vv(before) {
                // if after is not after before, we need to calculate the diff from the beginning
                before = &empty_vv;
                after = &merged;
                before_frontiers = None;
                after_frontiers = None;
                self.has_all = true;
            }

            if before.is_empty() {
                self.has_all = true;
            }

            let (lca, iter) =
                oplog.iter_from_lca_causally(before, before_frontiers, after, after_frontiers);
            let mut started_set = FxHashSet::default();
            for (change, vv) in iter {
                self.last_vv.extend_to_include_end_id(change.id_end());
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
    grouped: FxHashMap<InternalString, CompactGroupedValues>,
}

impl MapDiffCalculator {
    pub(crate) fn new() -> Self {
        Self {
            grouped: Default::default(),
        }
    }

    fn checkout(&mut self, vv: &VersionVector) {
        for (_, g) in self.grouped.iter_mut() {
            let a = g.len();
            g.checkout(vv);
            debug_assert_eq!(a, g.len());
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
            .pending
            .push(CompactMapValue {
                lamport: op.lamport(),
                peer: op.client_id(),
                counter: op.id_start().counter,
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
        self.checkout(from);
        for (k, g) in self.grouped.iter_mut() {
            let top = g.applied_or_smaller.peek().copied();
            g.checkout(to);
            match (&top, g.applied_or_smaller.peek()) {
                (None, None) => {}
                (None, Some(_)) => changed.push(k.clone()),
                (Some(_), None) => changed.push(k.clone()),
                (Some(a), Some(b)) => {
                    if a != b {
                        changed.push(k.clone())
                    }
                }
            }
        }

        let mut updated = FxHashMap::with_capacity_and_hasher(changed.len(), Default::default());
        for key in changed {
            let value = self
                .grouped
                .get(&key)
                .unwrap()
                .applied_or_smaller
                .peek()
                .cloned()
                .map(|v| {
                    let value = oplog
                        .lookup_op(v.id_start())
                        .unwrap()
                        .content
                        .as_map()
                        .unwrap()
                        .value;
                    let value = oplog.arena.get_value(value as usize);
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
}

impl HasId for CompactMapValue {
    fn id_start(&self) -> ID {
        ID::new(self.peer, self.counter)
    }
}

#[derive(Debug, Default)]
struct CompactGroupedValues {
    /// Each value in this set should be included in the current version or
    /// "concurrent to the current version it is not at the peak".
    applied_or_smaller: BinaryHeap<CompactMapValue>,
    /// The values that are guaranteed not in the current version. (they are from the future)
    pending: Vec<CompactMapValue>,
}

impl CompactGroupedValues {
    fn checkout(&mut self, vv: &VersionVector) {
        self.pending.retain(|v| {
            if vv.includes_id(v.id_start()) {
                self.applied_or_smaller.push(*v);
                false
            } else {
                true
            }
        });

        while let Some(top) = self.applied_or_smaller.peek() {
            if vv.includes_id(top.id_start()) {
                break;
            } else {
                let top = self.applied_or_smaller.pop().unwrap();
                self.pending.push(top);
            }
        }
    }

    fn len(&self) -> usize {
        self.applied_or_smaller.len() + self.pending.len()
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

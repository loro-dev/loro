use std::{cmp::Ordering, collections::BinaryHeap};

use enum_dispatch::enum_dispatch;
use fxhash::{FxHashMap, FxHashSet};
use loro_common::{ContainerType, HasIdSpan, PeerID, ID};

use crate::{
    change::{Change, Lamport},
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
#[derive(Default)]
pub struct DiffCalculator {
    calculators: FxHashMap<ContainerIdx, ContainerDiffCalculator>,
}

impl DiffCalculator {
    pub fn new() -> Self {
        Self {
            calculators: Default::default(),
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
        let mut diffs = Vec::new();
        let (lca, iter) =
            oplog.iter_from_lca_causally(before, before_frontiers, after, after_frontiers);
        for (change, vv) in iter {
            let mut visited = FxHashSet::default();
            for op in change.ops.iter() {
                let calculator = self.calculators.entry(op.container).or_insert_with(|| {
                    let mut new = match op.container.get_type() {
                        crate::ContainerType::Text => {
                            ContainerDiffCalculator::Text(TextDiffCalculator::default())
                        }
                        crate::ContainerType::Map => {
                            ContainerDiffCalculator::Map(MapDiffCalculator::new(op.container))
                        }
                        crate::ContainerType::List => {
                            ContainerDiffCalculator::List(ListDiffCalculator::default())
                        }
                    };
                    new.start_tracking(oplog, &lca);
                    new
                });

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

        for (&idx, calculator) in self.calculators.iter_mut() {
            calculator.stop_tracking(oplog, after);
            diffs.push(InternalContainerDiff {
                idx,
                diff: calculator.calculate_diff(oplog, before, after),
            });
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
enum ContainerDiffCalculator {
    Text(TextDiffCalculator),
    Map(MapDiffCalculator),
    List(ListDiffCalculator),
}

#[derive(Default)]
struct TextDiffCalculator {
    tracker: Tracker,
}

struct MapDiffCalculator {
    idx: ContainerIdx,
    grouped: FxHashMap<InternalString, GroupedValues>,
}

#[derive(Default)]
struct GroupedValues {
    /// Each value in this set should be included in the current version or
    /// "concurrent to the current version it is not at the peak".
    applied_or_smaller: BinaryHeap<MapValue>,
    /// The values that are guaranteed not in the current version. (they are from the future)
    pending: FxHashSet<MapValue>,
}

impl GroupedValues {
    fn checkout(&mut self, vv: &VersionVector) {
        self.pending.retain(|v| {
            if vv.includes_id(v.id_start()) {
                self.applied_or_smaller.push(v.clone());
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
                self.pending.insert(top);
            }
        }
    }
}

impl MapDiffCalculator {
    pub(crate) fn new(idx: ContainerIdx) -> Self {
        Self {
            idx,
            grouped: Default::default(),
        }
    }

    fn checkout(&mut self, vv: &VersionVector) {
        for (_, g) in self.grouped.iter_mut() {
            g.checkout(vv)
        }
    }
}

impl DiffCalculatorTrait for MapDiffCalculator {
    fn start_tracking(&mut self, _oplog: &crate::OpLog, _vv: &crate::VersionVector) {}

    fn apply_change(
        &mut self,
        oplog: &crate::OpLog,
        op: crate::op::RichOp,
        _vv: Option<&crate::VersionVector>,
    ) {
        let map = op.op().content.as_map().unwrap();
        let value = oplog.arena.get_value(map.value as usize);
        self.grouped
            .entry(map.key.clone())
            .or_default()
            .pending
            .insert(MapValue::new(op.id_start(), op.lamport(), value));
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
            let top = g.applied_or_smaller.pop();
            g.checkout(to);
            if let Some(top) = &top {
                if to.includes_id(top.id_start()) {
                    g.applied_or_smaller.push(top.clone());
                }
            }

            match (&top, g.applied_or_smaller.peek()) {
                (None, None) => todo!(),
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
        let mut extra_lookup = Vec::new();
        for key in changed {
            if let Some(value) = self
                .grouped
                .get(&key)
                .unwrap()
                .applied_or_smaller
                .peek()
                .cloned()
            {
                updated.insert(key, value);
            } else {
                extra_lookup.push(key);
            }
        }

        if !extra_lookup.is_empty() {
            // PERF: the first time we do this, it may take a long time:
            // it will travel the whole history with O(n) time
            let ans = oplog.lookup_map_values_at(self.idx, &extra_lookup, to);
            for (k, v) in extra_lookup.into_iter().zip(ans.into_iter()) {
                updated.insert(
                    k,
                    v.unwrap_or_else(|| MapValue {
                        counter: 0,
                        value: None,
                        lamport: (0, 0),
                    }),
                );
            }
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

    fn peek(&self) -> Option<CompactMapValue> {
        self.applied_or_smaller.peek().cloned()
    }
}

#[derive(Default)]
pub(crate) struct GlobalMapDiffCalculator {
    maps: FxHashMap<ContainerIdx, FxHashMap<InternalString, CompactGroupedValues>>,
    pub(crate) last_vv: VersionVector,
}

impl GlobalMapDiffCalculator {
    pub fn process_change(&mut self, change: &Change) {
        if self.last_vv.includes_id(change.id_last()) {
            return;
        }

        for op in change.ops.iter() {
            if op.container.get_type() == ContainerType::Map {
                let key = op.content.as_map().unwrap().key.clone();
                self.maps
                    .entry(op.container)
                    .or_default()
                    .entry(key)
                    .or_default()
                    .pending
                    .push(CompactMapValue {
                        lamport: (op.counter - change.id.counter) as Lamport + change.lamport,
                        peer: change.id.peer,
                        counter: op.counter,
                    });
            }
        }

        self.last_vv.extend_to_include_end_id(change.id_end());
    }

    pub fn get_value_at(
        &mut self,
        container: ContainerIdx,
        key: &InternalString,
        vv: &VersionVector,
        oplog: &OpLog,
    ) -> Option<MapValue> {
        let group = self.maps.get_mut(&container)?.get_mut(key)?;
        group.checkout(vv);
        let peek = group.peek()?;
        let op = oplog.lookup_op(peek.id_start()).unwrap();
        let value_idx = op.content.as_map().unwrap().value;
        let value = oplog.arena.get_value(value_idx as usize);
        Some(MapValue {
            counter: peek.counter,
            value,
            lamport: (peek.lamport, peek.peer),
        })
    }
}

#[derive(Default)]
struct ListDiffCalculator {
    tracker: Tracker,
}

impl DiffCalculatorTrait for ListDiffCalculator {
    fn start_tracking(&mut self, _oplog: &OpLog, vv: &crate::VersionVector) {
        if matches!(
            self.tracker.start_vv().partial_cmp(vv),
            None | Some(Ordering::Less)
        ) {
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
        if matches!(
            self.tracker.start_vv().partial_cmp(vv),
            None | Some(Ordering::Less)
        ) {
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

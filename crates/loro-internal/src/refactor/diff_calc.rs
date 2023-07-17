use std::{cmp::Ordering, collections::BinaryHeap};

use debug_log::debug_dbg;
use enum_dispatch::enum_dispatch;
use fxhash::{FxHashMap, FxHashSet};

use crate::{
    container::registry::ContainerIdx,
    delta::{MapDelta, MapValue},
    event::Diff,
    id::Counter,
    op::RichOp,
    span::{HasId, HasLamport},
    text::tracker::Tracker,
    version::Frontiers,
    InternalString, VersionVector,
};

use super::{oplog::OpLog, state::ContainerStateDiff};

/// Calculate the diff between two versions. given [OpLog][super::oplog::OpLog]
/// and [AppState][super::state::AppState].
///
/// TODO: persist diffCalculator and skip processed version
#[derive(Default)]
pub struct DiffCalculator {
    start_vv: VersionVector,
    end_vv: VersionVector,
    calculators: FxHashMap<ContainerIdx, ContainerDiffCalculator>,
}

impl DiffCalculator {
    pub fn new() -> Self {
        Self {
            start_vv: Default::default(),
            end_vv: Default::default(),
            calculators: Default::default(),
        }
    }

    pub fn calc_diff(
        &mut self,
        oplog: &super::oplog::OpLog,
        before: &crate::VersionVector,
        after: &crate::VersionVector,
    ) -> Vec<ContainerStateDiff> {
        self.calc_diff_internal(oplog, before, None, after, None)
    }

    pub(crate) fn calc_diff_internal(
        &mut self,
        oplog: &super::oplog::OpLog,
        before: &crate::VersionVector,
        before_frontiers: Option<&Frontiers>,
        after: &crate::VersionVector,
        after_frontiers: Option<&Frontiers>,
    ) -> Vec<ContainerStateDiff> {
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
                            ContainerDiffCalculator::Map(MapDiffCalculator::default())
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
            diffs.push(ContainerStateDiff {
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

#[derive(Default)]
struct MapDiffCalculator {
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
    fn checkout(&mut self, vv: &VersionVector) {
        for (_, g) in self.grouped.iter_mut() {
            g.checkout(vv)
        }
    }
}

impl DiffCalculatorTrait for MapDiffCalculator {
    fn start_tracking(&mut self, _oplog: &super::oplog::OpLog, _vv: &crate::VersionVector) {}

    fn apply_change(
        &mut self,
        oplog: &super::oplog::OpLog,
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
        _oplog: &super::oplog::OpLog,
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
        for key in changed {
            let value = self
                .grouped
                .get(&key)
                .unwrap()
                .applied_or_smaller
                .peek()
                .cloned()
                .unwrap();
            updated.insert(key, value);
        }

        Diff::NewMap(MapDelta { updated })
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
            self.tracker = Tracker::new(vv.clone(), 0);
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

    fn stop_tracking(&mut self, _oplog: &OpLog, _vv: &crate::VersionVector) {
        todo!()
    }

    fn calculate_diff(
        &mut self,
        _oplog: &OpLog,
        from: &crate::VersionVector,
        to: &crate::VersionVector,
    ) -> Diff {
        Diff::SeqRaw(self.tracker.diff(from, to))
    }
}

impl TextDiffCalculator {
    fn new(tracker: Tracker) -> Self {
        Self { tracker }
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

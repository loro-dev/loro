use std::{cmp::Ordering, collections::BinaryHeap};

use enum_dispatch::enum_dispatch;
use fxhash::{FxHashMap, FxHashSet};

use crate::{
    container::ContainerID,
    delta::{MapDelta, MapValue},
    event::Diff,
    span::{HasId, HasLamport},
    text::tracker::Tracker,
    InternalString, VersionVector,
};

use super::{oplog::OpLog, state::ContainerStateDiff};

/// Calculate the diff between two versions. given [OpLog][super::oplog::OpLog]
/// and [AppState][super::state::AppState].
#[derive(Default)]
pub struct DiffCalculator {
    start_vv: VersionVector,
    end_vv: VersionVector,
    calc: FxHashMap<ContainerID, ContainerDiffCalculator>,
}

impl DiffCalculator {
    pub(crate) fn calc(
        &self,
        _oplog: &super::oplog::OpLog,
        _before: &crate::VersionVector,
        _after: &crate::VersionVector,
    ) -> Vec<ContainerStateDiff> {
        todo!()
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
    fn apply_change(&mut self, oplog: &OpLog, op: crate::op::RichOp, vv: &crate::VersionVector);
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
        _vv: &crate::VersionVector,
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

    fn apply_change(&mut self, _oplog: &OpLog, op: crate::op::RichOp, vv: &crate::VersionVector) {
        self.tracker.checkout(vv);
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
            self.tracker = Tracker::new(vv.clone(), 0);
        }

        self.tracker.checkout(vv);
    }

    fn apply_change(
        &mut self,
        _oplog: &super::oplog::OpLog,
        op: crate::op::RichOp,
        vv: &crate::VersionVector,
    ) {
        self.tracker.checkout(vv);
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

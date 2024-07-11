use std::collections::BTreeMap;

use loro_common::{ContainerID, ID};

use crate::{container::idx::ContainerIdx, event::InternalDiff, OpLog};

use super::DiffCalculatorTrait;

#[derive(Debug)]
pub(crate) struct CounterDiffCalculator {
    idx: ContainerIdx,
    ops: BTreeMap<ID, f64>,
}

impl CounterDiffCalculator {
    pub(crate) fn new(idx: ContainerIdx) -> Self {
        Self {
            idx,
            ops: BTreeMap::new(),
        }
    }
}

impl DiffCalculatorTrait for CounterDiffCalculator {
    fn start_tracking(&mut self, _oplog: &OpLog, _vv: &crate::VersionVector) {}

    fn apply_change(
        &mut self,
        _oplog: &OpLog,
        op: crate::op::RichOp,
        _vv: Option<&crate::VersionVector>,
    ) {
        let id = op.id();
        self.ops.insert(
            id,
            *op.op().content.as_future().unwrap().as_counter().unwrap(),
        );
    }

    fn stop_tracking(&mut self, _oplog: &OpLog, _vv: &crate::VersionVector) {}

    fn calculate_diff(
        &mut self,
        _oplog: &OpLog,
        from: &crate::VersionVector,
        to: &crate::VersionVector,
        _on_new_container: impl FnMut(&ContainerID),
    ) -> InternalDiff {
        let mut diff = 0.;
        let (b, a) = from.diff_iter(to);

        for sub in b {
            for (_, c) in self.ops.range(sub.norm_id_start()..sub.norm_id_end()) {
                diff -= c;
            }
        }
        for sub in a {
            for (_, c) in self.ops.range(sub.norm_id_start()..sub.norm_id_end()) {
                diff += c;
            }
        }
        InternalDiff::Counter(diff)
    }
}

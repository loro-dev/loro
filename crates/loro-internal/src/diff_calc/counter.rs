use loro_common::ContainerID;

use crate::{container::idx::ContainerIdx, event::InternalDiff, OpLog};

use super::DiffCalculatorTrait;

#[derive(Debug)]
pub(crate) struct CounterDiffCalculator {
    idx: ContainerIdx,
}

impl CounterDiffCalculator {
    pub(crate) fn new(idx: ContainerIdx) -> Self {
        Self { idx }
    }
}

impl DiffCalculatorTrait for CounterDiffCalculator {
    fn start_tracking(&mut self, _oplog: &OpLog, _vv: &crate::VersionVector) {}

    fn apply_change(
        &mut self,
        _oplog: &OpLog,
        _op: crate::op::RichOp,
        _vv: Option<&crate::VersionVector>,
    ) {
    }

    fn stop_tracking(&mut self, _oplog: &OpLog, _vv: &crate::VersionVector) {}

    fn calculate_diff(
        &mut self,
        oplog: &OpLog,
        from: &crate::VersionVector,
        to: &crate::VersionVector,
        _on_new_container: impl FnMut(&ContainerID),
    ) -> InternalDiff {
        let mut diff = 0;
        // TODO: op group
        let (b, a) = from.diff_iter(to);
        let counter_group = oplog.op_groups.get_counter(&self.idx).unwrap();

        for sub in b {
            for (_, c) in counter_group
                .ops
                .range(sub.norm_id_start()..sub.norm_id_end())
            {
                diff -= c;
            }
        }
        for sub in a {
            for (_, c) in counter_group
                .ops
                .range(sub.norm_id_start()..sub.norm_id_end())
            {
                diff += c;
            }
        }
        InternalDiff::Counter(diff)
    }
}

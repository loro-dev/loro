use loro_common::ContainerID;

use crate::{
    event::InternalDiff,
    op::{FutureInnerContent, InnerContent},
    OpLog, VersionVector,
};

use super::DiffCalculatorTrait;

#[derive(Debug)]
pub(crate) struct CounterDiffCalculator {
    before_vv: VersionVector,
    after_vv: VersionVector,
    diff: i64,
}

impl CounterDiffCalculator {
    pub(crate) fn new(before_vv: VersionVector, after_vv: VersionVector) -> Self {
        Self {
            before_vv,
            after_vv,
            diff: 0,
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
        if let InnerContent::Future(FutureInnerContent::Counter(c)) = op.raw_op().content {
            let before_has = self.before_vv.includes_id(op.id());
            let after_has = self.after_vv.includes_id(op.id());
            if before_has && !after_has {
                self.diff -= c;
            } else if !before_has && after_has {
                self.diff += c;
            }
        }
    }

    fn stop_tracking(&mut self, _oplog: &OpLog, _vv: &crate::VersionVector) {}

    fn calculate_diff(
        &mut self,
        _oplog: &OpLog,
        _from: &crate::VersionVector,
        _to: &crate::VersionVector,
        _on_new_container: impl FnMut(&ContainerID),
    ) -> InternalDiff {
        InternalDiff::Counter(self.diff)
    }
}

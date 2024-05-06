use loro_common::{ContainerID, HasLamport};

use crate::{event::InternalDiff, op::OpWithId, OpLog};

use super::DiffCalculatorTrait;

#[derive(Debug, Default)]
pub struct UnknownDiffCalculator {
    ops: Vec<OpWithId>,
}

impl DiffCalculatorTrait for UnknownDiffCalculator {
    fn start_tracking(&mut self, _oplog: &OpLog, _vv: &crate::VersionVector) {}

    fn apply_change(
        &mut self,
        _oplog: &OpLog,
        op: crate::op::RichOp,
        _vv: Option<&crate::VersionVector>,
    ) {
        self.ops.push(OpWithId {
            peer: op.peer,
            op: op.raw_op().clone(),
            lamport: Some(op.lamport()),
        })
    }

    fn stop_tracking(&mut self, _oplog: &OpLog, _vv: &crate::VersionVector) {}

    fn calculate_diff(
        &mut self,
        _oplog: &OpLog,
        _from: &crate::VersionVector,
        _to: &crate::VersionVector,
        _on_new_container: impl FnMut(&ContainerID),
    ) -> InternalDiff {
        InternalDiff::Unknown
    }
}

use loro_common::ContainerID;

use crate::{event::InternalDiff, OpLog};

use super::{DiffCalculatorTrait, DiffMode};

#[derive(Debug, Default)]
pub struct UnknownDiffCalculator;

impl DiffCalculatorTrait for UnknownDiffCalculator {
    fn start_tracking(&mut self, _oplog: &OpLog, _vv: &crate::VersionVector, mode: DiffMode) {}

    fn apply_change(
        &mut self,
        _oplog: &OpLog,
        _op: crate::op::RichOp,
        _vv: Option<&crate::VersionVector>,
    ) {
    }

    fn finish_this_round(&mut self) {}

    fn calculate_diff(
        &mut self,
        _oplog: &OpLog,
        _from: &crate::VersionVector,
        _to: &crate::VersionVector,
        _on_new_container: impl FnMut(&ContainerID),
    ) -> (InternalDiff, DiffMode) {
        (InternalDiff::Unknown, DiffMode::Checkout)
    }
}

use loro_common::ContainerID;

use crate::internal::{container::idx::ContainerIdx, event::InternalDiff, OpLog};

use super::{DiffCalcVersionInfo, DiffCalculatorTrait, DiffMode};

#[derive(Debug, Default)]
pub struct UnknownDiffCalculator;

impl DiffCalculatorTrait for UnknownDiffCalculator {
    fn start_tracking(&mut self, _oplog: &OpLog, _vv: &crate::internal::VersionVector, _mode: DiffMode) {}

    fn apply_change(
        &mut self,
        _oplog: &OpLog,
        _op: crate::internal::op::RichOp,
        _vv: Option<&crate::internal::VersionVector>,
    ) {
    }

    fn finish_this_round(&mut self) {}

    fn calculate_diff(
        &mut self,
        _idx: ContainerIdx,
        _oplog: &OpLog,
        _info: DiffCalcVersionInfo,
        _on_new_container: impl FnMut(&ContainerID),
    ) -> (InternalDiff, DiffMode) {
        (InternalDiff::Unknown, DiffMode::Checkout)
    }
}

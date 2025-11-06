use std::sync::Weak;

use loro_common::{ContainerID, LoroResult, LoroValue};

use crate::{
    configure::Configure,
    container::idx::ContainerIdx,
    event::{Diff, Index, InternalDiff},
    op::{Op, RawOp},
    LoroDocInner,
};

use super::{ApplyLocalOpReturn, ContainerState, DiffApplyContext};

#[derive(Debug, Clone)]
pub struct UnknownState {
    idx: ContainerIdx,
}

impl UnknownState {
    pub fn new(idx: ContainerIdx) -> Self {
        Self { idx }
    }
}

impl ContainerState for UnknownState {
    fn container_idx(&self) -> ContainerIdx {
        self.idx
    }

    fn is_state_empty(&self) -> bool {
        false
    }

    fn apply_diff_and_convert(&mut self, _diff: InternalDiff, _ctx: DiffApplyContext) -> Diff {
        unreachable!()
    }

    fn apply_diff(&mut self, _diff: InternalDiff, _ctx: DiffApplyContext) {
        unreachable!()
    }

    fn apply_local_op(&mut self, _raw_op: &RawOp, _op: &Op) -> LoroResult<ApplyLocalOpReturn> {
        unreachable!()
    }

    #[doc = r" Convert a state to a diff, such that an empty state will be transformed into the same as this state when it's applied."]
    fn to_diff(&mut self, _doc: &Weak<LoroDocInner>) -> Diff {
        Diff::Unknown
    }

    fn get_value(&mut self) -> LoroValue {
        unreachable!()
    }

    #[doc = r" Get the index of the child container"]
    #[allow(unused)]
    fn get_child_index(&self, id: &ContainerID) -> Option<Index> {
        None
    }

    fn contains_child(&self, _id: &ContainerID) -> bool {
        false
    }

    #[allow(unused)]
    fn get_child_containers(&self) -> Vec<ContainerID> {
        vec![]
    }

    fn fork(&self, _config: &Configure) -> Self {
        self.clone()
    }
}

mod snapshot {
    use loro_common::LoroValue;

    use crate::state::FastStateSnapshot;

    use super::UnknownState;

    impl FastStateSnapshot for UnknownState {
        fn encode_snapshot_fast<W: std::io::prelude::Write>(&mut self, _w: W) {}

        fn decode_value(bytes: &[u8]) -> loro_common::LoroResult<(loro_common::LoroValue, &[u8])> {
            Ok((LoroValue::Null, bytes))
        }

        fn decode_snapshot_fast(
            idx: crate::container::idx::ContainerIdx,
            _v: (loro_common::LoroValue, &[u8]),
            _ctx: crate::state::ContainerCreationContext,
        ) -> loro_common::LoroResult<Self>
        where
            Self: Sized,
        {
            Ok(UnknownState::new(idx))
        }
    }
}

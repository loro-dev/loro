use std::sync::{Mutex, Weak};

use loro_common::{ContainerID, LoroResult, LoroValue};

use crate::{
    arena::SharedArena,
    container::idx::ContainerIdx,
    encoding::{StateSnapshotDecodeContext, StateSnapshotEncoder},
    event::{Diff, Index, InternalDiff},
    op::{Op, RawOp},
    txn::Transaction,
    DocState,
};

use super::ContainerState;

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

    fn estimate_size(&self) -> usize {
        0
    }

    fn is_state_empty(&self) -> bool {
        false
    }

    fn apply_diff_and_convert(
        &mut self,
        _diff: InternalDiff,
        _arena: &SharedArena,
        _txn: &Weak<Mutex<Option<Transaction>>>,
        _state: &Weak<Mutex<DocState>>,
    ) -> Diff {
        unreachable!()
    }

    fn apply_diff(
        &mut self,
        _diff: InternalDiff,
        _arena: &SharedArena,
        _txn: &Weak<Mutex<Option<Transaction>>>,
        _state: &Weak<Mutex<DocState>>,
    ) {
        unreachable!()
    }

    fn apply_local_op(&mut self, _raw_op: &RawOp, _op: &Op) -> LoroResult<()> {
        unreachable!()
    }

    #[doc = r" Convert a state to a diff, such that an empty state will be transformed into the same as this state when it's applied."]
    fn to_diff(
        &mut self,
        _arena: &SharedArena,
        _txn: &Weak<Mutex<Option<Transaction>>>,
        _state: &Weak<Mutex<DocState>>,
    ) -> Diff {
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

    #[doc = r" Encode the ops and the blob that can be used to restore the state to the current state."]
    #[doc = r""]
    #[doc = r" State will use the provided encoder to encode the ops and export a blob."]
    #[doc = r" The ops should be encoded into the snapshot as well as the blob."]
    #[doc = r" The users then can use the ops and the blob to restore the state to the current state."]
    fn encode_snapshot(&self, _encoder: StateSnapshotEncoder) -> Vec<u8> {
        vec![]
    }

    #[doc = r" Restore the state to the state represented by the ops and the blob that exported by `get_snapshot_ops`"]
    fn import_from_snapshot_ops(&mut self, _ctx: StateSnapshotDecodeContext) {}
}

use std::sync::{Mutex, Weak};

use loro_common::{ContainerID, LoroResult, LoroValue};

use crate::{
    arena::SharedArena,
    container::idx::ContainerIdx,
    encoding::{StateSnapshotDecodeContext, StateSnapshotEncoder},
    event::{Diff, Index, InternalDiff},
    op::{FutureRawOpContent, Op, RawOp, RawOpContent},
    txn::Transaction,
    DocState,
};

use super::ContainerState;

#[derive(Debug, Clone)]
pub struct CounterState {
    idx: ContainerIdx,
    value: i64,
}

impl CounterState {
    pub(crate) fn new(idx: ContainerIdx) -> Self {
        Self { idx, value: 0 }
    }
}

impl ContainerState for CounterState {
    fn container_idx(&self) -> ContainerIdx {
        self.idx
    }

    fn estimate_size(&self) -> usize {
        std::mem::size_of::<i64>()
    }

    fn is_state_empty(&self) -> bool {
        false
    }

    #[must_use]
    fn apply_diff_and_convert(
        &mut self,
        diff: InternalDiff,
        _arena: &SharedArena,
        _txn: &Weak<Mutex<Option<Transaction>>>,
        _state: &Weak<Mutex<DocState>>,
    ) -> Diff {
        if let InternalDiff::Counter(diff) = diff {
            self.value += diff;
            Diff::Counter(diff)
        } else {
            unreachable!()
        }
    }

    fn apply_diff(
        &mut self,
        diff: InternalDiff,
        arena: &SharedArena,
        txn: &Weak<Mutex<Option<Transaction>>>,
        state: &Weak<Mutex<DocState>>,
    ) {
        let _ = self.apply_diff_and_convert(diff, arena, txn, state);
    }

    fn apply_local_op(&mut self, raw_op: &RawOp, _op: &Op) -> LoroResult<()> {
        if let RawOpContent::Future(FutureRawOpContent::Counter(diff)) = raw_op.content {
            self.value += diff;
            Ok(())
        } else {
            unreachable!()
        }
    }

    #[doc = " Convert a state to a diff, such that an empty state will be transformed into the same as this state when it\'s applied."]
    fn to_diff(
        &mut self,
        _arena: &SharedArena,
        _txn: &Weak<Mutex<Option<Transaction>>>,
        _state: &Weak<Mutex<DocState>>,
    ) -> Diff {
        Diff::Counter(self.value)
    }

    fn get_value(&mut self) -> LoroValue {
        LoroValue::I64(self.value)
    }

    #[doc = " Get the index of the child container"]
    #[allow(unused)]
    fn get_child_index(&self, id: &ContainerID) -> Option<Index> {
        None
    }

    #[allow(unused)]
    fn get_child_containers(&self) -> Vec<ContainerID> {
        vec![]
    }

    #[doc = " Encode the ops and the blob that can be used to restore the state to the current state."]
    #[doc = ""]
    #[doc = " State will use the provided encoder to encode the ops and export a blob."]
    #[doc = " The ops should be encoded into the snapshot as well as the blob."]
    #[doc = " The users then can use the ops and the blob to restore the state to the current state."]
    fn encode_snapshot(&self, _encoder: StateSnapshotEncoder) -> Vec<u8> {
        let mut ans = vec![];
        leb128::write::signed(&mut ans, self.value).unwrap();
        ans
    }

    #[doc = " Restore the state to the state represented by the ops and the blob that exported by `get_snapshot_ops`"]
    fn import_from_snapshot_ops(&mut self, ctx: StateSnapshotDecodeContext) {
        let mut reader = ctx.blob;
        self.value = leb128::read::signed(&mut reader).unwrap();
    }
}

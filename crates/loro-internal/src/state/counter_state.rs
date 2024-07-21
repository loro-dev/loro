use std::sync::{Mutex, Weak};

use loro_common::{ContainerID, LoroError, LoroResult, LoroValue};

use crate::{
    arena::SharedArena,
    container::idx::ContainerIdx,
    encoding::{StateSnapshotDecodeContext, StateSnapshotEncoder},
    event::{Diff, Index, InternalDiff},
    op::{Op, RawOp, RawOpContent},
    txn::Transaction,
    DocState,
};

use super::ContainerState;

#[derive(Debug, Clone)]
pub struct CounterState {
    idx: ContainerIdx,
    value: f64,
}

impl CounterState {
    pub(crate) fn new(idx: ContainerIdx) -> Self {
        Self { idx, value: 0. }
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
        if let RawOpContent::Counter(diff) = raw_op.content {
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
        LoroValue::Double(self.value)
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
        self.value.to_be_bytes().to_vec()
    }

    #[doc = " Restore the state to the state represented by the ops and the blob that exported by `get_snapshot_ops`"]
    fn import_from_snapshot_ops(&mut self, ctx: StateSnapshotDecodeContext) -> LoroResult<()> {
        let reader = ctx.blob;
        let Some(bytes) = reader.get(0..8) else {
            return Err(LoroError::DecodeDataCorruptionError);
        };
        let mut buf = [0; 8];
        buf.copy_from_slice(bytes);
        self.value = f64::from_be_bytes(buf);
        Ok(())
    }

    #[allow(unused)]
    fn contains_child(&self, id: &ContainerID) -> bool {
        false
    }
}

mod snapshot {
    use crate::state::FastStateSnapshot;

    use super::*;

    impl FastStateSnapshot for CounterState {
        fn encode_snapshot_fast<W: std::io::Write>(&mut self, mut w: W) {
            let bytes = self.value.to_le_bytes();
            w.write_all(&bytes).unwrap();
        }

        fn decode_value(bytes: &[u8]) -> LoroResult<(LoroValue, &[u8])> {
            Ok((
                LoroValue::Double(f64::from_le_bytes(bytes.try_into().unwrap())),
                &[],
            ))
        }

        fn decode_snapshot_fast(
            idx: ContainerIdx,
            v: (LoroValue, &[u8]),
            ctx: crate::state::ContainerCreationContext,
        ) -> LoroResult<Self>
        where
            Self: Sized,
        {
            let mut counter = CounterState::new(idx);
            counter.value = *v.0.as_double().unwrap();
            Ok(counter)
        }
    }
}

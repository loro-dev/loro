use crate::{LoroDoc, OpLog};
use bytes::Bytes;
use loro_common::{LoroError, LoroResult};

pub(crate) fn decode_snapshot(doc: &LoroDoc, bytes: Bytes) -> LoroResult<()> {
    let mut state = doc.app_state().try_lock().map_err(|_| {
        LoroError::DecodeError(
            "decode_snapshot: failed to lock app state"
                .to_string()
                .into_boxed_str(),
        )
    })?;

    state.check_before_decode_snapshot()?;
    let mut oplog = doc.oplog().try_lock().map_err(|_| {
        LoroError::DecodeError(
            "decode_snapshot: failed to lock oplog"
                .to_string()
                .into_boxed_str(),
        )
    })?;

    if !oplog.is_empty() {
        unimplemented!("You can only import snapshot to a empty loro doc now");
    }

    assert!(state.frontiers.is_empty());
    assert!(oplog.frontiers().is_empty());
    let oplog_len = u32::from_le_bytes(bytes[0..4].try_into().unwrap());
    let oplog_bytes = bytes.slice(4..4 + oplog_len as usize);
    let state_len = u32::from_le_bytes(
        bytes[(4 + oplog_len as usize)..(8 + oplog_len as usize)]
            .try_into()
            .unwrap(),
    );
    let state_bytes =
        bytes.slice(8 + oplog_len as usize..8 + oplog_len as usize + state_len as usize);
    state.store.decode(state_bytes)?;
    oplog.decode_change_store(oplog_bytes)?;
    state.frontiers = oplog.frontiers().clone();
    Ok(())
}

impl OpLog {
    fn decode_change_store(&mut self, bytes: bytes::Bytes) -> LoroResult<()> {
        let v = self.change_store().decode_all(bytes)?;
        self.next_lamport = v.next_lamport;
        self.latest_timestamp = v.max_timestamp;
        self.dag.set_version_by_fast_snapshot_import(v);
        Ok(())
    }
}

pub(crate) fn encode_snapshot(doc: &LoroDoc) -> Vec<Bytes> {
    let mut state = doc.app_state().try_lock().unwrap();
    let oplog = doc.oplog().try_lock().unwrap();
    assert!(!state.is_in_txn());
    assert_eq!(oplog.frontiers(), &state.frontiers);

    let oplog_bytes = oplog.encode_change_store();
    let state_bytes = state.store.encode();
    let oplog_len = oplog_bytes.len() as u32;
    let state_len = state_bytes.len() as u32;
    vec![
        oplog_len.to_le_bytes().to_vec().into(),
        oplog_bytes,
        state_len.to_le_bytes().to_vec().into(),
        state_bytes,
    ]
}

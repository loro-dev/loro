use loro_common::{LoroError, LoroResult};

use crate::LoroDoc;

pub(super) fn decode_snapshot(doc: &LoroDoc, bytes: &[u8]) -> LoroResult<()> {
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

    todo!("decode bytes into state kv and oplog's change store kv");

    Ok(())
}

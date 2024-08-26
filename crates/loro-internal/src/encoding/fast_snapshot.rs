//! Fast snapshot encoding and decoding.
//!
//! # Layout
//!
//! - u32 in little endian for len of bytes for oplog
//! - oplog bytes
//! - u32 in little endian for len of bytes for state
//! - state bytes
//! - u32 in little endian for len of bytes for gc
//! - gc bytes
//!
//! All of `oplog bytes`, `state bytes` and `gc bytes` are encoded KV store bytes.
//!
//!
//!
use std::io::{Read, Write};

use crate::{oplog::ChangeStore, LoroDoc, OpLog, VersionVector};
use bytes::{Buf, Bytes};
use loro_common::{LoroError, LoroResult};

use super::encode_reordered::import_changes_to_oplog;

pub(super) struct Snapshot {
    pub oplog_bytes: Bytes,
    pub state_bytes: Bytes,
    pub gc_bytes: Bytes,
}

pub(super) fn _encode_snapshot<W: Write>(s: Snapshot, w: &mut W) {
    w.write_all(&(s.oplog_bytes.len() as u32).to_le_bytes())
        .unwrap();
    w.write_all(&s.oplog_bytes).unwrap();
    w.write_all(&(s.state_bytes.len() as u32).to_le_bytes())
        .unwrap();
    w.write_all(&s.state_bytes).unwrap();
    w.write_all(&(s.gc_bytes.len() as u32).to_le_bytes())
        .unwrap();
    w.write_all(&s.gc_bytes).unwrap();
}

pub(super) fn _decode_snapshot_bytes(bytes: Bytes) -> LoroResult<Snapshot> {
    let mut r = bytes.reader();
    let oplog_bytes_len = read_u32_le(&mut r) as usize;
    let oplog_bytes = r.get_mut().copy_to_bytes(oplog_bytes_len);
    let state_bytes_len = read_u32_le(&mut r) as usize;
    let state_bytes = r.get_mut().copy_to_bytes(state_bytes_len);
    let gc_bytes_len = read_u32_le(&mut r) as usize;
    let gc_bytes = r.get_mut().copy_to_bytes(gc_bytes_len);
    Ok(Snapshot {
        oplog_bytes,
        state_bytes,
        gc_bytes,
    })
}

fn read_u32_le(r: &mut bytes::buf::Reader<Bytes>) -> u32 {
    let mut buf = [0; 4];
    r.read_exact(&mut buf).unwrap();
    u32::from_le_bytes(buf)
}

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
    let Snapshot {
        oplog_bytes,
        state_bytes,
        gc_bytes,
    } = _decode_snapshot_bytes(bytes)?;
    oplog.decode_change_store(oplog_bytes)?;
    if gc_bytes.is_empty() {
        state.store.decode(state_bytes)?;
    } else {
        state.store.decode_gc(
            gc_bytes,
            state_bytes,
            oplog.dag().trimmed_frontiers().clone(),
        )?;
    }
    // FIXME: we may need to extract the unknown containers here?
    // Or we should lazy load it when the time comes?
    state.init_with_states_and_version(oplog.frontiers().clone(), &oplog, vec![], false);
    Ok(())
}

impl OpLog {
    pub(super) fn decode_change_store(&mut self, bytes: bytes::Bytes) -> LoroResult<()> {
        let v = self.change_store().import_all(bytes)?;
        self.next_lamport = v.next_lamport;
        self.latest_timestamp = v.max_timestamp;
        // FIXME: handle start vv and start frontiers
        self.dag.set_version_by_fast_snapshot_import(v);
        Ok(())
    }
}

pub(crate) fn encode_snapshot<W: std::io::Write>(doc: &LoroDoc, w: &mut W) {
    let mut state = doc.app_state().try_lock().unwrap();
    let oplog = doc.oplog().try_lock().unwrap();
    assert!(!state.is_in_txn());
    assert_eq!(oplog.frontiers(), &state.frontiers);

    let oplog_bytes = oplog.encode_change_store();
    let state_bytes = state.store.encode();

    if oplog.is_trimmed() {
        assert_eq!(
            oplog.trimmed_frontiers(),
            state.store.trimmed_frontiers().unwrap()
        );
    }

    _encode_snapshot(
        Snapshot {
            oplog_bytes,
            state_bytes,
            gc_bytes: state.store.encode_gc(),
        },
        w,
    );
}

pub(crate) fn encode_updates<W: std::io::Write>(doc: &LoroDoc, vv: &VersionVector, w: &mut W) {
    let oplog = doc.oplog().try_lock().unwrap();
    let bytes = oplog.export_from_fast(vv);
    _encode_snapshot(
        Snapshot {
            oplog_bytes: bytes,
            state_bytes: Bytes::new(),
            gc_bytes: Bytes::new(),
        },
        w,
    );
}

pub(crate) fn decode_oplog(oplog: &mut OpLog, bytes: &[u8]) -> Result<(), LoroError> {
    let oplog_len = u32::from_le_bytes(bytes[0..4].try_into().unwrap());
    let oplog_bytes = &bytes[4..4 + oplog_len as usize];
    let mut changes = ChangeStore::decode_snapshot_for_updates(
        oplog_bytes.to_vec().into(),
        &oplog.arena,
        oplog.vv(),
    )?;
    changes.sort_unstable_by_key(|x| x.lamport);
    let (latest_ids, pending_changes) = import_changes_to_oplog(changes, oplog)?;
    // TODO: PERF: should we use hashmap to filter latest_ids with the same peer first?
    oplog.try_apply_pending(latest_ids);
    oplog.import_unknown_lamport_pending_changes(pending_changes)?;
    Ok(())
}

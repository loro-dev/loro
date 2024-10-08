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

use crate::{
    change::Change, encoding::shallow_snapshot, oplog::ChangeStore, LoroDoc, OpLog, VersionVector,
};
use bytes::{Buf, Bytes};
use loro_common::{IdSpan, LoroError, LoroResult};
use tracing::trace;
pub(crate) const EMPTY_MARK: &[u8] = b"E";
pub(crate) struct Snapshot {
    pub oplog_bytes: Bytes,
    pub state_bytes: Option<Bytes>,
    pub shallow_root_state_bytes: Bytes,
}

pub(super) fn _encode_snapshot<W: Write>(s: Snapshot, w: &mut W) {
    w.write_all(&(s.oplog_bytes.len() as u32).to_le_bytes())
        .unwrap();
    w.write_all(&s.oplog_bytes).unwrap();
    let state_bytes = s
        .state_bytes
        .unwrap_or_else(|| Bytes::from_static(EMPTY_MARK));
    w.write_all(&(state_bytes.len() as u32).to_le_bytes())
        .unwrap();
    w.write_all(&state_bytes).unwrap();
    w.write_all(&(s.shallow_root_state_bytes.len() as u32).to_le_bytes())
        .unwrap();
    w.write_all(&s.shallow_root_state_bytes).unwrap();
}

pub(super) fn _decode_snapshot_bytes(bytes: Bytes) -> LoroResult<Snapshot> {
    let mut r = bytes.reader();
    let oplog_bytes_len = read_u32_le(&mut r) as usize;
    let oplog_bytes = r.get_mut().copy_to_bytes(oplog_bytes_len);
    let state_bytes_len = read_u32_le(&mut r) as usize;
    let state_bytes = r.get_mut().copy_to_bytes(state_bytes_len);
    let state_bytes = if state_bytes == EMPTY_MARK {
        None
    } else {
        Some(state_bytes)
    };
    let shallow_bytes_len = read_u32_le(&mut r) as usize;
    let shallow_root_state_bytes = r.get_mut().copy_to_bytes(shallow_bytes_len);
    Ok(Snapshot {
        oplog_bytes,
        state_bytes,
        shallow_root_state_bytes,
    })
}

fn read_u32_le(r: &mut bytes::buf::Reader<Bytes>) -> u32 {
    let mut buf = [0; 4];
    r.read_exact(&mut buf).unwrap();
    u32::from_le_bytes(buf)
}

pub(crate) fn decode_snapshot(doc: &LoroDoc, bytes: Bytes) -> LoroResult<()> {
    let snapshot = _decode_snapshot_bytes(bytes)?;
    decode_snapshot_inner(snapshot, doc)
}

pub(crate) fn decode_snapshot_inner(snapshot: Snapshot, doc: &LoroDoc) -> Result<(), LoroError> {
    let Snapshot {
        oplog_bytes,
        state_bytes,
        shallow_root_state_bytes,
    } = snapshot;
    ensure_cov::notify_cov("loro_internal::import::fast_snapshot::decode_snapshot");
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
        panic!("InternalError importing snapshot to an non-empty doc");
    }

    assert!(state.frontiers.is_empty());
    assert!(oplog.frontiers().is_empty());
    oplog.decode_change_store(oplog_bytes)?;
    let need_calc = state_bytes.is_none();
    let state_frontiers;
    if shallow_root_state_bytes.is_empty() {
        ensure_cov::notify_cov("loro_internal::import::snapshot::normal");
        if let Some(bytes) = state_bytes {
            state.store.decode(bytes)?;
        }
        state_frontiers = oplog.frontiers().clone();
    } else {
        ensure_cov::notify_cov("loro_internal::import::snapshot::gc");
        let shallow_root_state_frontiers = state.store.decode_gc(
            shallow_root_state_bytes.clone(),
            oplog.dag().shallow_since_frontiers().clone(),
        )?;
        state
            .store
            .decode_state_by_two_bytes(shallow_root_state_bytes, state_bytes.unwrap_or_default())?;

        let shallow_root_store = state.shallow_root_store().cloned();
        oplog.with_history_cache(|h| {
            h.set_shallow_root_store(shallow_root_store);
        });

        if need_calc {
            ensure_cov::notify_cov("shallow_snapshot::need_calc");
            state_frontiers = shallow_root_state_frontiers.unwrap();
        } else {
            ensure_cov::notify_cov("shallow_snapshot::dont_need_calc");
            state_frontiers = oplog.frontiers().clone();
        }
    }

    // FIXME: we may need to extract the unknown containers here?
    // Or we should lazy load it when the time comes?

    state.init_with_states_and_version(state_frontiers, &oplog, vec![], false);
    drop(oplog);
    drop(state);
    if need_calc {
        doc.detach();
        doc.checkout_to_latest();
        debug_assert_eq!(doc.state_frontiers(), doc.oplog_frontiers());
    }

    Ok(())
}

impl OpLog {
    pub(super) fn decode_change_store(&mut self, bytes: bytes::Bytes) -> LoroResult<()> {
        let v = self.change_store().import_all(bytes)?;
        // FIXME: handle start vv and start frontiers
        self.dag.set_version_by_fast_snapshot_import(v);
        Ok(())
    }
}

pub(crate) fn encode_snapshot<W: std::io::Write>(doc: &LoroDoc, w: &mut W) {
    let snapshot = encode_snapshot_inner(doc);
    _encode_snapshot(snapshot, w);
}

pub(crate) fn encode_snapshot_inner(doc: &LoroDoc) -> Snapshot {
    assert!(doc.drop_pending_events().is_empty());
    let old_state_frontiers = doc.state_frontiers();
    let was_detached = doc.is_detached();
    let mut state = doc.app_state().try_lock().unwrap();
    let oplog = doc.oplog().try_lock().unwrap();
    let is_gc = state.store.shallow_root_store().is_some();
    if is_gc {
        // TODO: PERF: this can be optimized by reusing the bytes of gc store
        let f = oplog.shallow_since_frontiers().clone();
        drop(oplog);
        drop(state);
        let (snapshot, _) = shallow_snapshot::export_shallow_snapshot_inner(doc, &f).unwrap();
        return snapshot;
    }

    assert!(!state.is_in_txn());
    let oplog_bytes = oplog.encode_change_store();
    if oplog.is_shallow() {
        assert_eq!(
            oplog.shallow_since_frontiers(),
            state.store.shallow_root_frontiers().unwrap()
        );
    }
    if was_detached {
        let latest = oplog.frontiers().clone();
        drop(oplog);
        drop(state);
        doc.checkout_without_emitting(&latest).unwrap();
        state = doc.app_state().try_lock().unwrap();
    }
    state.ensure_all_alive_containers();
    let state_bytes = state.store.encode();
    let snapshot = Snapshot {
        oplog_bytes,
        state_bytes: Some(state_bytes),
        shallow_root_state_bytes: Bytes::new(),
    };
    if was_detached {
        drop(state);
        doc.checkout_without_emitting(&old_state_frontiers).unwrap();
        doc.drop_pending_events();
    }

    snapshot
}

pub(crate) fn decode_oplog(oplog: &mut OpLog, bytes: &[u8]) -> Result<Vec<Change>, LoroError> {
    let oplog_len = u32::from_le_bytes(bytes[0..4].try_into().unwrap());
    let oplog_bytes = &bytes[4..4 + oplog_len as usize];
    let mut changes = ChangeStore::decode_snapshot_for_updates(
        oplog_bytes.to_vec().into(),
        &oplog.arena,
        oplog.vv(),
    )?;
    changes.sort_unstable_by_key(|x| x.lamport);
    Ok(changes)
}

pub(crate) fn encode_updates<W: std::io::Write>(doc: &LoroDoc, vv: &VersionVector, w: &mut W) {
    let oplog = doc.oplog().try_lock().unwrap();
    oplog.export_blocks_from(vv, w);
}

pub(crate) fn encode_updates_in_range<W: std::io::Write>(
    oplog: &OpLog,
    spans: &[IdSpan],
    w: &mut W,
) {
    oplog.export_blocks_in_range(spans, w);
}

pub(crate) fn decode_updates(oplog: &mut OpLog, body: Bytes) -> Result<Vec<Change>, LoroError> {
    let mut reader: &[u8] = body.as_ref();
    let mut index = 0;
    let self_vv = oplog.vv();
    let mut changes = Vec::new();
    while !reader.is_empty() {
        let old_reader_len = reader.len();
        let len = leb128::read::unsigned(&mut reader).unwrap() as usize;
        index += old_reader_len - reader.len();
        trace!("index={}", index);
        let block_bytes = body.slice(index..index + len);
        trace!("decoded block_bytes = {:?}", &block_bytes);
        let new_changes = ChangeStore::decode_block_bytes(block_bytes, &oplog.arena, self_vv)?;
        changes.extend(new_changes);
        index += len;
        reader = &reader[len..];
    }

    changes.sort_unstable_by_key(|x| x.lamport);
    Ok(changes)
}

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
    change::Change, encoding::shallow_snapshot, oplog::ChangeStore, version::Frontiers, LoroDoc,
    OpLog, VersionVector,
};
use bytes::{Buf, Bytes};
use loro_common::{HasCounterSpan, IdSpan, InternalString, LoroEncodeError, LoroError, LoroResult};

use super::{EncodedBlobMode, ImportBlobMetadata, ParsedHeaderAndBody};
pub(crate) const EMPTY_MARK: &[u8] = b"E";
pub(crate) struct Snapshot {
    pub oplog_bytes: Bytes,
    pub state_bytes: Option<Bytes>,
    pub shallow_root_state_bytes: Bytes,
}

impl Snapshot {
    pub(super) fn encoded_len(&self) -> Option<usize> {
        let state_len = self
            .state_bytes
            .as_ref()
            .map_or(EMPTY_MARK.len(), Bytes::len);
        u32::try_from(self.oplog_bytes.len()).ok()?;
        u32::try_from(state_len).ok()?;
        u32::try_from(self.shallow_root_state_bytes.len()).ok()?;
        (3 * std::mem::size_of::<u32>())
            .checked_add(self.oplog_bytes.len())?
            .checked_add(state_len)?
            .checked_add(self.shallow_root_state_bytes.len())
    }
}

pub(super) fn _encode_snapshot<W: Write>(s: &Snapshot, w: &mut W) {
    w.write_all(&(s.oplog_bytes.len() as u32).to_le_bytes())
        .unwrap();
    w.write_all(&s.oplog_bytes).unwrap();
    let state_bytes = s.state_bytes.as_deref().unwrap_or(EMPTY_MARK);
    w.write_all(&(state_bytes.len() as u32).to_le_bytes())
        .unwrap();
    w.write_all(state_bytes).unwrap();
    w.write_all(&(s.shallow_root_state_bytes.len() as u32).to_le_bytes())
        .unwrap();
    w.write_all(&s.shallow_root_state_bytes).unwrap();
}

pub(super) fn _decode_snapshot_bytes(bytes: Bytes) -> LoroResult<Snapshot> {
    let mut r = bytes.reader();
    let oplog_bytes_len = read_u32_le(&mut r)? as usize;
    if r.get_ref().len() < oplog_bytes_len {
        return Err(LoroError::DecodeError(
            "decode_snapshot: invalid oplog bytes length"
                .to_string()
                .into_boxed_str(),
        ));
    }
    let oplog_bytes = r.get_mut().copy_to_bytes(oplog_bytes_len);
    let state_bytes_len = read_u32_le(&mut r)? as usize;
    if r.get_ref().len() < state_bytes_len {
        return Err(LoroError::DecodeError(
            "decode_snapshot: invalid state bytes length"
                .to_string()
                .into_boxed_str(),
        ));
    }
    let state_bytes = r.get_mut().copy_to_bytes(state_bytes_len);
    let state_bytes = if state_bytes == EMPTY_MARK {
        None
    } else {
        Some(state_bytes)
    };
    let shallow_bytes_len = read_u32_le(&mut r)? as usize;
    if r.get_ref().len() < shallow_bytes_len {
        return Err(LoroError::DecodeError(
            "decode_snapshot: invalid shallow root bytes length"
                .to_string()
                .into_boxed_str(),
        ));
    }
    let shallow_root_state_bytes = r.get_mut().copy_to_bytes(shallow_bytes_len);
    if r.get_ref().has_remaining() {
        return Err(LoroError::DecodeError(
            "decode_snapshot: trailing bytes after snapshot"
                .to_string()
                .into_boxed_str(),
        ));
    }

    Ok(Snapshot {
        oplog_bytes,
        state_bytes,
        shallow_root_state_bytes,
    })
}

pub(super) fn _decode_snapshot_meta_partial(bytes: &[u8]) -> LoroResult<(&[u8], bool)> {
    let mut r = bytes;
    let oplog_bytes_len = read_u32_le_slice(&mut r)? as usize;
    if r.len() < oplog_bytes_len {
        return Err(LoroError::DecodeDataCorruptionError);
    }
    let oplog_bytes = &r[..oplog_bytes_len];
    r = &r[oplog_bytes_len..];
    let state_bytes_len = read_u32_le_slice(&mut r)? as usize;
    if r.len() < state_bytes_len {
        return Err(LoroError::DecodeDataCorruptionError);
    }
    r = &r[state_bytes_len..];
    let shallow_bytes_len = read_u32_le_slice(&mut r)? as usize;
    if r.len() < shallow_bytes_len {
        return Err(LoroError::DecodeDataCorruptionError);
    }
    r = &r[shallow_bytes_len..];
    if !r.is_empty() {
        return Err(LoroError::DecodeDataCorruptionError);
    }

    Ok((oplog_bytes, shallow_bytes_len > 0))
}

fn read_u32_le_slice(r: &mut &[u8]) -> LoroResult<u32> {
    let mut buf = [0; 4];
    r.read_exact(&mut buf)
        .map_err(|_| LoroError::DecodeDataCorruptionError)?;
    Ok(u32::from_le_bytes(buf))
}

fn read_u32_le(r: &mut bytes::buf::Reader<Bytes>) -> LoroResult<u32> {
    let mut buf = [0; 4];
    r.read_exact(&mut buf).map_err(|_| {
        LoroError::DecodeError(
            "decode_snapshot: unexpected end of snapshot bytes"
                .to_string()
                .into_boxed_str(),
        )
    })?;
    Ok(u32::from_le_bytes(buf))
}

pub(crate) fn decode_snapshot(
    doc: &LoroDoc,
    bytes: Bytes,
    origin: InternalString,
) -> LoroResult<()> {
    let snapshot = _decode_snapshot_bytes(bytes)?;
    decode_snapshot_inner(snapshot, doc, origin)
}

pub(crate) fn decode_snapshot_inner(
    snapshot: Snapshot,
    doc: &LoroDoc,
    origin: InternalString,
) -> Result<(), LoroError> {
    let Snapshot {
        oplog_bytes,
        state_bytes,
        shallow_root_state_bytes,
    } = snapshot;
    ensure_cov::notify_cov("loro_internal::import::fast_snapshot::decode_snapshot");
    let mut oplog = doc.oplog().lock();
    if !oplog.is_empty() {
        return Err(LoroError::DecodeError(
            "decode_snapshot: cannot import snapshot into a non-empty doc"
                .to_string()
                .into_boxed_str(),
        ));
    }

    let mut state = doc.app_state().lock();

    state.check_before_decode_snapshot()?;

    if !state.frontiers.is_empty() {
        return Err(LoroError::DecodeError(
            "decode_snapshot: app state frontiers must be empty before import"
                .to_string()
                .into_boxed_str(),
        ));
    }

    if !oplog.frontiers().is_empty() {
        return Err(LoroError::DecodeError(
            "decode_snapshot: oplog frontiers must be empty before import"
                .to_string()
                .into_boxed_str(),
        ));
    }
    let need_calc = state_bytes.is_none();
    let checkout_origin = origin.clone();

    let arena_checkpoint = oplog.arena.checkpoint_for_rollback();
    let decode_result = (|| -> LoroResult<()> {
        oplog.decode_change_store(oplog_bytes)?;
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
                doc.config.clone(),
            )?;
            state.store.decode_state_by_two_bytes(
                shallow_root_state_bytes,
                state_bytes.unwrap_or_default(),
            )?;

            let shallow_root_store = state.shallow_root_store().cloned();
            oplog.with_history_cache(|h| {
                h.set_shallow_root_store(shallow_root_store);
            });

            if need_calc {
                ensure_cov::notify_cov("shallow_snapshot::need_calc");
                state_frontiers = shallow_root_state_frontiers.ok_or_else(|| {
                    LoroError::DecodeError(
                        "decode_snapshot: shallow root frontiers are missing"
                            .to_string()
                            .into_boxed_str(),
                    )
                })?;
            } else {
                ensure_cov::notify_cov("shallow_snapshot::dont_need_calc");
                state_frontiers = oplog.frontiers().clone();
            }
        }

        state.init_with_states_and_version(state_frontiers, &oplog, vec![], false, origin)?;
        Ok(())
    })();

    if let Err(e) = decode_result {
        state.reset_to_empty_for_failed_snapshot_import();
        oplog.reset_to_empty_for_failed_snapshot_import(arena_checkpoint);
        return Err(e);
    }
    drop(state);
    drop(oplog);
    if need_calc {
        doc.set_detached(true);
        if let Err(e) = doc._checkout_to_latest_without_commit_as_import(false, checkout_origin) {
            doc.set_detached(false);
            doc.app_state()
                .lock()
                .reset_to_empty_for_failed_snapshot_import();
            doc.oplog()
                .lock()
                .reset_to_empty_for_failed_snapshot_import(arena_checkpoint);
            return Err(e);
        }
        debug_assert_eq!(doc.state_frontiers(), doc.oplog_frontiers());
    }

    Ok(())
}

impl OpLog {
    pub(super) fn decode_change_store(&mut self, bytes: bytes::Bytes) -> LoroResult<()> {
        let v = self.change_store().import_all(bytes)?;
        self.dag.set_version_by_fast_snapshot_import(v);
        self.refresh_visible_op_count();
        Ok(())
    }
}

pub(crate) fn encode_snapshot_inner(doc: &LoroDoc) -> Result<Snapshot, LoroEncodeError> {
    assert!(doc.drop_pending_events().is_empty());
    let old_state_frontiers = doc.state_frontiers();
    let was_detached = doc.is_detached();
    let oplog = doc.oplog().lock();
    let mut state = doc.app_state().lock();
    let is_gc = state.store.shallow_root_store().is_some();
    if is_gc {
        // TODO: PERF: this can be optimized by reusing the bytes of gc store
        let f = oplog.shallow_since_frontiers().clone();
        drop(state);
        drop(oplog);
        let (snapshot, _) = shallow_snapshot::export_shallow_snapshot_inner(doc, &f)?;
        return Ok(snapshot);
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
        drop(state);
        drop(oplog);
        doc._checkout_without_emitting(&latest, false, true)
            .unwrap();
        state = doc.app_state().lock();
    }
    let snapshot = match state.ensure_all_alive_containers() {
        Ok(_) => Ok(Snapshot {
            oplog_bytes,
            state_bytes: Some(state.store.encode()),
            shallow_root_state_bytes: Bytes::new(),
        }),
        Err(err) => Err(LoroEncodeError::from(err)),
    };
    if was_detached {
        drop(state);
        doc._checkout_without_emitting(&old_state_frontiers, false, true)
            .unwrap();
        doc.drop_pending_events();
    }

    snapshot
}

pub(crate) fn decode_oplog(oplog: &mut OpLog, bytes: &[u8]) -> Result<Vec<Change>, LoroError> {
    let oplog_len = bytes
        .get(0..4)
        .ok_or_else(|| LoroError::DecodeError("decode_oplog: missing length prefix".into()))?;
    let oplog_len = u32::from_le_bytes(
        oplog_len
            .try_into()
            .expect("slice length checked to be exactly 4"),
    ) as usize;
    let oplog_bytes = bytes
        .get(4..4 + oplog_len)
        .ok_or_else(|| LoroError::DecodeError("decode_oplog: invalid oplog length".into()))?;
    let mut changes = ChangeStore::decode_snapshot_for_updates(
        oplog_bytes.to_vec().into(),
        &oplog.arena,
        oplog.vv(),
    )?;
    changes.sort_unstable_by_key(|x| x.lamport);
    Ok(changes)
}

pub(crate) fn encode_updates<W: std::io::Write>(doc: &LoroDoc, vv: &VersionVector, w: &mut W) {
    let oplog = doc.oplog().lock();
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
        let len = leb128::read::unsigned(&mut reader)
            .map_err(|_| LoroError::DecodeError("decode_updates: invalid block length".into()))?
            as usize;
        index += old_reader_len - reader.len();
        let end = index.checked_add(len).ok_or_else(|| {
            LoroError::DecodeError("decode_updates: block length overflow".into())
        })?;
        if end > body.len() {
            return Err(LoroError::DecodeError(
                "decode_updates: truncated block payload".into(),
            ));
        }
        let block_bytes = body.slice(index..end);
        let new_changes = ChangeStore::decode_block_bytes(block_bytes, &oplog.arena, self_vv)?;
        changes.extend(new_changes);
        index = end;
        reader = &reader[len..];
    }

    changes.sort_unstable_by_key(|x| x.lamport);
    Ok(changes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        encoding::ExportMode, handler::HandlerTrait, utils::kv_wrapper::KvWrapper, MapHandler,
    };

    fn snapshot_sections(doc: &LoroDoc) -> Snapshot {
        let encoded = doc.export(ExportMode::Snapshot).unwrap();
        let parsed = crate::encoding::parse_header_and_body(&encoded, true).unwrap();
        _decode_snapshot_bytes(Bytes::copy_from_slice(parsed.body)).unwrap()
    }

    #[test]
    fn snapshot_encoded_len_matches_written_len() {
        for snapshot in [
            Snapshot {
                oplog_bytes: Bytes::from_static(b"oplog"),
                state_bytes: Some(Bytes::from_static(b"state")),
                shallow_root_state_bytes: Bytes::new(),
            },
            Snapshot {
                oplog_bytes: Bytes::new(),
                state_bytes: None,
                shallow_root_state_bytes: Bytes::from_static(b"shallow"),
            },
        ] {
            let expected_len = snapshot.encoded_len().unwrap();
            let mut encoded = Vec::new();
            _encode_snapshot(&snapshot, &mut encoded);
            assert_eq!(encoded.len(), expected_len);
        }
    }

    #[test]
    fn decode_updates_rejects_truncated_block() {
        let doc = LoroDoc::new();
        let mut oplog = doc.oplog.lock();
        let err = decode_updates(&mut oplog, Bytes::from_static(&[0x02, 0x01]))
            .expect_err("truncated update block should be rejected");
        assert!(matches!(err, LoroError::DecodeError(_)));
    }

    #[test]
    fn snapshot_export_rejects_lazy_child_parent_conflict() {
        fn doc_with_child(root: &str) -> (LoroDoc, loro_common::ContainerID) {
            let doc = LoroDoc::new_auto_commit();
            doc.set_peer_id(42).unwrap();
            let child = doc
                .get_map(root)
                .insert_container("child", MapHandler::new_detached())
                .unwrap();
            child.insert("value", root).unwrap();
            let child_id = child.id();
            (doc, child_id)
        }

        let (parent_a, child_a) = doc_with_child("parent-a");
        let (parent_b, child_b) = doc_with_child("parent-b");
        assert_eq!(child_a, child_b, "test setup needs the same child id");

        let mut sections_a = snapshot_sections(&parent_a);
        let sections_b = snapshot_sections(&parent_b);
        let state_a = KvWrapper::new_mem();
        state_a
            .import(sections_a.state_bytes.take().unwrap())
            .unwrap();
        let state_b = KvWrapper::new_mem();
        state_b.import(sections_b.state_bytes.unwrap()).unwrap();
        let child_key = child_a.to_bytes();
        state_a.insert(&child_key, state_b.get(&child_key).unwrap());

        let target = LoroDoc::new();
        decode_snapshot_inner(
            Snapshot {
                oplog_bytes: sections_a.oplog_bytes,
                state_bytes: Some(state_a.export()),
                shallow_root_state_bytes: sections_a.shallow_root_state_bytes,
            },
            &target,
            Default::default(),
        )
        .unwrap();

        let err = target
            .export(ExportMode::Snapshot)
            .expect_err("conflicting lazy parent metadata must not be exported");
        assert!(err.to_string().contains("snapshot state encodes parent"));
    }

    #[test]
    fn snapshot_export_rejects_malformed_lazy_container_wrapper() {
        let source = LoroDoc::new_auto_commit();
        source.get_map("root").insert("value", 1).unwrap();
        let mut sections = snapshot_sections(&source);
        let state = KvWrapper::new_mem();
        state.import(sections.state_bytes.take().unwrap()).unwrap();
        let root = loro_common::ContainerID::new_root("root", crate::ContainerType::Map);
        state.insert(
            &root.to_bytes(),
            Bytes::from(vec![crate::ContainerType::Map.to_u8()]),
        );

        let target = LoroDoc::new();
        decode_snapshot_inner(
            Snapshot {
                oplog_bytes: sections.oplog_bytes,
                state_bytes: Some(state.export()),
                shallow_root_state_bytes: sections.shallow_root_state_bytes,
            },
            &target,
            Default::default(),
        )
        .unwrap();

        let err = target
            .export(ExportMode::Snapshot)
            .expect_err("malformed lazy state must return an export error");
        assert!(err.to_string().contains("Decode container state failed"));
    }
}

pub(crate) fn decode_snapshot_blob_meta(
    parsed: ParsedHeaderAndBody,
) -> LoroResult<ImportBlobMetadata> {
    let (oplog_bytes, is_shallow) = _decode_snapshot_meta_partial(parsed.body)?;
    let mode = if is_shallow {
        EncodedBlobMode::ShallowSnapshot
    } else {
        EncodedBlobMode::Snapshot
    };

    let doc = LoroDoc::new();
    let mut oplog = doc.oplog.lock();
    oplog.decode_change_store(oplog_bytes.to_vec().into())?;
    let timestamp = oplog.get_greatest_timestamp(oplog.dag.frontiers());
    let f = oplog.dag.shallow_since_frontiers().clone();
    let start_timestamp = oplog.get_timestamp_of_version(&f);
    let change_num = oplog.change_store().change_num() as u32;

    Ok(ImportBlobMetadata {
        mode,
        partial_start_vv: oplog.dag.shallow_since_vv().to_vv(),
        partial_end_vv: oplog.vv().clone(),
        start_timestamp,
        start_frontiers: f,
        end_timestamp: timestamp,
        change_num,
    })
}

pub(crate) fn decode_updates_blob_meta(
    parsed: ParsedHeaderAndBody,
) -> LoroResult<ImportBlobMetadata> {
    let doc = LoroDoc::new();
    let mut oplog = doc.oplog.lock();
    let changes = decode_updates(&mut oplog, parsed.body.to_vec().into())?;
    let mut start_vv = VersionVector::new();
    let mut end_vv = VersionVector::new();
    for c in changes.iter() {
        match start_vv.get(&c.id.peer).copied() {
            Some(start) if start <= c.id.counter => {}
            _ => {
                start_vv.insert(c.id.peer, c.id.counter);
            }
        }
        match end_vv.get(&c.id.peer).copied() {
            Some(end) if end >= c.ctr_end() => {}
            _ => {
                end_vv.insert(c.id.peer, c.ctr_end());
            }
        }
    }

    let mut start_frontiers = Frontiers::new();
    for c in changes.iter() {
        for dep in c.deps().iter() {
            if let Some(start_counter) = start_vv.get(&dep.peer) {
                if *start_counter > dep.counter {
                    start_frontiers.push(dep);
                }
            } else if end_vv.get(&dep.peer).is_none() {
                start_frontiers.push(dep);
            }
        }
    }

    Ok(ImportBlobMetadata {
        mode: EncodedBlobMode::Updates,
        partial_start_vv: start_vv,
        partial_end_vv: end_vv,
        start_timestamp: changes.first().map(|x| x.timestamp).unwrap_or(0),
        start_frontiers,
        end_timestamp: changes.last().map(|x| x.timestamp).unwrap_or(0),
        change_num: changes.len() as u32,
    })
}

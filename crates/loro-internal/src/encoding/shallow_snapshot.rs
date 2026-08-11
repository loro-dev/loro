use bytes::Bytes;
use rle::HasLength;
use rustc_hash::FxHashSet;
use std::collections::BTreeSet;

use loro_common::{ContainerID, ContainerType, LoroEncodeError, LoroError, ID};

use crate::{
    container::{idx::ContainerIdx, list::list_op::InnerListOp},
    dag::DagUtils,
    encoding::fast_snapshot::{_encode_snapshot, Snapshot},
    state::{
        container_store::{ContainerWrapper, FRONTIERS_KEY},
        redact_dead_style_values, DocState,
    },
    utils::kv_wrapper::KvWrapper,
    version::{Frontiers, VersionVector},
    LoroDoc,
};

#[cfg(test)]
const MAX_OPS_NUM_TO_ENCODE_WITHOUT_LATEST_STATE: usize = 16;
#[cfg(not(test))]
const MAX_OPS_NUM_TO_ENCODE_WITHOUT_LATEST_STATE: usize = 256;

#[tracing::instrument(skip_all)]
pub(crate) fn export_shallow_snapshot<W: std::io::Write>(
    doc: &LoroDoc,
    start_from: &Frontiers,
    w: &mut W,
) -> Result<Frontiers, LoroEncodeError> {
    let (snapshot, start_from) = export_shallow_snapshot_inner(doc, start_from)?;
    _encode_snapshot(&snapshot, w);
    Ok(start_from)
}

pub(crate) fn export_shallow_snapshot_inner(
    doc: &LoroDoc,
    start_from: &Frontiers,
) -> Result<(Snapshot, Frontiers), LoroEncodeError> {
    let oplog = doc.oplog().lock();
    let start_from = calc_shallow_doc_start(&oplog, start_from);
    let mut start_vv = frontiers_to_vv_for_export(&oplog, &start_from, "export_shallow_snapshot")?;
    for id in start_from.iter() {
        // we need to include the ops in start_from, this can make things easier
        start_vv.insert(id.peer, id.counter);
    }

    #[cfg(debug_assertions)]
    {
        use crate::dag::Dag;
        if !start_from.is_empty() {
            assert!(start_from.len() == 1);
            let id = start_from.as_single().unwrap();
            let node = oplog.dag.get(id).unwrap();
            if id.counter == node.cnt {
                let vv = oplog.dag().frontiers_to_vv(&node.deps).unwrap();
                assert_eq!(vv, start_vv);
            } else {
                let vv = oplog
                    .dag()
                    .frontiers_to_vv(&Frontiers::from(id.inc(-1)))
                    .unwrap();
                assert_eq!(vv, start_vv);
            }
        }
    }

    loro_common::debug!(
        "start version vv={:?} frontiers={:?}",
        &start_vv,
        &start_from,
    );

    let latest_frontiers = oplog.frontiers().clone();
    let state_frontiers = doc.state_frontiers();
    let is_attached = !doc.is_detached();
    let oplog_bytes = oplog.export_change_store_from(&start_vv, &start_from);
    let latest_vv = oplog.vv();
    let ops_num: usize = latest_vv.sub_iter(&start_vv).map(|x| x.atom_len()).sum();
    if &start_from == oplog.shallow_since_frontiers() && state_frontiers == latest_frontiers {
        let mut state = doc.app_state().lock();
        if let Some((shallow_root_state_bytes, shallow_root_kv)) =
            state.store.shallow_root_state_for_export()
        {
            // Ops since the root are few enough to replay on import; otherwise
            // also ship the encoded latest state as an overlay.
            let overlay_kv = if ops_num > MAX_OPS_NUM_TO_ENCODE_WITHOUT_LATEST_STATE {
                let mut alive_c_bytes = shallow_root_kv.keys();
                if has_unknown_container_key(alive_c_bytes.iter()) {
                    return Err(LoroEncodeError::UnknownContainer);
                }

                state.ensure_all_alive_containers()?;
                state.store.flush();

                // All the containers that are created after start_from need to be encoded.
                for cid in state.store.iter_all_container_ids() {
                    if let ContainerID::Normal { peer, counter, .. } = cid {
                        let temp_id = ID::new(peer, counter);
                        if !start_from.contains(&temp_id) {
                            alive_c_bytes.insert(cid.to_bytes());
                        }
                    } else {
                        alive_c_bytes.insert(cid.to_bytes());
                    }
                }

                let new_kv = state.store.get_kv_clone();
                new_kv.remove_same(&shallow_root_kv);
                new_kv.retain_keys(&alive_c_bytes);
                Some(new_kv)
            } else {
                None
            };

            // The stored shallow-root bytes may predate dead-style redaction
            // (e.g. imported from an older export), so re-run it before reuse.
            let shallow_root_state_bytes =
                if redact_export_states(&shallow_root_kv, overlay_kv.as_ref())? {
                    // The cloned root kv has no FRONTIERS_KEY (InnerStore::decode
                    // strips it on import); restore it before export.
                    shallow_root_kv.insert(FRONTIERS_KEY, start_from.encode().into());
                    shallow_root_kv.export()
                } else {
                    shallow_root_state_bytes
                };

            return Ok((
                Snapshot {
                    oplog_bytes,
                    state_bytes: overlay_kv.map(|kv| kv.export()),
                    shallow_root_state_bytes,
                },
                start_from,
            ));
        }
    }
    drop(oplog);
    let result = (|| -> Result<Snapshot, LoroEncodeError> {
        doc._checkout_without_emitting(&start_from, false, false)
            .map_err(LoroEncodeError::from)?;
        let mut state = doc.app_state().lock();
        let alive_containers = state.ensure_all_alive_containers()?;
        if has_unknown_container(alive_containers.iter().copied()) {
            return Err(LoroEncodeError::UnknownContainer);
        }
        let mut alive_c_bytes = alive_indices_to_bytes(&state, &alive_containers);
        state.store.flush();
        let shallow_root_state_kv = state.store.get_kv_clone();
        drop(state);
        doc._checkout_without_emitting(&latest_frontiers, false, false)
            .map_err(LoroEncodeError::from)?;
        let latest_state_kv = if ops_num > MAX_OPS_NUM_TO_ENCODE_WITHOUT_LATEST_STATE {
            let mut state = doc.app_state().lock();
            state.ensure_all_alive_containers()?;
            state.store.encode();
            // All the containers that are created after start_from need to be encoded
            for cid in state.store.iter_all_container_ids() {
                if let ContainerID::Normal { peer, counter, .. } = cid {
                    let temp_id = ID::new(peer, counter);
                    if !start_from.contains(&temp_id) {
                        alive_c_bytes.insert(cid.to_bytes());
                    }
                } else {
                    alive_c_bytes.insert(cid.to_bytes());
                }
            }

            let new_kv = state.store.get_kv_clone();
            new_kv.remove_same(&shallow_root_state_kv);
            new_kv.retain_keys(&alive_c_bytes);
            Some(new_kv)
        } else {
            None
        };

        shallow_root_state_kv.retain_keys(&alive_c_bytes);
        redact_export_states(&shallow_root_state_kv, latest_state_kv.as_ref())?;
        let state_bytes = latest_state_kv.map(|kv| kv.export());
        shallow_root_state_kv.insert(FRONTIERS_KEY, start_from.encode().into());
        let shallow_root_state_bytes = shallow_root_state_kv.export();

        Ok(Snapshot {
            oplog_bytes,
            state_bytes,
            shallow_root_state_bytes,
        })
    })();

    restore_export_doc_state(doc, &state_frontiers, is_attached)?;
    doc.drop_pending_events();
    Ok((result?, start_from))
}

fn has_unknown_container(mut idxs: impl Iterator<Item = ContainerIdx>) -> bool {
    idxs.any(|idx| matches!(idx.get_type(), ContainerType::Unknown(_)))
}

fn has_unknown_container_key<'a>(mut keys: impl Iterator<Item = &'a Vec<u8>>) -> bool {
    keys.any(|key| ContainerID::from_bytes(key).is_unknown())
}

/// Nulls the values of rich-text style pairs that are dead in this state (no
/// text between the anchors). In a document whose history is trimmed at this
/// state such a pair can never style anything again, but its value would
/// otherwise ship in the export even though no read API can reach it. See
/// `context/shallow-snapshot-style-redaction.md`.
///
/// Containers are keyed by `ContainerID` bytes whose first byte is the
/// container type, so the scan stays within the two Text key ranges and blocks
/// holding other containers are passed through in their compressed form.
///
/// Returns the ids of the redacted `StyleStart` ops, or `None` when the KV was
/// left untouched.
/// Applies dead-style redaction to the states being exported.
///
/// The root state is fully redacted; the optional overlay (the encoded
/// latest/target state shipped alongside it) only redacts the pairs the root
/// redacted. Pairs that die *after* the root must keep their values so
/// checkouts into the retained range still render them — this function is the
/// single place that protocol lives.
///
/// Returns whether the root KV changed. Call before the KVs are exported and
/// after `remove_same`, so byte-identical entries still dedup (a deduped entry
/// resolves to the redacted root version on import).
fn redact_export_states(
    root: &KvWrapper,
    overlay: Option<&KvWrapper>,
) -> Result<bool, LoroEncodeError> {
    match redact_dead_text_styles(root, None)? {
        None => Ok(false),
        Some(redacted) => {
            if let Some(overlay) = overlay {
                redact_dead_text_styles(overlay, Some(&redacted))?;
            }
            Ok(true)
        }
    }
}

fn redact_dead_text_styles(
    kv: &KvWrapper,
    only_pairs: Option<&FxHashSet<ID>>,
) -> Result<Option<FxHashSet<ID>>, LoroEncodeError> {
    const ROOT_MARK: u8 = 0b1000_0000;
    let text_kind = ContainerType::Text.to_u8();
    debug_assert_eq!(text_kind & ROOT_MARK, 0);
    let mut redacted: FxHashSet<ID> = FxHashSet::default();
    let mut changed = false;
    for first_key_byte in [text_kind, text_kind | ROOT_MARK] {
        for (key, value) in kv.scan_range_entries(&[first_key_byte], &[first_key_byte + 1]) {
            if value.is_empty() {
                continue;
            }
            let offset = ContainerWrapper::payload_offset(&value).map_err(LoroEncodeError::from)?;
            if let Some((payload, ids)) = redact_dead_style_values(&value[offset..], only_pairs)
                .map_err(LoroEncodeError::from)?
            {
                let mut new_value = Vec::with_capacity(offset + payload.len());
                new_value.extend_from_slice(&value[..offset]);
                new_value.extend_from_slice(&payload);
                kv.insert(&key, new_value.into());
                redacted.extend(ids);
                changed = true;
            }
        }
    }
    Ok(changed.then_some(redacted))
}

pub(crate) fn export_state_only_snapshot<W: std::io::Write>(
    doc: &LoroDoc,
    target_frontiers: &Frontiers,
    w: &mut W,
) -> Result<Frontiers, LoroEncodeError> {
    let oplog = doc.oplog().lock();
    let start_from = calc_shallow_doc_start(&oplog, target_frontiers);
    let mut start_vv =
        frontiers_to_vv_for_export(&oplog, &start_from, "export_state_only_snapshot")?;
    for id in start_from.iter() {
        // we need to include the ops in start_from, this can make things easier
        start_vv.insert(id.peer, id.counter);
    }

    loro_common::debug!(
        "start version vv={:?} frontiers={:?}",
        &start_vv,
        &start_from,
    );

    let to_vv = frontiers_to_vv_for_export(&oplog, target_frontiers, "export_state_only_snapshot")?;
    let oplog_bytes =
        oplog.export_change_store_in_range(&start_vv, &start_from, &to_vv, target_frontiers);
    let state_frontiers = doc.state_frontiers();
    let is_attached = !doc.is_detached();
    drop(oplog);
    let result = (|| -> Result<(), LoroEncodeError> {
        doc._checkout_without_emitting(&start_from, false, false)
            .map_err(LoroEncodeError::from)?;
        let mut state = doc.app_state().lock();
        let alive_containers = state.ensure_all_alive_containers()?;
        if has_unknown_container(alive_containers.iter().copied()) {
            return Err(LoroEncodeError::UnknownContainer);
        }
        let mut alive_c_bytes = alive_indices_to_bytes(&state, &alive_containers);
        state.store.flush();
        let shallow_state_kv = state.store.get_kv_clone();
        drop(state);

        doc._checkout_without_emitting(target_frontiers, false, false)
            .map_err(LoroEncodeError::from)?;
        let mut state = doc.app_state().lock();
        state.ensure_all_alive_containers()?;
        state.store.encode();
        for cid in state.store.iter_all_container_ids() {
            if let ContainerID::Normal { peer, counter, .. } = cid {
                let temp_id = ID::new(peer, counter);
                if !start_from.contains(&temp_id) {
                    alive_c_bytes.insert(cid.to_bytes());
                }
            } else {
                alive_c_bytes.insert(cid.to_bytes());
            }
        }

        let target_state_kv = state.store.get_kv_clone();
        drop(state);
        target_state_kv.remove_same(&shallow_state_kv);
        target_state_kv.retain_keys(&alive_c_bytes);

        shallow_state_kv.retain_keys(&alive_c_bytes);
        redact_export_states(&shallow_state_kv, Some(&target_state_kv))?;
        shallow_state_kv.insert(FRONTIERS_KEY, start_from.encode().into());
        let shallow_state_bytes = shallow_state_kv.export();
        let snapshot = Snapshot {
            oplog_bytes,
            state_bytes: Some(target_state_kv.export()),
            shallow_root_state_bytes: shallow_state_bytes,
        };
        _encode_snapshot(&snapshot, w);
        Ok(())
    })();

    restore_export_doc_state(doc, &state_frontiers, is_attached)?;
    doc.drop_pending_events();
    result?;
    Ok(start_from)
}

fn alive_indices_to_bytes(
    state: &DocState,
    alive_containers: &rustc_hash::FxHashSet<ContainerIdx>,
) -> BTreeSet<Vec<u8>> {
    alive_containers
        .iter()
        .map(|idx| state.arena.get_container_id(*idx).unwrap().to_bytes())
        .collect()
}

fn frontiers_to_vv_for_export(
    oplog: &crate::OpLog,
    frontiers: &Frontiers,
    context: &str,
) -> Result<VersionVector, LoroEncodeError> {
    oplog.dag().frontiers_to_vv(frontiers).ok_or_else(|| {
        LoroEncodeError::FrontiersNotFound(format!(
            "{context}: unreachable frontiers {frontiers:?}"
        ))
    })
}

fn restore_export_doc_state(
    doc: &LoroDoc,
    state_frontiers: &Frontiers,
    was_attached: bool,
) -> Result<(), LoroEncodeError> {
    if &doc.state_frontiers() != state_frontiers {
        doc._checkout_without_emitting(state_frontiers, false, false)
            .map_err(LoroEncodeError::from)?;
    }

    if was_attached {
        doc.set_detached(false);
    }

    Ok(())
}

/// Calculates optimal starting version for the shallow doc
///
/// It should be a common ancestor version of the user-given version and the latest version.
/// Otherwise, users cannot replay the history from the initial version till the latest version.
fn calc_shallow_doc_start(oplog: &crate::OpLog, frontiers: &Frontiers) -> Frontiers {
    // Find a common ancestor version of the given frontiers by iterative pairwise reduction.
    // This converges to a single frontier or empty if there is no common ancestor.
    let mut current = frontiers.clone();
    while current.len() > 1 {
        let ids: Vec<ID> = current.iter().collect();
        let mut next = Frontiers::new();
        let mut i = 0;
        while i < ids.len() {
            if i + 1 < ids.len() {
                let (gca, _) = oplog
                    .dag()
                    .find_common_ancestor(&Frontiers::from(ids[i]), &Frontiers::from(ids[i + 1]));
                for id in gca.iter() {
                    next.push(id);
                }
            } else {
                next.push(ids[i]);
            }
            i += 2;
        }
        if next == current {
            // Cannot converge further (pairwise GCAs are the nodes themselves).
            // Fall back to empty frontiers, meaning export full history.
            return clamp_to_shallow_root(oplog, Frontiers::default());
        }
        current = next;
    }

    let mut ans = Frontiers::new();
    for id in current.iter() {
        let mut processed = false;
        if let Some(op) = oplog.get_op_that_includes(id) {
            if let crate::op::InnerContent::List(InnerListOp::StyleStart { .. }) = &op.content {
                // StyleStart and StyleEnd operations must be kept together in the GC snapshot.
                // Splitting them could lead to an weird document state that cannot be
                // properly encoded. To ensure they stay together, we advance the frontier by
                // one step to include both operations.

                // > Id.counter + 1 is guaranteed to be the StyleEnd Op
                ans.push(id.inc(1));
                processed = true;
            }
        }

        if !processed {
            ans.push(id);
        }
    }

    clamp_to_shallow_root(oplog, ans)
}

fn clamp_to_shallow_root(oplog: &crate::OpLog, frontiers: Frontiers) -> Frontiers {
    if oplog.shallow_since_vv().is_empty() {
        return frontiers;
    }

    let Some(vv) = oplog.dag().frontiers_to_vv(&frontiers) else {
        return oplog.shallow_since_frontiers().clone();
    };

    if vv.includes_vv(&oplog.shallow_since_vv().to_vv()) {
        frontiers
    } else {
        oplog.shallow_since_frontiers().clone()
    }
}

pub(crate) fn encode_snapshot_at<W: std::io::Write>(
    doc: &LoroDoc,
    frontiers: &Frontiers,
    w: &mut W,
) -> Result<(), LoroEncodeError> {
    let was_detached = doc.is_detached();
    let version_before_start = doc.state_frontiers().clone();
    doc._checkout_without_emitting(frontiers, true, false)
        .map_err(LoroEncodeError::from)?;
    let result = 'block: {
        let oplog = doc.oplog().lock();
        let mut state = doc.app_state().lock();
        let is_shallow = state.store.shallow_root_store().is_some();
        if is_shallow {
            break 'block Err(LoroEncodeError::from(LoroError::NotImplemented(
                "fork_at on shallow docs",
            )));
        }

        if state.is_in_txn() {
            break 'block Err(LoroEncodeError::internal(
                "encode_snapshot_at: state is unexpectedly still in a transaction",
            ));
        }
        let Some(oplog_bytes) = oplog.fork_changes_up_to(frontiers) else {
            break 'block Err(LoroEncodeError::FrontiersNotFound(format!(
                "frontiers: {:?} when export in SnapshotAt mode",
                frontiers
            )));
        };

        if oplog.is_shallow() {
            let Some(shallow_root_frontiers) = state.store.shallow_root_frontiers() else {
                break 'block Err(LoroEncodeError::internal(
                    "encode_snapshot_at: shallow oplog is missing shallow root frontiers",
                ));
            };
            if oplog.shallow_since_frontiers() != shallow_root_frontiers {
                break 'block Err(LoroEncodeError::internal(
                    "encode_snapshot_at: shallow root frontiers are inconsistent",
                ));
            }
        }

        let alive_containers = state.ensure_all_alive_containers()?;
        if has_unknown_container(alive_containers.iter().copied()) {
            break 'block Err(LoroEncodeError::UnknownContainer);
        }

        let alive_c_bytes = alive_indices_to_bytes(&state, &alive_containers);
        state.store.flush();
        let state_kv = state.store.get_kv_clone();
        state_kv.retain_keys(&alive_c_bytes);
        let bytes = state_kv.export();
        _encode_snapshot(
            &Snapshot {
                oplog_bytes,
                state_bytes: Some(bytes),
                shallow_root_state_bytes: Bytes::new(),
            },
            w,
        );

        Ok(())
    };
    let restore_result = doc
        ._checkout_without_emitting(&version_before_start, false, false)
        .map_err(LoroEncodeError::from);
    if !was_detached {
        doc.set_detached(false);
    }
    doc.app_state().lock().take_events();

    match result {
        Err(err) => Err(err),
        Ok(()) => restore_result,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::configure::Configure;
    use crate::container::idx::ContainerIdx;
    use crate::container::richtext::richtext_state::{PosType, RichtextStateChunk};
    use crate::container::richtext::AnchorType;
    use crate::encoding::fast_snapshot::_decode_snapshot_bytes;
    use crate::encoding::EncodeMode;
    use crate::encoding::ExportMode;
    use crate::handler::TextHandler;
    use crate::state::{ContainerCreationContext, FastStateSnapshot, RichtextState};
    use crate::HandlerTrait;
    use crate::LoroDoc;
    use loro_common::LoroValue;

    fn shallow_sections(blob: &[u8]) -> Snapshot {
        let parsed = crate::encoding::parse_header_and_body(blob, true).unwrap();
        _decode_snapshot_bytes(Bytes::copy_from_slice(parsed.body)).unwrap()
    }

    /// Reassemble decomposed sections into a complete importable blob
    /// (magic + checksum + mode header, matching `encode_with`).
    fn assemble_snapshot_blob(sections: &Snapshot) -> Vec<u8> {
        let mut blob = Vec::new();
        blob.extend(crate::encoding::MAGIC_BYTES);
        blob.extend([0u8; 16]);
        blob.extend(EncodeMode::FastSnapshot.to_bytes());
        _encode_snapshot(sections, &mut blob);
        let checksum = xxhash_rust::xxh32::xxh32(&blob[20..], crate::encoding::XXH_SEED);
        blob[16..20].copy_from_slice(&checksum.to_le_bytes());
        blob
    }

    /// Decode the style values of a text container straight out of exported
    /// state KV bytes, so assertions don't depend on whether the secret is
    /// visible through LZ4 compression.
    fn text_style_values(kv_bytes: &Bytes, cid: &ContainerID) -> Vec<(ID, LoroValue)> {
        let kv = KvWrapper::new_mem();
        kv.import(kv_bytes.clone()).unwrap();
        let value = kv
            .get(&cid.to_bytes())
            .expect("text container should be present in the exported state");
        let offset = ContainerWrapper::payload_offset(&value).unwrap();
        let (text, rest) = RichtextState::decode_value(&value[offset..]).unwrap();
        let idx = ContainerIdx::from_index_and_type(0, ContainerType::Text);
        let configure = Configure::default();
        let ctx = ContainerCreationContext {
            configure: &configure,
            peer: 0,
        };
        let state = RichtextState::decode_snapshot_fast(idx, (text, rest), ctx).unwrap();
        let mut styles = Vec::new();
        state.iter_raw(&mut |chunk| {
            if let RichtextStateChunk::Style {
                style,
                anchor_type: AnchorType::Start,
            } = chunk
            {
                styles.push((ID::new(style.peer, style.cnt), style.value.clone()));
            }
        });
        styles
    }

    fn root_text_cid() -> ContainerID {
        ContainerID::new_root("text", ContainerType::Text)
    }

    #[test]
    fn shallow_export_nulls_dead_style_values_for_root_and_nested_text() {
        let doc = LoroDoc::new_auto_commit();
        doc.set_peer_id(1).unwrap();
        let root_text = doc.get_text("text");
        root_text.insert(0, "abc", PosType::Unicode).unwrap();
        root_text
            .mark(0, 3, "comment", "root-secret".into(), PosType::Unicode)
            .unwrap();
        let nested_text = doc
            .get_map("meta")
            .insert_container("note", TextHandler::new_detached())
            .unwrap();
        nested_text.insert(0, "xyz", PosType::Unicode).unwrap();
        nested_text
            .mark(0, 3, "comment", "nested-secret".into(), PosType::Unicode)
            .unwrap();
        doc.commit_then_renew();
        root_text.delete(0, 3, PosType::Unicode).unwrap();
        nested_text.delete(0, 3, PosType::Unicode).unwrap();
        doc.commit_then_renew();

        let blob = doc
            .export(ExportMode::shallow_snapshot(&doc.oplog_frontiers()))
            .unwrap();
        let sections = shallow_sections(&blob);

        for cid in [root_text_cid(), nested_text.id()] {
            let styles = text_style_values(&sections.shallow_root_state_bytes, &cid);
            assert_eq!(styles.len(), 1, "container {cid}");
            assert_eq!(styles[0].1, LoroValue::Null, "container {cid}");
        }
    }

    #[test]
    fn overlay_keeps_style_values_that_die_after_the_root() {
        let doc = LoroDoc::new_auto_commit();
        doc.set_peer_id(1).unwrap();
        let text = doc.get_text("text");
        text.insert(0, "hello", PosType::Unicode).unwrap();
        text.mark(0, 5, "comment", "keep-me".into(), PosType::Unicode)
            .unwrap();
        doc.commit_then_renew();
        let start = doc.oplog_frontiers();

        // The style dies after the shallow root...
        text.delete(0, 5, PosType::Unicode).unwrap();
        // ...and enough tail ops force the export to carry the latest state.
        let filler = doc.get_map("filler");
        for i in 0..(MAX_OPS_NUM_TO_ENCODE_WITHOUT_LATEST_STATE + 1) {
            filler.insert(&i.to_string(), i as i64).unwrap();
        }
        doc.commit_then_renew();

        let blob = doc.export(ExportMode::shallow_snapshot(&start)).unwrap();
        let sections = shallow_sections(&blob);
        let state_bytes = sections
            .state_bytes
            .expect("overlay state should be encoded");

        let cid = root_text_cid();
        let keep = LoroValue::String("keep-me".into());
        assert_eq!(
            text_style_values(&sections.shallow_root_state_bytes, &cid)[0].1,
            keep,
            "style alive at the root must keep its value"
        );
        // The pair is dead in the latest state but was alive at the root, so a
        // checkout back into the retained range must still render it.
        assert_eq!(text_style_values(&state_bytes, &cid)[0].1, keep);
    }

    #[test]
    fn overlay_redacts_pairs_dead_at_the_root() {
        let doc = LoroDoc::new_auto_commit();
        doc.set_peer_id(1).unwrap();
        let text = doc.get_text("text");
        text.insert(0, "abc", PosType::Unicode).unwrap();
        text.mark(0, 3, "comment", "dead-secret".into(), PosType::Unicode)
            .unwrap();
        doc.commit_then_renew();
        text.delete(0, 3, PosType::Unicode).unwrap();
        doc.commit_then_renew();
        let start = doc.oplog_frontiers();

        // Make the latest text entry differ from the root entry so it survives
        // `remove_same` and must be redacted via the root's pair whitelist.
        text.insert(0, "later", PosType::Unicode).unwrap();
        let filler = doc.get_map("filler");
        for i in 0..(MAX_OPS_NUM_TO_ENCODE_WITHOUT_LATEST_STATE + 1) {
            filler.insert(&i.to_string(), i as i64).unwrap();
        }
        doc.commit_then_renew();

        let blob = doc.export(ExportMode::shallow_snapshot(&start)).unwrap();
        let sections = shallow_sections(&blob);
        let cid = root_text_cid();
        assert_eq!(
            text_style_values(&sections.shallow_root_state_bytes, &cid)[0].1,
            LoroValue::Null
        );
        let state_bytes = sections
            .state_bytes
            .expect("overlay state should be encoded");
        assert_eq!(text_style_values(&state_bytes, &cid)[0].1, LoroValue::Null);
    }

    #[test]
    fn legacy_unredacted_shallow_blob_is_cleaned_on_reexport() {
        let doc = LoroDoc::new_auto_commit();
        doc.set_peer_id(1).unwrap();
        let text = doc.get_text("text");
        text.insert(0, "abc", PosType::Unicode).unwrap();
        text.mark(0, 3, "comment", "legacy-secret".into(), PosType::Unicode)
            .unwrap();
        doc.commit_then_renew();
        text.delete(0, 3, PosType::Unicode).unwrap();
        doc.commit_then_renew();

        let blob = doc
            .export(ExportMode::shallow_snapshot(&doc.oplog_frontiers()))
            .unwrap();
        let mut sections = shallow_sections(&blob);

        // Rebuild the root state the way pre-redaction exports encoded it: the
        // doc's own state at the tip still holds the secret value.
        let unredacted_root = {
            let mut state = doc.app_state().lock();
            state.store.flush();
            let kv = state.store.get_kv_clone();
            drop(state);
            kv.insert(FRONTIERS_KEY, doc.oplog_frontiers().encode().into());
            kv.export()
        };
        let cid = root_text_cid();
        assert_eq!(
            text_style_values(&unredacted_root, &cid)[0].1,
            LoroValue::String("legacy-secret".into()),
            "test setup must produce an unredacted legacy root"
        );
        sections.shallow_root_state_bytes = unredacted_root;

        // Import the legacy-shaped blob; re-exporting must clean the stored
        // root bytes on the reuse branch.
        let legacy_doc = LoroDoc::new();
        legacy_doc
            .import(&assemble_snapshot_blob(&sections))
            .unwrap();
        let reexported = legacy_doc
            .export(ExportMode::shallow_snapshot(&legacy_doc.oplog_frontiers()))
            .unwrap();
        let resections = shallow_sections(&reexported);
        assert_eq!(
            text_style_values(&resections.shallow_root_state_bytes, &cid)[0].1,
            LoroValue::Null
        );
    }
}

//! Imported changes that reuse op ids the doc already has.
//!
//! An import skips the part of each change the doc already has (by version vector)
//! and applies the rest on top of the local history. When two clients shared a
//! peer id, the "known" part is different history, and the rest is then applied to
//! a state it was not written against: it panicked inside state apply (poisoning
//! the doc) or silently scrambled it (loro-dev/loro#1118). See
//! `context/import-peer-id-reuse.md`.

use loro_common::{Counter, HasCounterSpan, LoroError, LoroResult, PeerID, ID};
use rle::{HasLength, Sliceable};
use rustc_hash::FxHashMap;

use crate::{
    arena::SharedArena,
    change::Change,
    container::list::list_op::InnerListOp,
    op::{InnerContent, Op},
    version::Frontiers,
    OpLog,
};

#[cfg(feature = "counter")]
use crate::op::FutureInnerContent;

/// How faithfully the import format carries op values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ImportedValues {
    /// Binary encodings round-trip every value.
    Exact,
    /// JSON updates may have gone through JSON text, which does not round-trip
    /// every value (`NaN` becomes `null`, binary may come back as a list). Values
    /// are then not compared; everything else still is.
    Lossy,
}

impl OpLog {
    /// Check the decoded `changes` against the history this doc already has for the
    /// same op ids, then drop the parts it already has.
    ///
    /// Returns [`LoroError::UsedOpID`] with the first conflicting id when an imported
    /// change differs from the local one (deps or op content), and
    /// [`LoroError::DecodeError`] when a change skips counters of its peer (see
    /// [`Self::check_changes_do_not_skip_counters`]). It reads the op log only, so a
    /// caller can return the error before mutating anything.
    ///
    /// The comparison runs only when the import brings something new: that is what
    /// would be applied on top of a conflicting prefix. An import made only of
    /// known changes is a no-op either way and stays as cheap as before.
    pub(crate) fn check_and_trim_known_part_of_changes(
        &self,
        changes: Vec<Change>,
        values: ImportedValues,
    ) -> LoroResult<Vec<Change>> {
        self.check_and_trim_known_part_in_arena(changes, values, &self.arena)
    }

    /// Snapshot overlap is decoded into a temporary arena. Compare before moving
    /// only the retained suffix into the document's arena.
    pub(crate) fn check_and_trim_known_part_in_arena(
        &self,
        mut changes: Vec<Change>,
        values: ImportedValues,
        imported_arena: &SharedArena,
    ) -> LoroResult<Vec<Change>> {
        let vv = self.vv();
        let known_end = |c: &Change| vv.get(&c.id.peer).copied().unwrap_or(0);
        if changes.iter().any(|c| c.ctr_end() > known_end(c)) {
            for change in changes.iter() {
                if change.id.counter < known_end(change) {
                    self.check_known_part_of_change(
                        change,
                        known_end(change),
                        values,
                        imported_arena,
                    )?;
                }
            }
        }

        changes.retain_mut(|c| {
            let end = known_end(c);
            if c.id.counter >= end {
                true
            } else if c.ctr_end() > end {
                *c = c.slice((end - c.id.counter) as usize, c.atom_len());
                true
            } else {
                false
            }
        });
        self.check_changes_do_not_skip_counters(&changes)?;
        Ok(changes)
    }

    /// A change of peer `p` starting at counter `c > 0` is written after `p@c-1`, so
    /// `p@c-1` is in the causal past of its deps. When all its deps are present but
    /// `p@c-1` is not, the change skips counters and is invalid. The main pass of
    /// `import_changes_to_oplog` applies any change whose deps are present and
    /// would hit the counter assertion in `AppDag::update_version_on_new_change`
    /// under the op log lock. (Changes parked as pending already wait for `p@c-1`,
    /// see `remote_change_apply_state`.)
    ///
    /// This replays that pass on counter ends only: changes are visited in the same
    /// order, and a change counts as applied when its deps are.
    fn check_changes_do_not_skip_counters(&self, changes: &[Change]) -> LoroResult<()> {
        let vv = self.vv();
        let mut ends: FxHashMap<PeerID, Counter> = FxHashMap::default();
        for change in changes {
            let end_of = |ends: &FxHashMap<PeerID, Counter>, peer: &PeerID| {
                ends.get(peer)
                    .or_else(|| vv.get(peer))
                    .copied()
                    .unwrap_or(0)
            };
            let peer = change.id.peer;
            let end = end_of(&ends, &peer);
            if change.ctr_end() <= end
                || self.dag.try_import_deps_before_shallow_root(&change.deps)?
            {
                continue;
            }

            if !change
                .deps
                .iter()
                .all(|dep| dep.counter < end_of(&ends, &dep.peer))
            {
                // Parked as pending.
                continue;
            }

            if change.id.counter > end {
                return Err(LoroError::DecodeError(
                    format!(
                        "Invalid change {}: its deps are present but {} is not, so it skips counters of its peer",
                        change.id,
                        ID::new(peer, change.id.counter - 1),
                    )
                    .into_boxed_str(),
                ));
            }

            ends.insert(peer, change.ctr_end());
        }

        Ok(())
    }

    fn check_known_part_of_change(
        &self,
        change: &Change,
        known_end: Counter,
        values: ImportedValues,
        imported_arena: &SharedArena,
    ) -> LoroResult<()> {
        let peer = change.id.peer;
        // History before the shallow root is not stored, so it cannot be compared.
        let shallow_start = self.shallow_since_vv().get(&peer).copied().unwrap_or(0);
        let end = change.ctr_end().min(known_end);
        let mut ctr = change.id.counter.max(shallow_start);
        while ctr < end {
            if let Some(checked_end) =
                self.check_known_text_in_cold_block(change, imported_arena, ctr, end)?
            {
                ctr = checked_end;
                continue;
            }
            // Missing local history cannot be compared; keep the old behavior for it.
            let Some(local) = self.change_store.get_change(ID::new(peer, ctr)) else {
                break;
            };
            let seg_end = end.min(local.ctr_end());
            if seg_end <= ctr {
                break;
            }

            if deps_at(change, ctr) != deps_at(&local, ctr) {
                return Err(LoroError::UsedOpID {
                    id: ID::new(peer, ctr),
                });
            }

            if let Some(bad) = first_mismatch(
                imported_arena,
                &self.arena,
                values,
                change,
                &local,
                ctr,
                seg_end,
            ) {
                return Err(LoroError::UsedOpID {
                    id: ID::new(peer, bad),
                });
            }

            ctr = seg_end;
        }

        Ok(())
    }
    /// Compare cold Text insert history directly from its encoded block. This
    /// preserves every dependency boundary and op atom without retaining the
    /// block's parsed changes or strings. Unsupported blocks use the full check.
    fn check_known_text_in_cold_block(
        &self,
        change: &Change,
        arena: &SharedArena,
        start: Counter,
        end: Counter,
    ) -> LoroResult<Option<Counter>> {
        let Some(bytes) = self
            .change_store
            .unparsed_block_bytes(ID::new(change.id.peer, start))
        else {
            return Ok(None);
        };
        let Ok(range) = crate::oplog::ChangeStore::encoded_block_counter_range(&bytes) else {
            return Ok(None);
        };
        // A short imported change must not rescan a large local block for each
        // atom/change. Parse it once and reuse the general comparison instead.
        if end < range.1 {
            return Ok(None);
        }
        let block_end = end.min(range.1);
        let mut cursor = OpCursor::new(change, start);
        let checked = self.change_store.check_text_insert_block(
            &bytes,
            |counter, cid, pos, text, len, deps| {
                let local_end = counter + len as Counter;
                let mut ctr = counter.max(start);
                let limit = local_end.min(end);
                while ctr < limit {
                    let bad = || LoroError::UsedOpID {
                        id: ID::new(change.id.peer, ctr),
                    };
                    let local_deps = if ctr == counter {
                        deps.clone()
                    } else {
                        Frontiers::from_id(ID::new(change.id.peer, ctr - 1))
                    };
                    if deps_at(change, ctr) != local_deps {
                        return Err(bad());
                    }
                    let Some(op) = cursor.op_at(ctr) else {
                        return Err(bad());
                    };
                    let n = (limit - ctr).min(op.ctr_end() - ctr);
                    let slice = slice_op(op, ctr, n);
                    let InnerContent::List(InnerListOp::InsertText {
                        slice: imported,
                        pos: imported_pos,
                        unicode_len,
                        ..
                    }) = &slice.content
                    else {
                        return Err(bad());
                    };
                    if arena.idx_to_id(op.container).as_ref() != Some(cid)
                        || pos
                            .checked_add((ctr - counter) as u32)
                            .is_none_or(|p| *imported_pos != p)
                        || *unicode_len != n as u32
                    {
                        return Err(bad());
                    }
                    let imported = std::str::from_utf8(imported).unwrap();
                    let offset = (ctr - counter) as usize;
                    let local = if offset == 0 && n as u32 == len {
                        text
                    } else {
                        let byte_start = text
                            .char_indices()
                            .nth(offset)
                            .map_or(text.len(), |(i, _)| i);
                        let byte_end = text[byte_start..]
                            .char_indices()
                            .nth(n as usize)
                            .map_or(text.len(), |(i, _)| byte_start + i);
                        &text[byte_start..byte_end]
                    };
                    if local != imported {
                        let offset = local
                            .chars()
                            .zip(imported.chars())
                            .position(|(a, b)| a != b)
                            .unwrap_or(0);
                        return Err(LoroError::UsedOpID {
                            id: ID::new(change.id.peer, ctr + offset as Counter),
                        });
                    }
                    ctr += n;
                }
                Ok(())
            },
        )?;
        Ok(checked.then_some(block_end))
    }
}

/// The deps of the op at `ctr`. Inside a change every op depends on the previous
/// one, and changes only merge when that holds, so this does not depend on how
/// either side split its history into changes.
fn deps_at(change: &Change, ctr: Counter) -> Frontiers {
    if ctr == change.id.counter {
        change.deps.clone()
    } else {
        Frontiers::from_id(ID::new(change.id.peer, ctr - 1))
    }
}

/// Walk the ops of `a` and `b` over `[start, end)` and return the first counter at
/// which they differ. Each side may split or merge the ops differently, so both
/// are cut at every boundary of either side before comparing.
fn first_mismatch(
    a_arena: &SharedArena,
    b_arena: &SharedArena,
    values: ImportedValues,
    a: &Change,
    b: &Change,
    start: Counter,
    end: Counter,
) -> Option<Counter> {
    let mut a_ops = OpCursor::new(a, start);
    let mut b_ops = OpCursor::new(b, start);
    let mut ctr = start;
    while ctr < end {
        let (Some(a_op), Some(b_op)) = (a_ops.op_at(ctr), b_ops.op_at(ctr)) else {
            return Some(ctr);
        };
        let len = (end - ctr)
            .min(a_op.ctr_end() - ctr)
            .min(b_op.ctr_end() - ctr);
        let a_slice = slice_op(a_op, ctr, len);
        let b_slice = slice_op(b_op, ctr, len);
        if !op_eq(a_arena, b_arena, values, &a_slice, &b_slice) {
            // Error path only: find the exact atom.
            let offset = (0..len)
                .find(|&i| {
                    !op_eq(
                        a_arena,
                        b_arena,
                        values,
                        &slice_op(a_op, ctr + i, 1),
                        &slice_op(b_op, ctr + i, 1),
                    )
                })
                .unwrap_or(0);
            return Some(ctr + offset);
        }
        ctr += len;
    }

    None
}

struct OpCursor<'a> {
    ops: &'a [Op],
    index: usize,
}

impl<'a> OpCursor<'a> {
    fn new(change: &'a Change, start: Counter) -> Self {
        let ops = change.ops.vec().as_slice();
        let index = ops.partition_point(|op| op.ctr_end() <= start);
        Self { ops, index }
    }

    /// The op containing `ctr`. Counters only move forward.
    fn op_at(&mut self, ctr: Counter) -> Option<&'a Op> {
        while let Some(op) = self.ops.get(self.index) {
            if op.ctr_end() > ctr {
                return (op.counter <= ctr).then_some(op);
            }
            self.index += 1;
        }
        None
    }
}

fn slice_op(op: &Op, ctr: Counter, len: Counter) -> std::borrow::Cow<'_, Op> {
    let from = (ctr - op.counter) as usize;
    let to = from + len as usize;
    if from == 0 && to == op.atom_len() {
        std::borrow::Cow::Borrowed(op)
    } else {
        std::borrow::Cow::Owned(op.slice(from, to))
    }
}

/// Whether two ops with the same id range are the same op. Compares what the op
/// means, not how it is stored: arena offsets and the direction of a one-atom
/// delete are representation details.
fn op_eq(
    a_arena: &SharedArena,
    b_arena: &SharedArena,
    values: ImportedValues,
    a: &Op,
    b: &Op,
) -> bool {
    let same_container = if std::ptr::eq(a_arena, b_arena) {
        a.container == b.container
    } else {
        a_arena.idx_to_id(a.container) == b_arena.idx_to_id(b.container)
    };
    if !same_container || a.atom_len() != b.atom_len() {
        return false;
    }

    let exact = values == ImportedValues::Exact;

    match (&a.content, &b.content) {
        (InnerContent::List(a), InnerContent::List(b)) => list_op_eq(a_arena, b_arena, exact, a, b),
        (InnerContent::Map(a), InnerContent::Map(b)) => {
            a.key == b.key
                && a.value.is_some() == b.value.is_some()
                && (!exact || a.value == b.value)
        }
        (InnerContent::Tree(a), InnerContent::Tree(b)) => a == b,
        (InnerContent::Future(a), InnerContent::Future(b)) => match (a, b) {
            #[cfg(feature = "counter")]
            (FutureInnerContent::Counter(a), FutureInnerContent::Counter(b)) => {
                !exact || a == b || (a.is_nan() && b.is_nan())
            }
            (
                crate::op::FutureInnerContent::Unknown {
                    prop: a_prop,
                    value: a_value,
                },
                crate::op::FutureInnerContent::Unknown {
                    prop: b_prop,
                    value: b_value,
                },
            ) => a_prop == b_prop && (!exact || a_value == b_value),
            #[allow(unreachable_patterns)]
            _ => false,
        },
        _ => false,
    }
}

fn list_op_eq(
    a_arena: &SharedArena,
    b_arena: &SharedArena,
    exact: bool,
    a: &InnerListOp,
    b: &InnerListOp,
) -> bool {
    match (a, b) {
        (
            InnerListOp::Insert {
                slice: a_slice,
                pos: a_pos,
            },
            InnerListOp::Insert {
                slice: b_slice,
                pos: b_pos,
            },
        ) => {
            a_pos == b_pos
                && (!exact
                    || if std::ptr::eq(a_arena, b_arena) {
                        a_arena.value_slices_eq(a_slice.to_range(), b_slice.to_range())
                    } else {
                        a_arena.with_values(a_slice.to_range(), |a| {
                            b_arena.with_values(b_slice.to_range(), |b| a == b)
                        })
                    })
        }
        (
            InnerListOp::InsertText {
                slice: a_slice,
                unicode_len: a_len,
                pos: a_pos,
                ..
            },
            InnerListOp::InsertText {
                slice: b_slice,
                unicode_len: b_len,
                pos: b_pos,
                ..
            },
        ) => a_pos == b_pos && a_len == b_len && a_slice[..] == b_slice[..],
        (InnerListOp::Delete(a), InnerListOp::Delete(b)) => {
            // A one-atom delete has no direction: `signed_len` 1 and -1 delete the
            // same element, and which one is stored depends on how it was sliced.
            a.id_start == b.id_start
                && a.span.pos == b.span.pos
                && (a.span.signed_len == b.span.signed_len
                    || (a.span.signed_len.abs() == 1 && b.span.signed_len.abs() == 1))
        }
        (
            InnerListOp::Move {
                from: a_from,
                elem_id: a_elem,
                to: a_to,
            },
            InnerListOp::Move {
                from: b_from,
                elem_id: b_elem,
                to: b_to,
            },
        ) => a_from == b_from && a_elem == b_elem && a_to == b_to,
        (
            InnerListOp::Set {
                elem_id: a_elem,
                value: a_value,
            },
            InnerListOp::Set {
                elem_id: b_elem,
                value: b_value,
            },
        ) => a_elem == b_elem && (!exact || a_value == b_value),
        (
            InnerListOp::StyleStart {
                start: a_start,
                end: a_end,
                key: a_key,
                value: a_value,
                info: a_info,
            },
            InnerListOp::StyleStart {
                start: b_start,
                end: b_end,
                key: b_key,
                value: b_value,
                info: b_info,
            },
        ) => {
            a_start == b_start
                && a_end == b_end
                && a_key == b_key
                && (!exact || a_value == b_value)
                && a_info == b_info
        }
        (InnerListOp::StyleEnd, InnerListOp::StyleEnd) => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        container::list::list_op::ListOp,
        op::{ListSlice, RawOpContent},
    };
    use loro_common::{ContainerID, ContainerType};

    fn text_change(arena: &SharedArena, text: &str) -> Change {
        let op = arena.convert_single_op(
            &ContainerID::new_root("t", ContainerType::Text),
            1,
            0,
            0,
            RawOpContent::List(ListOp::Insert {
                slice: ListSlice::RawStr {
                    str: text.into(),
                    unicode_len: text.chars().count(),
                },
                pos: 0,
            }),
        );
        let mut ops = rle::RleVec::new();
        ops.push(op);
        Change {
            id: ID::new(1, 0),
            lamport: 0,
            timestamp: 0,
            commit_msg: None,
            deps: Frontiers::default(),
            ops,
        }
    }

    #[test]
    fn different_arenas_compare_container_ids_and_unicode_atoms() {
        let a = SharedArena::new();
        let b = SharedArena::new();
        b.register_container(&ContainerID::new_root("other", ContainerType::Map));
        b.alloc_str("unrelated");
        let left = text_change(&a, "a😀bc");
        let same = text_change(&b, "a😀bc");
        let different = text_change(&b, "a😀Bc");
        assert_eq!(
            first_mismatch(&a, &b, ImportedValues::Exact, &left, &same, 0, 4),
            None
        );
        assert_eq!(
            first_mismatch(&a, &b, ImportedValues::Exact, &left, &different, 1, 4),
            Some(2)
        );
    }

    #[test]
    fn cold_text_check_compares_dependencies_inside_a_merged_import() {
        use crate::{cursor::PosType, LoroDoc};
        use std::sync::{atomic::AtomicI64, Arc};
        let source = LoroDoc::new_auto_commit();
        source.set_peer_id(1).unwrap();
        source.set_change_merge_interval(-1);
        for i in 0..40 {
            source
                .get_text("t")
                .insert(i, "a", PosType::Unicode)
                .unwrap();
            source.commit_then_renew();
        }
        let oplog = source.oplog().lock();
        let forged = crate::oplog::ChangeStore::new_mem(&oplog.arena, Arc::new(AtomicI64::new(-1)));
        for i in 0..40 {
            let mut c = (*oplog.get_change_at(ID::new(1, i)).unwrap()).clone();
            if i == 1 {
                c.deps = Frontiers::default();
            }
            forged.insert_change(c, false, true);
        }
        let bytes = forged.encode_all(oplog.vv(), oplog.frontiers());
        let local = crate::OpLog::new(Default::default());
        local.change_store.import_all(bytes).unwrap();
        let arena = SharedArena::new();
        let incoming = text_change(&arena, &"a".repeat(40));
        let err = local
            .check_known_text_in_cold_block(&incoming, &arena, 0, 40)
            .unwrap_err();
        assert!(matches!(err, LoroError::UsedOpID { id } if id == ID::new(1, 1)));
    }
}

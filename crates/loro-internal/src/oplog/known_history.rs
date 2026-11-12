//! Imported changes that reuse op ids the doc already has.
//!
//! An import skips the part of each change the doc already has (by version vector)
//! and applies the rest on top of the local history. When two clients shared a
//! peer id, the "known" part is different history, and the rest is then applied to
//! a state it was not written against: it panicked inside state apply (poisoning
//! the doc) or silently scrambled it (loro-dev/loro#1118). See
//! `context/import-peer-id-reuse.md`.

use loro_common::{
    Counter, HasCounterSpan, HasIdSpan, LoroError, LoroResult, LoroValue, PeerID, ID,
};
use rle::{HasLength, Sliceable};
use rustc_hash::{FxHashMap, FxHashSet};

use super::pending_changes::ClaimedSpans;
use crate::{
    arena::SharedArena,
    change::Change,
    container::list::list_op::InnerListOp,
    dag::Dag,
    op::{InnerContent, Op},
    version::{Frontiers, ImVersionVector},
    OpLog,
};

#[cfg(feature = "counter")]
use crate::op::FutureInnerContent;

/// How to compare imported op values with known history.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ImportedValues {
    /// Structural equality. Default for binary import.
    Exact,
    /// Equivalences JSON export/import is known to introduce. Callers opt in.
    JsonLossy,
}

impl ImportedValues {
    /// Walks collections with a work list: a value comes from an untrusted peer and
    /// can be nested deeper than the stack allows, like in `LoroValue`'s own `eq`.
    /// See `context/deep-value-stack-safety.md`.
    fn eq<'a>(self, a: &'a LoroValue, b: &'a LoroValue) -> bool {
        if self == Self::Exact {
            return a == b;
        }

        let mut pending = vec![(a, b)];
        while let Some((a, b)) = pending.pop() {
            let equal = match (a, b) {
                (LoroValue::I64(a), LoroValue::I64(b)) => {
                    // JS numbers can round distinct large integers to the same f64.
                    // Keep exact comparison in the range where every integer fits.
                    const MAX_EXACT_INTEGER: i64 = 1 << 53;
                    a == b
                        || ((a.unsigned_abs() > MAX_EXACT_INTEGER as u64
                            || b.unsigned_abs() > MAX_EXACT_INTEGER as u64)
                            && *a as f64 == *b as f64)
                }
                (LoroValue::Double(d), LoroValue::I64(i))
                | (LoroValue::I64(i), LoroValue::Double(d)) => {
                    // Inside +/-2^53 this is exact numeric equality; outside it also
                    // accepts the rounded Double produced by a JS-number relay.
                    *d == *i as f64
                }
                (LoroValue::String(string), LoroValue::Container(_))
                | (LoroValue::Container(_), LoroValue::String(string)) => {
                    // A marker-looking string also becomes a container reference.
                    // JSON peer compression can reinterpret its peer as an index,
                    // so the decoded id cannot safely be compared with that string.
                    loro_common::ContainerID::try_from_loro_value_string(string).is_some()
                }
                (LoroValue::Binary(bytes), LoroValue::List(list))
                | (LoroValue::List(list), LoroValue::Binary(bytes)) => {
                    bytes.len() == list.len()
                        && bytes
                            .iter()
                            .zip(list.iter())
                            .all(|(&byte, value)| match value {
                                LoroValue::I64(number) => *number == i64::from(byte),
                                LoroValue::Double(number) => *number == f64::from(byte),
                                _ => false,
                            })
                }
                (LoroValue::Double(number), LoroValue::Null)
                | (LoroValue::Null, LoroValue::Double(number)) => !number.is_finite(),
                (LoroValue::List(a), LoroValue::List(b)) => {
                    pending.extend(a.iter().zip(b.iter()));
                    a.len() == b.len()
                }
                (LoroValue::Map(a), LoroValue::Map(b)) => {
                    a.len() == b.len()
                        && a.iter().all(|(key, a)| match b.get(key) {
                            Some(b) => {
                                pending.push((a, b));
                                true
                            }
                            None => false,
                        })
                }
                _ => a == b,
            };
            if !equal {
                return false;
            }
        }

        true
    }
}

impl OpLog {
    /// Check the decoded `changes` against the history this doc already has for the
    /// same op ids, then drop the parts it already has.
    ///
    /// Returns [`LoroError::UsedOpID`] with the first conflicting id when an imported
    /// change differs from the local one, from a parked change that holds the same
    /// ids, or from an earlier change of the same import (deps or op content), and
    /// [`LoroError::DecodeError`] when a change skips counters of its peer (see
    /// [`Self::check_changes_do_not_skip_counters`]) or depends on an op of its own
    /// peer other than the previous one (see [`Self::check_own_peer_deps`]). It reads
    /// the op log only, so a caller can return the error before mutating anything.
    ///
    /// The comparison also runs when the import brings nothing new. Such an import
    /// changes nothing either way, but returning `Ok` would tell the caller its
    /// ops are in this doc when others hold their ids. An import that repeats
    /// what the doc has passes the comparison, and costs O(overlap) on top of
    /// decoding it: see "Cost" in `context/import-peer-id-reuse.md`.
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
        for change in changes.iter() {
            let known_end = known_end(change);
            if change.id.counter < known_end {
                self.check_known_part_of_change(change, known_end, values, imported_arena)?;
            }
            if change.ctr_end() > known_end {
                self.check_parked_part_of_change(change, known_end, values, imported_arena)?;
            }
        }
        self.check_changes_agree_with_each_other(&changes, values, imported_arena)?;

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
        Self::check_own_peer_deps(&changes)?;
        self.check_changes_do_not_skip_counters(&changes)?;
        self.check_changes_include_predecessors(&changes)?;
        Ok(changes)
    }

    /// A change of peer `p` at counter `c` can depend on its own peer only through
    /// `p@c-1`: a local commit depends on the doc's frontiers, which hold at most the
    /// latest op of `p`. A crafted change of `p` whose dep is an older op of `p` gets
    /// its lamport from that dep, but the DAG merges it into the last node of `p`, and
    /// the lamports do not continue: `AppDag` failed the assertion "lamport is not
    /// continuous" under the op log lock, poisoning the doc. A dep on `p@c` or a later
    /// counter can never be met.
    fn check_own_peer_deps(changes: &[Change]) -> LoroResult<()> {
        for change in changes {
            let id = change.id;
            if let Some(dep) = change
                .deps
                .iter()
                .find(|dep| dep.peer == id.peer && dep.counter != id.counter - 1)
            {
                return Err(LoroError::DecodeError(
                    format!(
                        "Invalid change {id}: it depends on {dep}, an op of its own peer other than the previous one"
                    )
                    .into_boxed_str(),
                ));
            }
        }

        Ok(())
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

    /// Presence in the receiver is not proof of causality (#1239). Check the
    /// predecessor against the change's deps, including transitive cross-peer
    /// edges. Direct own-peer deps and head-frontier imports take allocation-free
    /// O(1) fast paths, and unapplied subgraphs memoize causal summaries so batch
    /// and out-of-order multi-peer imports remain near-linear.
    ///
    /// Unresolved edges may still cover the predecessor, so defer those checks.
    /// Follow the pending missing-dep index when new history arrives, checking
    /// affected pending changes before this import mutates anything as well.
    fn check_changes_include_predecessors(&self, changes: &[Change]) -> LoroResult<()> {
        let head_dominates = |deps: &Frontiers| {
            !self.dag.frontier().is_empty() && self.dag.frontier().iter().all(|f| deps.contains(&f))
        };
        if self.pending_changes.is_empty()
            && changes.iter().all(|change| {
                let id = change.id;
                id.counter == 0
                    || change.deps.iter().any(|dep| dep == id.inc(-1))
                    || (self.vv().includes_id(id.inc(-1)) && head_dominates(&change.deps))
            })
        {
            return Ok(());
        }

        let imported: std::cell::OnceCell<FxHashMap<PeerID, ClaimedSpans<&Change>>> =
            std::cell::OnceCell::new();
        let mut workspace = CausalWorkspace::default();
        let mut candidates: Vec<&Change> = changes.iter().collect();
        let mut visited = FxHashSet::default();
        let mut unlocked_from: FxHashMap<PeerID, Counter> = FxHashMap::default();
        let mut i = 0;
        while i < candidates.len() {
            let change = candidates[i];
            i += 1;
            if !visited.insert(change.id) {
                continue;
            }
            let id_last = change.id_last();
            let next_from = unlocked_from.entry(id_last.peer).or_insert(Counter::MIN);
            if id_last.counter >= *next_from {
                candidates.extend(self.pending_changes.waiting_on_range(
                    id_last.peer,
                    *next_from,
                    id_last.counter,
                ));
                *next_from = id_last.counter.saturating_add(1);
            }

            let id = change.id;
            let target = id.inc(-1);
            if id.counter == 0
                || change.deps.iter().any(|dep| dep == target)
                || (self.vv().includes_id(target) && head_dominates(&change.deps))
                || self.dag.try_import_deps_before_shallow_root(&change.deps)?
            {
                continue;
            }

            let imported_map = imported.get_or_init(|| {
                let mut index: FxHashMap<PeerID, ClaimedSpans<&Change>> = FxHashMap::default();
                for c in changes {
                    let spans = index.entry(c.id.peer).or_default();
                    let start = c.id.counter;
                    let end = c.ctr_end();
                    for (g_start, g_end) in spans.gaps(start, end) {
                        spans.hold(g_start, g_end, c);
                    }
                }
                index
            });

            if change.deps.iter().any(|dep| {
                !self.vv().includes_id(dep)
                    && find_unapplied_source(self, imported_map, dep)
                        .is_some_and(|src| src.deps.contains(&target))
            }) {
                continue;
            }

            let summary = workspace.summary_of(self, imported_map, change)?;
            if summary.unresolved || summary.unapplied_vv.includes_id(target) {
                continue;
            }

            let mut covered_or_unresolved = false;
            if self.vv().includes_id(target) && !summary.dag_deps.is_empty() {
                if summary.dag_deps.includes_id(target)
                    || (!self.dag.frontier().is_empty()
                        && self
                            .dag
                            .frontier()
                            .iter()
                            .all(|f| summary.dag_deps.includes_id(f)))
                {
                    covered_or_unresolved = true;
                } else {
                    for (&peer, &end_ctr) in summary.dag_deps.iter() {
                        if end_ctr <= 0 {
                            continue;
                        }
                        match self
                            .dag
                            .try_does_id_cover(ID::new(peer, end_ctr - 1), target)?
                        {
                            Some(true) | None => {
                                covered_or_unresolved = true;
                                break;
                            }
                            Some(false) => {}
                        }
                    }
                }
            }

            if !covered_or_unresolved {
                return Err(LoroError::DecodeError(format!(
                    "Invalid change {id}: its deps do not include {target} in their causal history, so it skips counters of its peer",
                ).into_boxed_str()));
            }
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

    /// Compare `change` from `known_end` on with the parked changes that hold the
    /// same ids. Applying either one later drops the other as already known, so this
    /// is the last point where the import that brought the conflict can be told, and
    /// nothing has changed yet. Parked history stays comparable through
    /// [`PendingChanges::claims_in`](super::pending_changes::PendingChanges::claims_in),
    /// which keeps this O(overlap) and free for a doc with nothing parked.
    fn check_parked_part_of_change(
        &self,
        change: &Change,
        known_end: Counter,
        values: ImportedValues,
        imported_arena: &SharedArena,
    ) -> LoroResult<()> {
        let peer = change.id.peer;
        let start = change.id.counter.max(known_end);
        let end = change.ctr_end();
        check_holders_of_change(
            change,
            start,
            end,
            self.pending_changes.claims_in(peer, start, end),
            imported_arena,
            &self.arena,
            values,
        )
    }

    /// Compare the changes of one import with each other, from the version vector on.
    /// Applying the first of two that disagree and parking the second would bring the
    /// second over the first once its dep arrives, which breaks the DAG under the op
    /// log lock. Only a forged import has this, and an exporter writes each peer's
    /// changes in counter order, so a pass over the counters clears everything else.
    fn check_changes_agree_with_each_other(
        &self,
        changes: &[Change],
        values: ImportedValues,
        imported_arena: &SharedArena,
    ) -> LoroResult<()> {
        // A sync loop delivers one change at a time: not worth a map per import.
        if changes.len() < 2 {
            return Ok(());
        }

        let mut ends: FxHashMap<PeerID, Counter> = FxHashMap::default();
        let in_order = changes.iter().all(|change| {
            let end = ends.entry(change.id.peer).or_default();
            let in_order = change.id.counter >= *end;
            *end = change.ctr_end();
            in_order
        });
        if in_order {
            return Ok(());
        }

        let vv = self.vv();
        let mut held: FxHashMap<PeerID, ClaimedSpans<&Change>> = FxHashMap::default();
        for change in changes {
            let peer = change.id.peer;
            let known_end = vv.get(&peer).copied().unwrap_or(0);
            let start = change.id.counter.max(known_end);
            let end = change.ctr_end();
            if start >= end {
                continue;
            }

            let spans = held.entry(peer).or_default();
            check_holders_of_change(
                change,
                start,
                end,
                spans.overlapping(start, end),
                imported_arena,
                imported_arena,
                values,
            )?;
            for (start, end) in spans.gaps(start, end) {
                spans.hold(start, end, change);
            }
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

/// Compare `change` over `start..end` with the changes that hold those ids.
fn check_holders_of_change<'a>(
    change: &Change,
    start: Counter,
    end: Counter,
    holders: impl Iterator<Item = (Counter, Counter, &'a Change)>,
    change_arena: &SharedArena,
    holder_arena: &SharedArena,
    values: ImportedValues,
) -> LoroResult<()> {
    for (claim_start, claim_end, holder) in holders {
        let ctr = claim_start.max(start);
        let seg_end = claim_end.min(end);
        let mismatch = if deps_at(change, ctr) != deps_at(holder, ctr) {
            Some(ctr)
        } else {
            first_mismatch(
                change_arena,
                holder_arena,
                values,
                change,
                holder,
                ctr,
                seg_end,
            )
        };
        if let Some(bad) = mismatch {
            return Err(LoroError::UsedOpID {
                id: ID::new(change.id.peer, bad),
            });
        }
    }

    Ok(())
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

    match (&a.content, &b.content) {
        (InnerContent::List(a), InnerContent::List(b)) => list_op_eq(values, a, b),
        (InnerContent::Map(a), InnerContent::Map(b)) => {
            a.key == b.key
                && match (&a.value, &b.value) {
                    (Some(a), Some(b)) => values.eq(a, b),
                    (None, None) => true,
                    _ => false,
                }
        }
        (InnerContent::Tree(a), InnerContent::Tree(b)) => a == b,
        (InnerContent::Future(a), InnerContent::Future(b)) => match (a, b) {
            #[cfg(feature = "counter")]
            (FutureInnerContent::Counter(a), FutureInnerContent::Counter(b)) => {
                a == b || (a.is_nan() && b.is_nan())
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
            ) => {
                // A newer container's payload may contain LoroValues or arena
                // indices whose meaning this version cannot interpret. JSON
                // imports skip that payload; binary imports compare it.
                a_prop == b_prop
                    && match values {
                        ImportedValues::JsonLossy => true,
                        ImportedValues::Exact => a_value == b_value,
                    }
            }
            #[allow(unreachable_patterns)]
            _ => false,
        },
        _ => false,
    }
}

fn list_op_eq(values: ImportedValues, a: &InnerListOp, b: &InnerListOp) -> bool {
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
            let (a_values, b_values) = (a_slice.as_slice(), b_slice.as_slice());
            a_pos == b_pos
                && a_values.len() == b_values.len()
                && a_values.iter().zip(b_values).all(|(a, b)| values.eq(a, b))
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
        ) => a_elem == b_elem && values.eq(a_value, b_value),
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
                && values.eq(a_value, b_value)
                && a_info == b_info
        }
        (InnerListOp::StyleEnd, InnerListOp::StyleEnd) => true,
        _ => false,
    }
}

#[derive(Clone, Default)]
struct CausalSummary {
    unapplied_vv: ImVersionVector,
    dag_deps: ImVersionVector,
    unresolved: bool,
}

impl CausalSummary {
    fn merge(&mut self, other: &Self) {
        merge_im_vv(&mut self.unapplied_vv, &other.unapplied_vv);
        merge_im_vv(&mut self.dag_deps, &other.dag_deps);
        self.unresolved |= other.unresolved;
    }
}

#[inline]
fn extend_im_vv_last_id(vv: &mut ImVersionVector, id: ID) {
    let end = id.counter.saturating_add(1).max(0);
    if end > 0 && vv.get(&id.peer).is_none_or(|&cur| cur < end) {
        vv.insert(id.peer, end);
    }
}

fn merge_im_vv(dst: &mut ImVersionVector, src: &ImVersionVector) {
    if dst.is_empty() {
        *dst = src.clone();
        return;
    }
    if src.is_empty() {
        return;
    }
    if dst.len() < src.len() {
        let mut merged = src.clone();
        merged.extend_to_include_vv(dst.iter());
        *dst = merged;
    } else {
        dst.extend_to_include_vv(src.iter());
    }
}

#[inline]
fn find_unapplied_source<'a>(
    oplog: &'a OpLog,
    imported: &'a FxHashMap<PeerID, ClaimedSpans<&'a Change>>,
    dep: ID,
) -> Option<&'a Change> {
    imported
        .get(&dep.peer)
        .and_then(|spans| {
            spans
                .overlapping(dep.counter, dep.counter + 1)
                .next()
                .map(|(_, _, c)| c)
        })
        .or_else(|| {
            oplog
                .pending_changes
                .claims_in(dep.peer, dep.counter, dep.counter + 1)
                .next()
                .map(|(_, _, c)| c)
        })
}

struct Frame<'a> {
    node: &'a Change,
    deps: smallvec::SmallVec<[ID; 2]>,
    dep_idx: usize,
    pending_child: Option<ID>,
    local: CausalSummary,
}

#[derive(Default)]
struct CausalWorkspace<'a> {
    memo: FxHashMap<ID, CausalSummary>,
    indices: FxHashMap<ID, u32>,
    lowlink: FxHashMap<ID, u32>,
    on_stack: FxHashSet<ID>,
    scc_stack: Vec<(ID, CausalSummary)>,
    call_stack: Vec<Frame<'a>>,
}

impl<'a> CausalWorkspace<'a> {
    fn summary_of(
        &mut self,
        oplog: &'a OpLog,
        imported: &'a FxHashMap<PeerID, ClaimedSpans<&'a Change>>,
        root: &'a Change,
    ) -> LoroResult<CausalSummary> {
        if let Some(cached) = self.memo.get(&root.id) {
            return Ok(cached.clone());
        }

        self.indices.clear();
        self.lowlink.clear();
        self.on_stack.clear();
        self.scc_stack.clear();
        self.call_stack.clear();

        let mut next_index: u32 = 1;
        self.indices.insert(root.id, 0);
        self.lowlink.insert(root.id, 0);
        self.on_stack.insert(root.id);
        self.scc_stack.push((root.id, CausalSummary::default()));
        self.call_stack.push(Frame {
            node: root,
            deps: root.deps.iter().collect(),
            dep_idx: 0,
            pending_child: None,
            local: CausalSummary::default(),
        });

        while let Some(frame) = self.call_stack.last_mut() {
            let node_id = frame.node.id;
            if let Some(child_id) = frame.pending_child.take() {
                let child_low = self.lowlink[&child_id];
                let cur_low = self.lowlink.get_mut(&node_id).unwrap();
                *cur_low = (*cur_low).min(child_low);
                if let Some(child_sum) = self.memo.get(&child_id) {
                    frame.local.merge(child_sum);
                }
            }

            if frame.dep_idx < frame.deps.len() {
                let dep = frame.deps[frame.dep_idx];
                frame.dep_idx += 1;

                if oplog.vv().includes_id(dep) {
                    if oplog.dag.is_trimmed_and_unresolved(dep) {
                        frame.local.unresolved = true;
                    } else {
                        extend_im_vv_last_id(&mut frame.local.dag_deps, dep);
                    }
                    continue;
                }

                let Some(child) = find_unapplied_source(oplog, imported, dep) else {
                    frame.local.unresolved = true;
                    continue;
                };

                extend_im_vv_last_id(&mut frame.local.unapplied_vv, dep);
                let child_id = child.id;
                if let Some(child_sum) = self.memo.get(&child_id) {
                    frame.local.merge(child_sum);
                } else if !self.indices.contains_key(&child_id) {
                    frame.pending_child = Some(child_id);
                    let idx = next_index;
                    next_index += 1;
                    self.indices.insert(child_id, idx);
                    self.lowlink.insert(child_id, idx);
                    self.on_stack.insert(child_id);
                    self.scc_stack.push((child_id, CausalSummary::default()));
                    self.call_stack.push(Frame {
                        node: child,
                        deps: child.deps.iter().collect(),
                        dep_idx: 0,
                        pending_child: None,
                        local: CausalSummary::default(),
                    });
                } else if self.on_stack.contains(&child_id) {
                    let child_idx = self.indices[&child_id];
                    let cur_low = self.lowlink.get_mut(&node_id).unwrap();
                    *cur_low = (*cur_low).min(child_idx);
                }
                continue;
            }

            let finished = self.call_stack.pop().unwrap();
            if self.lowlink[&node_id] == self.indices[&node_id] {
                if self.scc_stack.last().map(|(id, _)| *id) == Some(node_id) {
                    self.scc_stack.pop();
                    self.on_stack.remove(&node_id);
                    self.memo.insert(node_id, finished.local);
                } else {
                    return Err(LoroError::DecodeError(
                        format!("Invalid change {node_id}: cyclic causal dependency")
                            .into_boxed_str(),
                    ));
                }
            } else if let Some((_, slot)) = self
                .scc_stack
                .iter_mut()
                .rev()
                .find(|(id, _)| *id == node_id)
            {
                *slot = finished.local;
            }
        }

        Ok(self.memo[&root.id].clone())
    }
}

#[cfg(test)]
mod json_lossy_value_tests {
    use super::ImportedValues;
    use loro_common::LoroValue;

    fn assert_lossy_pair(a: &LoroValue, b: &LoroValue, equal: bool) {
        assert_eq!(ImportedValues::JsonLossy.eq(a, b), equal, "{a:?} vs {b:?}");
        assert_eq!(ImportedValues::JsonLossy.eq(b, a), equal, "{b:?} vs {a:?}");
    }

    #[test]
    fn byte_lists_must_match_every_byte() {
        let binary = LoroValue::from(vec![0u8, 1, 255]);
        for list in [
            LoroValue::from(vec![0, 1, 255]),
            LoroValue::from(vec![0.0, 1.0, 255.0]),
        ] {
            assert_lossy_pair(&binary, &list, true);
            assert!(!ImportedValues::Exact.eq(&binary, &list));
        }
        assert_lossy_pair(
            &LoroValue::from(Vec::<u8>::new()),
            &LoroValue::from(Vec::<i32>::new()),
            true,
        );
        for list in [
            LoroValue::from(vec![0, 1]),
            LoroValue::from(vec![0, 1, 255, 0]),
            LoroValue::from(vec![0, 1, 254]),
            LoroValue::from(vec![0, -1, 255]),
            LoroValue::from(vec![0, 1, 256]),
            LoroValue::from(vec![0.0, 1.5, 255.0]),
            LoroValue::from(vec![0.0, f64::NAN, 255.0]),
            LoroValue::from(vec![
                LoroValue::I64(0),
                LoroValue::Bool(true),
                LoroValue::I64(255),
            ]),
        ] {
            assert_lossy_pair(&binary, &list, false);
        }
        assert_lossy_pair(&binary, &LoroValue::from(vec![0u8, 1, 254]), false);
    }

    #[test]
    fn only_non_finite_numbers_match_null() {
        for number in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let number = LoroValue::Double(number);
            assert_lossy_pair(&number, &LoroValue::Null, true);
            assert!(!ImportedValues::Exact.eq(&number, &LoroValue::Null));
            assert!(ImportedValues::Exact.eq(&number, &number));
        }
        for number in [0.0, -0.0, 1.5, f64::MAX] {
            assert_lossy_pair(&LoroValue::Double(number), &LoroValue::Null, false);
        }
        assert_lossy_pair(&LoroValue::I64(0), &LoroValue::Null, false);
        assert_lossy_pair(
            &LoroValue::Double(f64::INFINITY),
            &LoroValue::Double(f64::NEG_INFINITY),
            false,
        );
    }

    #[test]
    fn nested_values_allow_only_json_representations() {
        let nested =
            |value| LoroValue::Map(vec![("key".into(), LoroValue::from(vec![value]))].into());
        let original = nested(LoroValue::from(vec![
            LoroValue::from(vec![1u8, 2]),
            LoroValue::Double(f64::NAN),
        ]));
        let json: LoroValue =
            serde_json::from_str(&serde_json::to_string(&original).unwrap()).unwrap();
        assert_lossy_pair(&original, &json, true);
        assert!(!ImportedValues::Exact.eq(&original, &json));
        assert_lossy_pair(
            &original,
            &nested(LoroValue::from(vec![
                LoroValue::from(vec![1, 3]),
                LoroValue::Null,
            ])),
            false,
        );
        assert_lossy_pair(
            &original,
            &LoroValue::Map(vec![("other".into(), original["key"].clone())].into()),
            false,
        );
        assert_lossy_pair(&original, &LoroValue::Map(Default::default()), false);
        assert_lossy_pair(
            &LoroValue::from(vec![1, 2]),
            &LoroValue::from(vec![1, 2, 3]),
            false,
        );
    }

    #[test]
    fn finite_doubles_one_ulp_apart_stay_different() {
        let a = 1.0_f64;
        let b = f64::from_bits(a.to_bits() + 1);
        let (a, b) = (LoroValue::Double(a), LoroValue::Double(b));
        assert_lossy_pair(&a, &b, false);
        assert!(!ImportedValues::Exact.eq(&a, &b));
    }

    #[test]
    fn ordinary_values_still_require_exact_equality() {
        let values = [
            LoroValue::Null,
            LoroValue::Bool(false),
            LoroValue::Bool(true),
            LoroValue::I64(1),
            LoroValue::I64(2),
            LoroValue::Double(1.5),
            LoroValue::from("one"),
            LoroValue::from("two"),
            LoroValue::from(loro_common::ContainerID::new_root(
                "a",
                loro_common::ContainerType::Map,
            )),
            LoroValue::from(loro_common::ContainerID::new_root(
                "b",
                loro_common::ContainerType::Map,
            )),
        ];
        for a in &values {
            for b in &values {
                assert_lossy_pair(a, b, a == b);
            }
        }
    }

    #[test]
    fn doubles_match_the_integer_as_f64() {
        for integer in [0, 2, -2, 9_007_199_254_740_991, i64::MIN, i64::MAX] {
            let double = LoroValue::Double(integer as f64);
            let integer = LoroValue::I64(integer);
            assert_lossy_pair(&double, &integer, true);
            assert!(!ImportedValues::Exact.eq(&double, &integer));
        }
        assert_lossy_pair(&LoroValue::Double(-0.0), &LoroValue::I64(0), true);
        for number in [2.5, 3.0, f64::NAN, f64::INFINITY] {
            assert_lossy_pair(&LoroValue::Double(number), &LoroValue::I64(2), false);
        }
    }

    #[test]
    fn large_integers_match_only_the_same_js_number() {
        const BOUND: i64 = 1 << 53;
        const LARGE: i64 = 1 << 60;
        for (original, rounded) in [
            (BOUND + 1, BOUND),
            (-BOUND - 1, -BOUND),
            (LARGE + 1, LARGE),
            (LARGE + 1, LARGE + 24), // JSON.stringify spells 2^60 as ...7000.
            (-LARGE - 1, -LARGE),
            (-LARGE - 1, -LARGE - 24),
            (i64::MAX, i64::MAX - 1),
            (i64::MIN + 1, i64::MIN),
        ] {
            let original = LoroValue::I64(original);
            for rounded in [LoroValue::I64(rounded), LoroValue::Double(rounded as f64)] {
                assert_lossy_pair(&original, &rounded, true);
                assert!(!ImportedValues::Exact.eq(&original, &rounded));
            }
        }
        for (a, b) in [
            (BOUND - 1, BOUND),
            (-BOUND + 1, -BOUND),
            (BOUND, BOUND + 2),
            (-BOUND, -BOUND - 2),
            (LARGE, LARGE + 256),
            (-LARGE, -LARGE - 256),
            (i64::MAX, i64::MAX - 1024),
            (i64::MIN, i64::MIN + 1024),
        ] {
            assert_lossy_pair(&LoroValue::I64(a), &LoroValue::I64(b), false);
            assert_lossy_pair(&LoroValue::I64(a), &LoroValue::Double(b as f64), false);
        }
    }

    #[test]
    fn container_marker_strings_are_ambiguous_after_peer_compression() {
        use loro_common::{ContainerID, ContainerType};
        let id = ContainerID::new_root("target", ContainerType::Text);
        let marker = LoroValue::from(id.to_loro_value_string());
        let container = LoroValue::Container(id.clone());
        assert_lossy_pair(&marker, &container, true);
        assert!(!ImportedValues::Exact.eq(&marker, &container));
        for string in ["plain text", "🦜:invalid"] {
            assert_lossy_pair(&LoroValue::from(string), &container, false);
        }
        assert_lossy_pair(
            &LoroValue::from("🦜:cid:0@0:Text"),
            &LoroValue::Container(ContainerID::new_normal(
                loro_common::ID::new(1, 0),
                ContainerType::Text,
            )),
            true,
        );
        assert_lossy_pair(&marker, &LoroValue::from("🦜:cid:root-other:Text"), false);
        assert_lossy_pair(
            &container,
            &LoroValue::Container(ContainerID::new_root("other", ContainerType::Text)),
            false,
        );
    }

    #[test]
    fn unknown_payload_bypass_still_compares_props_and_containers() {
        use crate::{
            arena::SharedArena,
            encoding::OwnedValue,
            op::{FutureInnerContent, InnerContent, Op},
        };
        use loro_common::{ContainerID, ContainerType};
        let arena = SharedArena::new();
        let container =
            arena.register_container(&ContainerID::new_root("future", ContainerType::Unknown(9)));
        let make_op = |prop, value| Op {
            counter: 0,
            container,
            content: InnerContent::Future(FutureInnerContent::Unknown {
                prop,
                value: Box::new(value),
            }),
        };
        let a = make_op(1, OwnedValue::LoroValue(LoroValue::Double(2.0)));
        let b = make_op(1, OwnedValue::LoroValue(LoroValue::I64(2)));
        assert!(super::op_eq(
            &arena,
            &arena,
            ImportedValues::JsonLossy,
            &a,
            &b
        ));
        assert!(!super::op_eq(&arena, &arena, ImportedValues::Exact, &a, &b));
        let different_prop = make_op(2, OwnedValue::LoroValue(LoroValue::I64(2)));
        assert!(!super::op_eq(
            &arena,
            &arena,
            ImportedValues::JsonLossy,
            &a,
            &different_prop
        ));
        let mut different_container = b;
        different_container.container =
            arena.register_container(&ContainerID::new_root("other", ContainerType::Unknown(9)));
        assert!(!super::op_eq(
            &arena,
            &arena,
            ImportedValues::JsonLossy,
            &a,
            &different_container
        ));
    }

    #[cfg(feature = "counter")]
    #[test]
    fn counter_values_keep_numeric_equality_in_lossy_mode() {
        use crate::{
            arena::SharedArena,
            op::{FutureInnerContent, InnerContent, Op},
        };
        use loro_common::{ContainerID, ContainerType};
        let arena = SharedArena::new();
        let container =
            arena.register_container(&ContainerID::new_root("counter", ContainerType::Counter));
        let make_op = |value| Op {
            counter: 0,
            container,
            content: InnerContent::Future(FutureInnerContent::Counter(value)),
        };
        for (a, b, equal) in [
            (2.0, 2.0, true),
            (2.0, 3.0, false),
            (-0.0, 0.0, true),
            (f64::NAN, f64::NAN, true),
            (f64::INFINITY, f64::INFINITY, true),
            (f64::NAN, f64::INFINITY, false),
            (1e-17, 0.0, false),
            (-1e-17, 0.0, false),
        ] {
            for values in [ImportedValues::JsonLossy, ImportedValues::Exact] {
                assert_eq!(
                    super::op_eq(&arena, &arena, values, &make_op(a), &make_op(b)),
                    equal
                );
            }
        }
    }
}

#[cfg(test)]
mod two_arena_tests {
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

use std::ops::ControlFlow;

use generic_btree::{
    rle::{HasLength as _, Sliceable},
    LeafIndex,
};
use loro_common::{Counter, CounterSpan, HasId, HasIdSpan, IdFull, IdSpan, Lamport, PeerID, ID};
use rle::HasLength as _;
use rustc_hash::FxHashMap;
use smallvec::SmallVec;
use tracing::instrument;

use crate::{cursor::AbsolutePosition, version::CausalVersion, VersionVector};

use self::{crdt_rope::CrdtRope, id_to_cursor::IdToCursor};

use super::{
    fugue_span::{FugueSpan, Status},
    richtext_state::RichtextStateChunk,
    RichtextChunk, StyleOp,
};

mod crdt_rope;
mod id_to_cursor;
pub(crate) use crdt_rope::CrdtRopeDelta;

pub(crate) type PeerSpanCoverage = FxHashMap<PeerID, CounterSpan>;

#[derive(Debug)]
pub(crate) struct Tracker {
    applied_vv: VersionVector,
    rope: CrdtRope,
    id_to_cursor: IdToCursor,
}

/// Tracks the version currently materialized in a richtext tracker.
///
/// This state intentionally lives outside [`Tracker`]. The diff calculators keep
/// it next to the tracker because the stable cross-round invariant is:
///
/// - after `calculate_diff(from, to)` finishes, the tracker is materialized at
///   the coverage-local projection of `from`;
/// - during replay, this value may temporarily move through causal versions;
/// - diff-status checkout to `to` must not change it.
///
/// Only peers that have ops in the container coverage need to be stored here.
/// Missing peers are treated as materialized at counter `0`.
///
/// The type deliberately owns the mutable version vector. Tracker checkout that
/// mutates the materialized version requires `&mut Self`, while diff-status
/// checkout only takes `&Self`, so callers cannot accidentally advance the
/// stable materialized version while marking the `to` diff.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct TrackerMaterializedVersion {
    vv: Box<VersionVector>,
}

impl TrackerMaterializedVersion {
    #[inline]
    pub(crate) fn as_vv(&self) -> &VersionVector {
        &self.vv
    }

    #[inline]
    pub(crate) fn includes_id(&self, id: ID) -> bool {
        self.vv.includes_id(id)
    }

    pub(crate) fn reset_to_version_projection(
        &mut self,
        target: &VersionVector,
        coverage: &PeerSpanCoverage,
    ) {
        self.vv.clear();
        for &peer in coverage.keys() {
            if let Some(&end) = target.get(&peer) {
                if end > 0 {
                    self.vv.insert(peer, end);
                }
            }
        }
    }

    pub(crate) fn checkout_to_version(
        &mut self,
        tracker: &mut Tracker,
        target: &VersionVector,
        coverage: &PeerSpanCoverage,
    ) {
        let spans = self.checkout_spans_to_version(target, coverage);
        self.checkout_peer_spans(tracker, &spans, Some(coverage));
    }

    /// Marks diff status at `target` without changing the stable materialized
    /// version. This is the second half of diff calculation: after checkout to
    /// `from`, mark which spans would change at `to`.
    pub(crate) fn checkout_diff_status_to_version(
        &self,
        tracker: &mut Tracker,
        target: &VersionVector,
        coverage: &PeerSpanCoverage,
    ) {
        let spans = self.checkout_spans_to_version(target, coverage);
        tracker.apply_peer_spans(&spans, true, Some(coverage));
    }

    pub(crate) fn checkout_to_causal(
        &mut self,
        tracker: &mut Tracker,
        target: CausalVersion<'_>,
        coverage: &PeerSpanCoverage,
    ) {
        let spans = self.checkout_spans_to_causal(target, coverage);
        self.checkout_peer_spans(tracker, &spans, Some(coverage));
    }

    #[cfg(test)]
    fn checkout_to_version_without_coverage(
        &mut self,
        tracker: &mut Tracker,
        target: &VersionVector,
    ) {
        let spans = self.checkout_spans_to_version_without_coverage(target);
        self.checkout_peer_spans(tracker, &spans, None);
    }

    #[cfg(test)]
    fn checkout_diff_status_to_version_without_coverage(
        &self,
        tracker: &mut Tracker,
        target: &VersionVector,
    ) {
        let spans = self.checkout_spans_to_version_without_coverage(target);
        tracker.apply_peer_spans(&spans, true, None);
    }

    #[cfg(test)]
    fn checkout_to_causal_without_coverage(
        &mut self,
        tracker: &mut Tracker,
        target: CausalVersion<'_>,
    ) {
        let spans = self.checkout_spans_to_causal_without_coverage(target);
        self.checkout_peer_spans(tracker, &spans, None);
    }

    #[cfg(test)]
    fn checkout_peer_spans_without_coverage(
        &mut self,
        tracker: &mut Tracker,
        spans: &[IdSpan],
    ) {
        self.checkout_peer_spans(tracker, spans, None);
    }

    fn checkout_peer_spans(
        &mut self,
        tracker: &mut Tracker,
        spans: &[IdSpan],
        coverage: Option<&PeerSpanCoverage>,
    ) {
        tracker.apply_peer_spans(spans, false, coverage);

        for &span in spans {
            if coverage.is_some_and(|coverage| !coverage.contains_key(&span.peer)) {
                continue;
            }

            if span.is_reversed() {
                self.vv.shrink_to_exclude(span);
            } else {
                self.vv.extend_to_include(span);
            }
        }
    }

    fn checkout_spans_to_version(
        &self,
        target: &VersionVector,
        coverage: &PeerSpanCoverage,
    ) -> SmallVec<[IdSpan; 4]> {
        let mut spans: SmallVec<[IdSpan; 4]> = SmallVec::new();
        self.push_retreat_spans_to_version(&mut spans, |peer| {
            target.get(&peer).copied().unwrap_or(0)
        });
        for &peer in coverage.keys() {
            let target_end = target.get(&peer).copied().unwrap_or(0);
            let current_end = self.vv.get(&peer).copied().unwrap_or(0);
            if target_end > current_end {
                spans.push(IdSpan::new(peer, current_end, target_end));
            }
        }

        spans
    }

    fn checkout_spans_to_causal(
        &self,
        target: CausalVersion<'_>,
        coverage: &PeerSpanCoverage,
    ) -> SmallVec<[IdSpan; 4]> {
        let mut spans: SmallVec<[IdSpan; 4]> = SmallVec::new();
        self.push_retreat_spans_to_version(&mut spans, |peer| target.end_for_peer(peer));
        for &peer in coverage.keys() {
            let target_end = target.end_for_peer(peer);
            let current_end = self.vv.get(&peer).copied().unwrap_or(0);
            if target_end > current_end {
                spans.push(IdSpan::new(peer, current_end, target_end));
            }
        }

        spans
    }

    #[cfg(test)]
    fn checkout_spans_to_version_without_coverage(
        &self,
        target: &VersionVector,
    ) -> SmallVec<[IdSpan; 4]> {
        let mut spans: SmallVec<[IdSpan; 4]> = SmallVec::new();
        spans.extend(self.vv.sub_iter(target).map(reversed_span));
        spans.extend(target.sub_iter(&self.vv));
        spans
    }

    #[cfg(test)]
    fn checkout_spans_to_causal_without_coverage(
        &self,
        target: CausalVersion<'_>,
    ) -> SmallVec<[IdSpan; 4]> {
        let mut spans: SmallVec<[IdSpan; 4]> = SmallVec::new();
        self.push_retreat_spans_to_version(&mut spans, |peer| target.end_for_peer(peer));

        for (&peer, &base_end) in target.base().iter() {
            let target_end = if peer == target.peer() {
                base_end.max(target.peer_end())
            } else {
                base_end
            };
            let current_end = self.vv.get(&peer).copied().unwrap_or(0);
            if target_end > current_end {
                spans.push(IdSpan::new(peer, current_end, target_end));
            }
        }

        if !target.base().contains_key(&target.peer()) {
            let target_end = target.peer_end();
            let current_end = self.vv.get(&target.peer()).copied().unwrap_or(0);
            if target_end > current_end {
                spans.push(IdSpan::new(target.peer(), current_end, target_end));
            }
        }

        spans
    }

    fn push_retreat_spans_to_version(
        &self,
        spans: &mut SmallVec<[IdSpan; 4]>,
        target_end_for_peer: impl Fn(PeerID) -> Counter,
    ) {
        for (&peer, &counter) in self.vv.iter() {
            let target_end = target_end_for_peer(peer);
            if counter > target_end {
                spans.push(reversed_span(IdSpan::new(peer, target_end, counter)));
            }
        }
    }

    fn extend_to_include_end_id(&mut self, id: ID) {
        self.vv.extend_to_include_end_id(id);
    }

    fn extend_to_include_last_id(&mut self, id: ID) {
        self.vv.extend_to_include_last_id(id);
    }

    #[cfg(debug_assertions)]
    pub(crate) fn debug_assert_matches_version_projection(
        &self,
        target: &VersionVector,
        coverage: &PeerSpanCoverage,
    ) {
        for &peer in coverage.keys() {
            let expected = target.get(&peer).copied().unwrap_or(0);
            let actual = self.vv.get(&peer).copied().unwrap_or(0);
            debug_assert_eq!(
                actual, expected,
                "tracker materialized version must match the stable from-version projection"
            );
        }

        for (&peer, &actual) in self.vv.iter() {
            debug_assert!(
                coverage.contains_key(&peer),
                "tracker materialized version should only contain covered peers"
            );
            let expected = target.get(&peer).copied().unwrap_or(0);
            debug_assert_eq!(
                actual, expected,
                "tracker materialized version contains a stale peer counter"
            );
        }
    }

    #[cfg(not(debug_assertions))]
    pub(crate) fn debug_assert_matches_version_projection(
        &self,
        _target: &VersionVector,
        _coverage: &PeerSpanCoverage,
    ) {
    }
}

impl Default for Tracker {
    fn default() -> Self {
        Self::new_with_unknown()
    }
}

pub(super) const UNKNOWN_PEER_ID: PeerID = u64::MAX;
impl Tracker {
    pub fn new_with_unknown() -> Self {
        let mut this = Self {
            rope: CrdtRope::new(),
            id_to_cursor: IdToCursor::default(),
            applied_vv: Default::default(),
        };

        let result = this.rope.tree.push(FugueSpan {
            content: RichtextChunk::new_unknown(u32::MAX / 4),
            id: IdFull::new(UNKNOWN_PEER_ID, 0, 0),
            real_id: None,
            status: Status::default(),
            diff_status: None,
            origin_left: None,
            origin_right: None,
        });
        this.id_to_cursor.insert_without_split(
            ID::new(UNKNOWN_PEER_ID, 0),
            id_to_cursor::Cursor::new_insert(result.leaf, u32::MAX as usize / 4),
        );
        this
    }

    #[allow(unused)]
    fn new() -> Self {
        Self {
            rope: CrdtRope::new(),
            id_to_cursor: IdToCursor::default(),
            applied_vv: Default::default(),
        }
    }

    pub(crate) fn new_from_state_chunks(
        chunks: &[RichtextStateChunk],
        _styles: &mut Vec<(StyleOp, usize)>,
    ) -> Option<Self> {
        let mut last_lamport = None;
        for chunk in chunks {
            let RichtextStateChunk::Text(text) = chunk else {
                return None;
            };
            let id = text.id_full();
            if last_lamport.is_some_and(|last| last > id.lamport) {
                return None;
            }
            last_lamport = Some(id.lamport);
        }

        let mut this = Self::new();
        let mut pos = 0;
        for chunk in chunks {
            let RichtextStateChunk::Text(text) = chunk else {
                unreachable!("style chunks are rejected before seeding richtext tracker")
            };
            let len = text.unicode_len() as usize;
            if len == 0 {
                continue;
            }

            this._insert(pos, RichtextChunk::new_unknown(len as u32), text.id_full());
            pos += len;
        }

        Some(this)
    }

    #[inline]
    pub fn all_vv(&self) -> &VersionVector {
        &self.applied_vv
    }

    pub(crate) fn insert(
        &mut self,
        materialized: &mut TrackerMaterializedVersion,
        mut op_id: IdFull,
        mut pos: usize,
        mut content: RichtextChunk,
    ) {
        // trace!(
        //     "TrackerInsert op_id = {:#?}, pos = {:#?}, content = {:#?}",
        //     op_id,
        //     &pos,
        //     &content
        // );
        // tracing::span!(tracing::Level::INFO, "TrackerInsert");
        if let ControlFlow::Break(_) =
            self.skip_applied(materialized, op_id.id(), content.len(), |applied_counter_end| {
                // the op is partially included, need to slice the content
                let start = (applied_counter_end - op_id.counter) as usize;
                op_id.lamport += (applied_counter_end - op_id.counter) as Lamport;
                op_id.counter = applied_counter_end;
                pos += start;
                content = content.slice(start..);
            })
        {
            return;
        }

        // {
        //     tracing::span!(tracing::Level::INFO, "before insert {} pos={}", op_id, pos);
        //     debug_log::debug_dbg!(&self);
        // }
        let end_id = op_id.inc(content.len() as Counter);
        self._insert(pos, content, op_id);
        materialized.extend_to_include_end_id(end_id.id());
    }

    fn _insert(&mut self, pos: usize, content: RichtextChunk, op_id: IdFull) {
        let result = self.rope.insert(
            pos,
            FugueSpan {
                content,
                id: op_id,
                real_id: if op_id.peer == UNKNOWN_PEER_ID {
                    None
                } else {
                    Some(op_id.id().try_into().unwrap())
                },
                status: Status::default(),
                diff_status: None,
                origin_left: None,
                origin_right: None,
            },
            |id| self.id_to_cursor.get_insert(id).unwrap(),
        );
        self.id_to_cursor.insert(
            op_id.id(),
            id_to_cursor::Cursor::new_insert(result.leaf, content.len()),
        );

        self.update_insert_by_split(&result.splitted.arr);

        let end_id = op_id.inc(content.len() as Counter);
        self.applied_vv.extend_to_include_end_id(end_id.id());
    }

    fn update_insert_by_split(&mut self, split: &[LeafIndex]) {
        match split.len() {
            0 => {}
            1 => {
                let new_leaf_idx = split[0];
                let leaf = self.rope.tree().get_elem(new_leaf_idx).unwrap();
                self.id_to_cursor
                    .update_insert(leaf.id_span(), new_leaf_idx);
            }
            _ => {
                let mut updates = Vec::with_capacity(split.len());
                for &new_leaf_idx in split {
                    let leaf = self.rope.tree().get_elem(new_leaf_idx).unwrap();
                    updates.push((leaf.id_span(), new_leaf_idx));
                }
                self.id_to_cursor.update_insert_batch(&mut updates);
            }
        }
    }

    /// Delete the element from pos..pos+len
    ///
    /// If `reverse` is true, the deletion happens from the end of the range to the start.
    /// So the first op is the one that deletes element at `pos+len-1`, the last op
    /// is the one that deletes element at `pos`.
    ///
    /// - op_id: the first op that perform the deletion
    /// - target_start_id: in the target deleted span, it's the first id of the span
    /// - pos: the start pos of the deletion in the target span
    /// - len: the length of the deletion span
    /// - reverse: if true, the kth op delete the last kth element of the span
    pub(crate) fn delete(
        &mut self,
        materialized: &mut TrackerMaterializedVersion,
        mut op_id: ID,
        mut target_start_id: ID,
        pos: usize,
        mut len: usize,
        reverse: bool,
    ) {
        if let ControlFlow::Break(_) =
            self.skip_applied(materialized, op_id, len, |applied_counter_end: i32| {
                // the op is partially included, need to slice the op
                let start = (applied_counter_end - op_id.counter) as usize;
                op_id.counter = applied_counter_end;
                if !reverse {
                    target_start_id = target_start_id.inc(start as i32);
                }
                // Okay, this looks pretty weird, but it's correct.
                // If it's reverse, we don't need to change the target_start_id, because target_start_id always pointing towards the
                // leftmost element of the span. After applying the initial part of the deletion, which starts from the right side,
                // the target_start_id will be still pointing towards the same leftmost element, thus no need to change.
                len -= start;
                // If reverse, don't need to change the pos, because it's deleting backwards.
                // If not reverse, we don't need to change the pos either, because the `start` chars after it are already deleted
            })
        {
            return;
        }

        // tracing::info!("after forwarding pos={} len={}", pos, len);

        let end_id = op_id.inc(len as Counter);
        self._delete(target_start_id, pos, len, reverse, op_id);
        materialized.extend_to_include_end_id(end_id);
    }

    fn _delete(&mut self, target_start_id: ID, pos: usize, len: usize, reverse: bool, op_id: ID) {
        let mut ans = Vec::new();
        let split = self
            .rope
            .delete(target_start_id, pos, len, reverse, &mut |span| {
                let mut id_span = span.id_span();
                if reverse {
                    id_span.reverse();
                }
                ans.push(id_span);
            });

        let mut cur_id = op_id;
        for id_span in ans {
            let len = id_span.atom_len();
            self.id_to_cursor
                .insert(cur_id, id_to_cursor::Cursor::Delete(id_span));
            cur_id = cur_id.inc(len as Counter);
        }

        debug_assert_eq!(cur_id.counter - op_id.counter, len as Counter);
        for s in split {
            self.update_insert_by_split(&s.arr);
        }

        let end_id = op_id.inc(len as Counter);
        self.applied_vv.extend_to_include_end_id(end_id);
    }

    fn skip_applied(
        &mut self,
        materialized: &mut TrackerMaterializedVersion,
        op_id: ID,
        len: usize,
        mut f: impl FnMut(Counter),
    ) -> ControlFlow<()> {
        let last_id = op_id.inc(len as Counter - 1);
        let applied_counter_end = self.applied_vv.get(&last_id.peer).copied().unwrap_or(0);
        if applied_counter_end > op_id.counter {
            if !materialized.includes_id(last_id) {
                // PERF: may be slow
                let mut updates = Default::default();
                let cnt_start = materialized.as_vv().get(&op_id.peer).copied().unwrap_or(0);
                self.forward(
                    IdSpan::new(op_id.peer, cnt_start, op_id.counter + len as Counter),
                    &mut updates,
                );
                self.batch_update(updates, false);
            }

            if applied_counter_end > last_id.counter {
                materialized.extend_to_include_last_id(last_id);
                return ControlFlow::Break(());
            }

            f(applied_counter_end);
        }
        ControlFlow::Continue(())
    }

    /// Internally it's delete at the src and insert at the dst.
    ///
    /// But it needs special behavior for id_to_cursor data structure
    ///
    #[instrument(skip(self))]
    pub(crate) fn move_item(
        &mut self,
        materialized: &mut TrackerMaterializedVersion,
        op_id: IdFull,
        deleted_id: ID,
        from_pos: usize,
        to_pos: usize,
    ) {
        if let ControlFlow::Break(_) =
            self.skip_applied(materialized, op_id.id(), 1, |_| unreachable!())
        {
            return;
        }

        // We record the **fake** id of the deleted item, and store it in the `id_to_cursor`.
        // This is because when we retreat, we need to know the **fake** id of the deleted item,
        // so that we can look up the insert pos in `id_to_cursor`
        //
        // > `id_to_cursor` only stores the mappings from **fake** insert id to the leaf index.
        // > **Fake** means the id may be a temporary placeholder, created with UNKNOWN_PEER_ID.
        let mut fake_delete_id = None;
        let split = self.rope.delete(deleted_id, from_pos, 1, false, &mut |s| {
            debug_assert_eq!(s.rle_len(), 1);
            fake_delete_id = Some(s.id.id());
        });

        for s in split {
            self.update_insert_by_split(&s.arr);
        }

        let result = self.rope.insert(
            to_pos,
            FugueSpan {
                // we need to use the special move content to avoid
                // its merging with other [FugueSpan], which will make
                // id_to_cursor need to track its split.
                // It would be much harder to implement correctly
                content: RichtextChunk::new_move(),
                id: op_id,
                real_id: if op_id.peer == UNKNOWN_PEER_ID {
                    None
                } else {
                    Some(op_id.id().try_into().unwrap())
                },
                status: Status::default(),
                diff_status: None,
                origin_left: None,
                origin_right: None,
            },
            |id| self.id_to_cursor.get_insert(id).unwrap(),
        );
        self.update_insert_by_split(&result.splitted.arr);

        self.id_to_cursor.insert(
            op_id.id(),
            id_to_cursor::Cursor::new_move(result.leaf, fake_delete_id.unwrap()),
        );

        let end_id = op_id.inc(1);
        self.applied_vv.extend_to_include_end_id(end_id.id());
        materialized.extend_to_include_end_id(end_id.id());
    }

    /// Checkout by applying directed peer spans.
    ///
    /// Forward spans use the normal `[start, end)` representation. Retreat spans
    /// must use `CounterSpan`'s reversed representation for the same covered ids.
    fn apply_peer_spans(
        &mut self,
        spans: &[IdSpan],
        on_diff_status: bool,
        coverage: Option<&PeerSpanCoverage>,
    ) {
        debug_assert_no_mixed_peer_directions(spans);
        if on_diff_status {
            self.rope.clear_diff_status();
        }

        let filtered_spans = filter_spans_by_coverage(spans, coverage);
        #[cfg(feature = "test_utils")]
        crate::diff_calc::profiling::record_richtext_tracker_span_filter(
            spans.len(),
            filtered_spans.len(),
        );
        let mut updates = Vec::new();
        for &span in filtered_spans.iter().filter(|span| span.is_reversed()) {
            for c in self.id_to_cursor.iter(span) {
                match c {
                    id_to_cursor::IterCursor::Insert { leaf, id_span } => {
                        updates.push(crdt_rope::LeafUpdate {
                            leaf,
                            id_span,
                            set_future: Some(true),
                            delete_times_diff: 0,
                        })
                    }
                    id_to_cursor::IterCursor::Delete(span) => {
                        for to_del in self.id_to_cursor.iter(span) {
                            match to_del {
                                id_to_cursor::IterCursor::Insert { leaf, id_span } => {
                                    updates.push(crdt_rope::LeafUpdate {
                                        leaf,
                                        id_span,
                                        set_future: None,
                                        delete_times_diff: -1,
                                    })
                                }
                                id_to_cursor::IterCursor::Move {
                                    from_id: _,
                                    to_leaf,
                                    new_op_id,
                                } => updates.push(crdt_rope::LeafUpdate {
                                    leaf: to_leaf,
                                    id_span: new_op_id.to_span(1),
                                    set_future: None,
                                    delete_times_diff: -1,
                                }),
                                _ => unreachable!(),
                            }
                        }
                    }
                    id_to_cursor::IterCursor::Move {
                        from_id: from,
                        to_leaf: to,
                        new_op_id: op_id,
                    } => {
                        let mut visited = false;
                        for to_del in self.id_to_cursor.iter(IdSpan::new(
                            from.peer,
                            from.counter,
                            from.counter + 1,
                        )) {
                            visited = true;

                            match to_del {
                                id_to_cursor::IterCursor::Move {
                                    from_id: _,
                                    to_leaf: to,
                                    new_op_id: op_id,
                                } => updates.push(crdt_rope::LeafUpdate {
                                    leaf: to,
                                    id_span: op_id.to_span(1),
                                    set_future: None,
                                    delete_times_diff: -1,
                                }),
                                // Un delete the from
                                id_to_cursor::IterCursor::Insert { leaf, id_span } => {
                                    debug_assert_eq!(id_span.atom_len(), 1);
                                    debug_assert_eq!(id_span.counter.start, from.counter);
                                    updates.push(crdt_rope::LeafUpdate {
                                        leaf,
                                        id_span,
                                        set_future: None,
                                        delete_times_diff: -1,
                                    })
                                }
                                _ => unreachable!(),
                            }
                        }
                        assert!(visited);
                        // insert the new
                        updates.push(crdt_rope::LeafUpdate {
                            leaf: to,
                            id_span: IdSpan::new(op_id.peer, op_id.counter, op_id.counter + 1),
                            set_future: Some(true),
                            delete_times_diff: 0,
                        });
                    }
                }
            }
        }

        for &span in filtered_spans.iter().filter(|span| !span.is_reversed()) {
            self.forward(span, &mut updates);
        }

        self.batch_update(updates, on_diff_status);
    }

    fn batch_update(&mut self, updates: Vec<crdt_rope::LeafUpdate>, on_diff_status: bool) {
        let leaf_indexes = self.rope.update(updates, on_diff_status);
        self.update_insert_by_split(&leaf_indexes);
    }

    fn forward(&mut self, span: loro_common::IdSpan, updates: &mut Vec<crdt_rope::LeafUpdate>) {
        for c in self.id_to_cursor.iter(span) {
            match c {
                id_to_cursor::IterCursor::Insert { leaf, id_span } => {
                    updates.push(crdt_rope::LeafUpdate {
                        leaf,
                        id_span,
                        set_future: Some(false),
                        delete_times_diff: 0,
                    })
                }
                id_to_cursor::IterCursor::Delete(span) => {
                    for to_del in self.id_to_cursor.iter(span) {
                        match to_del {
                            id_to_cursor::IterCursor::Insert { leaf, id_span } => {
                                updates.push(crdt_rope::LeafUpdate {
                                    leaf,
                                    id_span,
                                    set_future: None,
                                    delete_times_diff: 1,
                                })
                            }
                            id_to_cursor::IterCursor::Move {
                                from_id: _,
                                to_leaf,
                                new_op_id,
                            } => updates.push(crdt_rope::LeafUpdate {
                                leaf: to_leaf,
                                id_span: new_op_id.to_span(1),
                                set_future: None,
                                delete_times_diff: 1,
                            }),
                            _ => unreachable!(),
                        }
                    }
                }
                id_to_cursor::IterCursor::Move {
                    from_id: from,
                    to_leaf: to,
                    new_op_id: op_id,
                } => {
                    for to_del in self.id_to_cursor.iter(IdSpan::new(
                        from.peer,
                        from.counter,
                        from.counter + 1,
                    )) {
                        match to_del {
                            id_to_cursor::IterCursor::Move {
                                from_id: _,
                                to_leaf: to,
                                new_op_id: op_id,
                            } => updates.push(crdt_rope::LeafUpdate {
                                leaf: to,
                                id_span: op_id.to_span(1),
                                set_future: None,
                                delete_times_diff: 1,
                            }),
                            id_to_cursor::IterCursor::Insert { leaf, id_span } => {
                                updates.push(crdt_rope::LeafUpdate {
                                    leaf,
                                    id_span,
                                    set_future: None,
                                    delete_times_diff: 1,
                                })
                            }
                            _ => unreachable!(),
                        }
                    }

                    updates.push(crdt_rope::LeafUpdate {
                        leaf: to,
                        id_span: IdSpan::new(op_id.peer, op_id.counter, op_id.counter + 1),
                        set_future: Some(false),
                        delete_times_diff: 0,
                    });
                }
            }
        }
    }

    #[allow(unused)]
    pub(crate) fn check(&self, materialized: &TrackerMaterializedVersion) {
        if !cfg!(debug_assertions) {
            return;
        }

        self.check_vv_correctness(materialized);
        self.check_id_to_cursor_insertions_correctness();
    }

    fn check_vv_correctness(&self, materialized: &TrackerMaterializedVersion) {
        if !cfg!(debug_assertions) {
            return;
        }

        for span in self.rope.tree().iter() {
            if span.id.peer == UNKNOWN_PEER_ID {
                continue;
            }

            let id_span = span.id_span();
            assert!(self.all_vv().includes_id(id_span.id_last()));
            if span.status.future {
                assert!(!materialized.includes_id(id_span.id_start()));
            } else {
                assert!(materialized.includes_id(id_span.id_last()));
            }
        }
    }

    // It can only check the correctness of insertions in id_to_cursor.
    // The deletions are not checked.
    fn check_id_to_cursor_insertions_correctness(&self) {
        if !cfg!(debug_assertions) {
            return;
        }

        for rope_elem in self.rope.tree().iter() {
            let id_span = rope_elem.id_span();
            let leaf_from_start = self.id_to_cursor.get_insert(id_span.id_start()).unwrap();
            let leaf_from_last = self.id_to_cursor.get_insert(id_span.id_last()).unwrap();
            assert_eq!(leaf_from_start, leaf_from_last);
            let elem_from_id_to_cursor_map = self.rope.tree().get_elem(leaf_from_last).unwrap();
            assert_eq!(rope_elem, elem_from_id_to_cursor_map);
        }

        for content in self.id_to_cursor.iter_all() {
            match content {
                id_to_cursor::IterCursor::Insert { leaf, id_span } => {
                    let leaf = self.rope.tree().get_elem(leaf).unwrap();
                    let span = leaf.id_span();
                    span.contains(id_span.id_start());
                    span.contains(id_span.id_last());
                }
                id_to_cursor::IterCursor::Delete(_) => {}
                id_to_cursor::IterCursor::Move { .. } => {}
            }
        }
    }

    pub(crate) fn get_target_id_latest_index_at_new_version(
        &self,
        id: ID,
    ) -> Option<AbsolutePosition> {
        // TODO: PERF this can be sped up from O(n) to O(log(n)) but I'm not sure if it's worth it
        let mut index = 0;
        for span in self.rope.tree.iter() {
            let is_activated = span.is_activated_in_diff();
            let span_id = span.real_id();
            let id_span = span_id.to_span(span.rle_len());
            if id_span.contains(id) {
                if is_activated {
                    index += (id.counter - id_span.counter.start) as usize;
                }

                return Some(AbsolutePosition {
                    pos: index,
                    side: if is_activated {
                        crate::cursor::Side::Middle
                    } else {
                        crate::cursor::Side::Left
                    },
                });
            }

            if is_activated {
                index += span.rle_len();
            }
        }

        None
    }

    // #[tracing::instrument(skip(self), level = "info")]
    #[cfg(test)]
    pub(crate) fn diff(
        &mut self,
        materialized: &mut TrackerMaterializedVersion,
        from: &VersionVector,
        to: &VersionVector,
    ) -> impl Iterator<Item = CrdtRopeDelta> + '_ {
        // tracing::info!("Init: {:#?}, ", &self);
        materialized.checkout_to_version_without_coverage(self, from);
        materialized.checkout_diff_status_to_version_without_coverage(self, to);
        // self.id_to_cursor.diagnose();
        // tracing::trace!("Trace::diff {:#?}, ", &self);

        self.rope.get_diff()
    }

    pub(crate) fn diff_with_coverage(
        &mut self,
        materialized: &mut TrackerMaterializedVersion,
        from: &VersionVector,
        to: &VersionVector,
        coverage: &PeerSpanCoverage,
    ) -> impl Iterator<Item = CrdtRopeDelta> + '_ {
        materialized.checkout_to_version(self, from, coverage);
        materialized.checkout_diff_status_to_version(self, to, coverage);

        self.rope.get_diff()
    }
}

fn reversed_span(mut span: IdSpan) -> IdSpan {
    span.reverse();
    span
}

fn filter_spans_by_coverage(
    spans: &[IdSpan],
    coverage: Option<&PeerSpanCoverage>,
) -> SmallVec<[IdSpan; 4]> {
    match coverage {
        Some(coverage) => spans
            .iter()
            .filter_map(|span| intersect_span_with_coverage(*span, coverage))
            .collect(),
        None => spans.iter().copied().collect(),
    }
}

fn intersect_span_with_coverage(span: IdSpan, coverage: &PeerSpanCoverage) -> Option<IdSpan> {
    let coverage = coverage.get(&span.peer)?;
    let start = span.counter.min().max(coverage.min());
    let end = span.counter.norm_end().min(coverage.norm_end());
    if start >= end {
        return None;
    }

    let mut ans = IdSpan::new(span.peer, start, end);
    if span.is_reversed() {
        ans.reverse();
    }
    Some(ans)
}

#[cfg(debug_assertions)]
fn debug_assert_no_mixed_peer_directions(spans: &[IdSpan]) {
    for (index, span) in spans.iter().enumerate() {
        for other in &spans[index + 1..] {
            if span.peer == other.peer {
                debug_assert_eq!(span.is_reversed(), other.is_reversed());
            }
        }
    }
}

#[cfg(not(debug_assertions))]
fn debug_assert_no_mixed_peer_directions(_spans: &[IdSpan]) {}

#[cfg(test)]
mod test {
    use crate::{
        container::richtext::RichtextChunk,
        version::{CausalVersion, ImVersionVector},
        vv,
    };
    use generic_btree::rle::HasLength;

    use super::*;
    use std::time::Instant;

    fn tracker() -> (Tracker, TrackerMaterializedVersion) {
        (Tracker::new(), TrackerMaterializedVersion::default())
    }

    fn insert_text(
        tracker: &mut Tracker,
        materialized: &mut TrackerMaterializedVersion,
        id: IdFull,
        pos: usize,
        text: std::ops::Range<u32>,
    ) {
        tracker.insert(materialized, id, pos, RichtextChunk::new_text(text));
    }

    fn delete_text(
        tracker: &mut Tracker,
        materialized: &mut TrackerMaterializedVersion,
        op_id: ID,
        target_start_id: ID,
        pos: usize,
        len: usize,
        reverse: bool,
    ) {
        tracker.delete(materialized, op_id, target_start_id, pos, len, reverse);
    }

    #[test]
    fn test_len() {
        let (mut t, mut materialized) = tracker();
        insert_text(&mut t, &mut materialized, IdFull::new(1, 0, 0), 0, 0..2);
        assert_eq!(t.rope.len(), 2);
        materialized.checkout_to_version_without_coverage(&mut t, &Default::default());
        assert_eq!(t.rope.len(), 0);
        insert_text(&mut t, &mut materialized, IdFull::new(2, 0, 0), 0, 2..4);
        let v = vv!(1 => 2, 2 => 2);
        materialized.checkout_to_version_without_coverage(&mut t, &v);
        assert_eq!(&t.applied_vv, &v);
        assert_eq!(t.rope.len(), 4);
    }

    #[test]
    fn checkout_causal_single_frontier_retreats_other_peers() {
        let (mut t, mut materialized) = tracker();
        insert_text(&mut t, &mut materialized, IdFull::new(2, 0, 0), 0, 0..2);
        insert_text(&mut t, &mut materialized, IdFull::new(1, 0, 0), 2, 2..4);
        assert_eq!(t.rope.len(), 4);

        let base = ImVersionVector::new();
        materialized.checkout_to_causal_without_coverage(
            &mut t,
            CausalVersion::new(&base, 1, 2),
        );

        assert_eq!(t.rope.len(), 2);
        assert_eq!(materialized.as_vv(), &vv!(1 => 2));
    }

    #[test]
    fn checkout_peer_spans_uses_reversed_span_boundaries() {
        let (mut t, mut materialized) = tracker();
        insert_text(&mut t, &mut materialized, IdFull::new(1, 0, 0), 0, 0..4);
        insert_text(&mut t, &mut materialized, IdFull::new(2, 0, 4), 4, 4..6);
        assert_eq!(t.rope.len(), 6);
        assert_eq!(materialized.as_vv(), &vv!(1 => 4, 2 => 2));

        let retreat_peer_2 = reversed_span(IdSpan::new(2, 0, 2));
        materialized.checkout_peer_spans_without_coverage(&mut t, &[retreat_peer_2]);

        assert_eq!(t.rope.len(), 4);
        assert_eq!(materialized.as_vv(), &vv!(1 => 4));

        materialized.checkout_peer_spans_without_coverage(&mut t, &[IdSpan::new(2, 0, 2)]);

        assert_eq!(t.rope.len(), 6);
        assert_eq!(materialized.as_vv(), &vv!(1 => 4, 2 => 2));
    }

    #[test]
    fn span_coverage_intersection_preserves_direction() {
        let mut coverage = PeerSpanCoverage::default();
        coverage.insert(1, CounterSpan::new(3, 6));

        assert_eq!(
            intersect_span_with_coverage(IdSpan::new(1, 0, 10), &coverage),
            Some(IdSpan::new(1, 3, 6))
        );

        let reversed = reversed_span(IdSpan::new(1, 0, 10));
        let expected = reversed_span(IdSpan::new(1, 3, 6));
        assert_eq!(
            intersect_span_with_coverage(reversed, &coverage),
            Some(expected)
        );
    }

    #[test]
    fn coverage_filtered_checkout_keeps_materialized_projection_local() {
        let (mut t, mut materialized) = tracker();
        insert_text(&mut t, &mut materialized, IdFull::new(1, 0, 0), 0, 0..4);
        assert_eq!(materialized.as_vv(), &vv!(1 => 4));

        let mut coverage = PeerSpanCoverage::default();
        coverage.insert(1, CounterSpan::new(0, 4));
        materialized.checkout_peer_spans(
            &mut t,
            &[reversed_span(IdSpan::new(2, 0, 5))],
            Some(&coverage),
        );

        assert_eq!(t.rope.len(), 4);
        assert_eq!(materialized.as_vv(), &vv!(1 => 4));
    }

    #[test]
    fn coverage_filtered_diff_matches_unfiltered_for_delete_span() {
        fn tracker_with_delete() -> (Tracker, TrackerMaterializedVersion) {
            let (mut t, mut materialized) = tracker();
            insert_text(&mut t, &mut materialized, IdFull::new(1, 0, 0), 0, 0..10);
            delete_text(
                &mut t,
                &mut materialized,
                ID::new(2, 0),
                ID::NONE_ID,
                0,
                10,
                true,
            );
            (t, materialized)
        }

        let from = vv!(1 => 10);
        let to = vv!(1 => 10, 2 => 10);
        let (mut unfiltered, mut unfiltered_materialized) = tracker_with_delete();
        let (mut filtered, mut filtered_materialized) = tracker_with_delete();

        let mut coverage = PeerSpanCoverage::default();
        coverage.insert(1, CounterSpan::new(0, 10));
        coverage.insert(2, CounterSpan::new(0, 10));

        let unfiltered_delta = unfiltered
            .diff(&mut unfiltered_materialized, &from, &to)
            .collect::<Vec<_>>();
        let filtered_delta = filtered
            .diff_with_coverage(&mut filtered_materialized, &from, &to, &coverage)
            .collect::<Vec<_>>();

        assert_eq!(filtered_delta, unfiltered_delta);
        assert_eq!(filtered_materialized, unfiltered_materialized);
        assert_eq!(filtered.rope.len(), unfiltered.rope.len());
    }

    #[test]
    fn diff_status_checkout_preserves_stable_materialized_version() {
        let (mut t, mut materialized) = tracker();
        insert_text(&mut t, &mut materialized, IdFull::new(1, 0, 0), 0, 0..2);
        insert_text(&mut t, &mut materialized, IdFull::new(2, 0, 2), 2, 2..4);

        let mut coverage = PeerSpanCoverage::default();
        coverage.insert(1, CounterSpan::new(0, 2));
        coverage.insert(2, CounterSpan::new(0, 2));
        let from = vv!(1 => 2);
        let to = vv!(1 => 2, 2 => 2);

        materialized.checkout_to_version(&mut t, &from, &coverage);
        let stable_from = materialized.clone();
        materialized.checkout_diff_status_to_version(&mut t, &to, &coverage);

        assert_eq!(materialized, stable_from);
        materialized.debug_assert_matches_version_projection(&from, &coverage);
    }

    #[test]
    fn test_retreat_and_forward_delete() {
        let (mut t, mut materialized) = tracker();
        insert_text(&mut t, &mut materialized, IdFull::new(1, 0, 0), 0, 0..10);
        delete_text(&mut t, &mut materialized, ID::new(2, 0), ID::NONE_ID, 0, 10, true);
        materialized.checkout_to_version_without_coverage(&mut t, &vv!(1 => 10, 2=>5));
        assert_eq!(t.rope.len(), 5);
        materialized.checkout_to_version_without_coverage(&mut t, &vv!(1 => 10, 2=>0));
        assert_eq!(t.rope.len(), 10);
        materialized.checkout_to_version_without_coverage(&mut t, &vv!(1 => 10, 2=>10));
        assert_eq!(t.rope.len(), 0);
        materialized.checkout_to_version_without_coverage(&mut t, &vv!(1 => 10, 2=>0));
        assert_eq!(t.rope.len(), 10);
    }

    #[test]
    fn repeated_tail_splits_keep_id_to_cursor_consistent() {
        let (mut t, mut materialized) = tracker();
        insert_text(&mut t, &mut materialized, IdFull::new(1, 0, 0), 0, 0..300);

        for (i, pos) in [100, 201, 252, 278].into_iter().enumerate() {
            let op_id = IdFull::new(2, i as Counter, i as Lamport);
            let start = 1000 + i as u32;
            insert_text(&mut t, &mut materialized, op_id, pos, start..start + 1);
        }

        t.check(&materialized);
    }

    #[test]
    fn test_checkout_in_doc_with_del_span() {
        let (mut t, mut materialized) = tracker();
        insert_text(&mut t, &mut materialized, IdFull::new(1, 0, 0), 0, 0..10);
        delete_text(&mut t, &mut materialized, ID::new(2, 0), ID::NONE_ID, 0, 10, false);
        materialized.checkout_to_version_without_coverage(&mut t, &vv!(1 => 10, 2=>4));
        let v: Vec<FugueSpan> = t.rope.tree().iter().copied().collect();
        assert_eq!(v.len(), 2);
        assert!(!v[0].is_activated());
        assert_eq!(v[0].rle_len(), 4);
        assert!(v[1].is_activated());
        assert_eq!(v[1].rle_len(), 6);
    }

    #[test]
    #[ignore]
    fn perf_update_insert_by_split_quadratic() {
        // Run with:
        // cargo test -p loro-internal perf_update_insert_by_split_quadratic -- --ignored --nocapture
        const CHUNK_LEN: usize = 256;
        let fragments: usize = std::env::var("LORO_PERF_FRAGMENTS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(8192);
        const PEER_A: PeerID = 1;
        const PEER_B: PeerID = 2;

        let doc_len = CHUNK_LEN * fragments;

        let (mut t, mut materialized) = tracker();
        t.insert(
            &mut materialized,
            IdFull::new(PEER_A, 0, 0),
            0,
            RichtextChunk::new_text(0..doc_len as u32),
        );
        t.id_to_cursor.diagnose();

        let start = Instant::now();
        let expected_fragment_updates = (fragments as u64) * ((fragments - 1) as u64) / 2;

        for i in 0..(fragments - 1) {
            let pos = (i + 1) * CHUNK_LEN + i;
            let op_id = IdFull::new(PEER_B, i as Counter, i as Lamport);
            let chunk = RichtextChunk::new_text(
                (doc_len as u32 + i as u32)..(doc_len as u32 + i as u32 + 1),
            );
            t.insert(&mut materialized, op_id, pos, chunk);
        }

        let elapsed = start.elapsed();
        let before_vv = vv!(PEER_A => doc_len as Counter);
        let after_vv = vv!(PEER_A => doc_len as Counter, PEER_B => (fragments - 1) as Counter);
        let diff_start = Instant::now();
        let diff_len = t.diff(&mut materialized, &before_vv, &after_vv).count();
        let diff_elapsed = diff_start.elapsed();
        assert_eq!(t.rope.tree().iter().count(), 1 + 2 * (fragments - 1));
        println!(
            "perf_update_insert_by_split_quadratic: doc_len={}, fragments={}, expected_fragment_updates={}, insert_elapsed={:?}, diff_items={}, diff_elapsed={:?}",
            doc_len, fragments, expected_fragment_updates, elapsed, diff_len, diff_elapsed
        );
    }

    #[test]
    #[ignore]
    fn perf_update_insert_by_split_quadratic_unknown() {
        // Run with:
        // LORO_PERF_FRAGMENTS=8192 cargo test -p loro-internal perf_update_insert_by_split_quadratic_unknown -- --ignored --nocapture
        const CHUNK_LEN: usize = 256;
        let fragments: usize = std::env::var("LORO_PERF_FRAGMENTS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(8192);
        const PEER_B: PeerID = 2;

        let doc_len = CHUNK_LEN * fragments;

        let mut t = Tracker::new_with_unknown();
        let mut materialized = TrackerMaterializedVersion::default();
        materialized.checkout_to_version_without_coverage(&mut t, &VersionVector::new());
        t.id_to_cursor.diagnose();

        let start = Instant::now();
        for i in 0..(fragments - 1) {
            let pos = (i + 1) * CHUNK_LEN + i;
            let op_id = IdFull::new(PEER_B, i as Counter, i as Lamport);
            let chunk = RichtextChunk::new_text(
                (doc_len as u32 + i as u32)..(doc_len as u32 + i as u32 + 1),
            );
            t.insert(&mut materialized, op_id, pos, chunk);
        }

        let elapsed = start.elapsed();
        println!(
            "perf_update_insert_by_split_quadratic_unknown: doc_len={}, fragments={}, elapsed={:?}",
            doc_len, fragments, elapsed
        );
    }
}

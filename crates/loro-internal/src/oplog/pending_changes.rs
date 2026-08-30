use std::{
    collections::{hash_map::Entry, BTreeMap},
    ops::Deref,
};

use crate::{
    change::Change,
    version::{ImVersionVector, VersionRange},
    OpLog, VersionVector,
};
use loro_common::{
    ContainerType, Counter, CounterSpan, HasCounterSpan, HasIdSpan, IdSpan, PeerID, ID,
};
use rustc_hash::FxHashMap;

#[derive(Debug, Clone)]
pub enum PendingChange {
    // The lamport of the change decoded by `enhanced` is unknown.
    // we need calculate it when the change can be applied
    Unknown(Change),
    // TODO: Refactor, remove this?
    #[allow(unused)]
    Known(Change),
}

impl Deref for PendingChange {
    type Target = Change;
    fn deref(&self) -> &Self::Target {
        match self {
            Self::Unknown(a) => a,
            Self::Known(a) => a,
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct PendingChanges {
    changes: FxHashMap<PeerID, BTreeMap<Counter, Vec<PendingChange>>>,
}

impl PendingChanges {
    pub(crate) fn has_state_apply_rollback_ops(&self) -> bool {
        self.changes.values().any(|tree| {
            tree.values().any(|changes| {
                changes.iter().any(|change| {
                    change.ops.iter().any(|op| {
                        matches!(
                            op.container.get_type(),
                            ContainerType::List | ContainerType::Tree
                        )
                    })
                })
            })
        })
    }

    /// Number of changes parked under `[peer][counter]`, or `None` if nothing is.
    fn slot_len(&self, peer: PeerID, counter: Counter) -> Option<usize> {
        Some(self.changes.get(&peer)?.get(&counter)?.len())
    }

    pub(crate) fn version_range(&self) -> VersionRange {
        let mut range = VersionRange::default();
        for tree in self.changes.values() {
            for pending_changes in tree.values() {
                for pending_change in pending_changes {
                    range.extends_to_include_id_span(pending_change.id_span());
                }
            }
        }

        range
    }
}

#[cfg(test)]
#[allow(dead_code)]
impl PendingChanges {
    pub(crate) fn len(&self) -> usize {
        self.changes
            .values()
            .map(|tree| tree.values().map(Vec::len).sum::<usize>())
            .sum()
    }
}

/// What every `PendingChanges` slot this import-rollback scope touched held when the
/// scope began, recorded on first touch and restored wholesale on rollback.
///
/// Snapshot-per-slot rather than a mutation log, for two reasons.
///
/// Correctness: one scope can park a change and later unlock it — `import_batch`
/// opens a single scope across the whole batch, where later blobs routinely unlock
/// what earlier blobs parked. Undoing individual mutations then depends on getting
/// their relative order right, while restoring each slot to its pre-scope content is
/// order-independent by construction.
///
/// Size: a slot created inside the scope only needs [`PendingSlot::Absent`], and a
/// slot merely appended to only needs its original length. Cloning is limited to
/// content that predates the scope, which is exactly what rollback cannot otherwise
/// reconstruct — an out-of-order batch parks everything it later unlocks, so it
/// clones nothing.
#[derive(Debug, Default)]
pub(crate) struct PendingChangesRollback {
    slots: FxHashMap<(PeerID, Counter), PendingSlot>,
}

#[derive(Debug)]
enum PendingSlot {
    /// The slot did not exist when the scope began; rollback drops it.
    Absent,
    /// The slot held this many changes when the scope began and has only been
    /// appended to since; rollback truncates it back.
    Truncate(usize),
    /// What the slot held when the scope began, kept because the slot was removed
    /// wholesale and cannot be recovered from the live map.
    Restore(Vec<PendingChange>),
}

impl PendingChangesRollback {
    /// Record the slot before a change is pushed onto `[id.peer][id.counter]`.
    /// `pre_len` is the slot's current length, or `None` if it does not exist yet.
    fn record_added(&mut self, id: ID, pre_len: Option<usize>) {
        self.slots
            .entry((id.peer, id.counter))
            .or_insert(match pre_len {
                Some(len) => PendingSlot::Truncate(len),
                None => PendingSlot::Absent,
            });
    }

    /// Record the slot while `[peer][counter]` is removed wholesale.
    fn record_removed(&mut self, peer: PeerID, counter: Counter, removed: &[PendingChange]) {
        match self.slots.entry((peer, counter)) {
            Entry::Vacant(e) => {
                e.insert(PendingSlot::Restore(removed.to_vec()));
            }
            Entry::Occupied(mut e) => {
                // `Absent` stays absent (nothing here predates the scope) and
                // `Restore` already holds the pre-scope content. Only `Truncate`
                // has to materialize, and just the prefix it was pointing at:
                // everything past it was pushed inside the scope, so rollback
                // drops it either way.
                if let PendingSlot::Truncate(pre_len) = *e.get() {
                    debug_assert!(pre_len <= removed.len(), "slot shrank inside the scope");
                    e.insert(PendingSlot::Restore(removed[..pre_len].to_vec()));
                }
            }
        }
    }

    pub(crate) fn rollback(self, pending_changes: &mut PendingChanges) {
        for ((peer, counter), slot) in self.slots {
            match slot {
                PendingSlot::Absent => {
                    if let Some(tree) = pending_changes.changes.get_mut(&peer) {
                        tree.remove(&counter);
                    }
                }
                PendingSlot::Truncate(pre_len) => {
                    // The slot was never removed (that would have upgraded it to
                    // `Restore`), so it is still here with `pre_len` or more entries.
                    if let Some(changes) = pending_changes
                        .changes
                        .get_mut(&peer)
                        .and_then(|tree| tree.get_mut(&counter))
                    {
                        changes.truncate(pre_len);
                    }
                }
                PendingSlot::Restore(changes) => {
                    debug_assert!(!changes.is_empty(), "empty slots are never stored");
                    pending_changes
                        .changes
                        .entry(peer)
                        .or_default()
                        .insert(counter, changes);
                }
            }
        }

        pending_changes.changes.retain(|_, tree| !tree.is_empty());
    }
}

impl OpLog {
    fn push_pending_change(&mut self, missing_dep: ID, change: PendingChange) {
        let Self {
            pending_changes,
            import_rollback,
            ..
        } = self;
        if let Some(rollback) = import_rollback.as_mut() {
            let pre_len = pending_changes.slot_len(missing_dep.peer, missing_dep.counter);
            rollback.pending.record_added(missing_dep, pre_len);
        }

        pending_changes
            .changes
            .entry(missing_dep.peer)
            .or_default()
            .entry(missing_dep.counter)
            .or_default()
            .push(change);
    }

    /// Store or apply changes that could not get a lamport during the main import pass.
    ///
    /// These changes were deferred because some deps were missing from the DAG when
    /// first seen. By the time this runs, `try_apply_pending` may already have unlocked
    /// those deps (e.g. applying peer B unlocked previously pending peer A ops that a
    /// later B change depended on). Treat that as normal: apply when possible, skip if
    /// already present, and only park changes that are still waiting on a missing dep.
    ///
    /// Returns the version range of changes from `remote_changes` that remain pending.
    pub(super) fn extend_pending_changes_with_unknown_lamport(
        &mut self,
        remote_changes: Vec<Change>,
        mut would_affect: Option<&mut VersionRange>,
    ) -> VersionRange {
        let mut parked = Vec::new();
        let mut newly_applied_ids = Vec::new();

        for change in remote_changes {
            let local_change = PendingChange::Unknown(change);
            match remote_change_apply_state(self.vv(), self.shallow_since_vv(), &local_change) {
                ChangeState::AwaitingMissingDependency(miss_dep) => {
                    parked.push(local_change.id_span());
                    self.push_pending_change(miss_dep, local_change);
                }
                ChangeState::Applied => {}
                ChangeState::CanApplyDirectly => {
                    newly_applied_ids.push(local_change.id_last());
                    self.apply_change_from_remote(local_change, would_affect.as_deref_mut());
                }
            }
        }

        if !newly_applied_ids.is_empty() {
            self.try_apply_pending(newly_applied_ids, would_affect);
        }

        // A parked change can already be partially covered by the oplog VV: a change whose
        // span straddles the VV head is not `Applied`, and it still gets parked when one of
        // its cross-peer deps is missing. Only the uncovered tail of such a span is pending.
        //
        // Trim each span on its own rather than merging first. `VersionRange` keeps a single
        // (start, end) per peer, so trimming a merged range would attribute the gap between
        // two disjoint parked spans to the covered one. That is defensive here — the input
        // is lamport-ordered, so a parked change is unlikely to be unblocked by the
        // `try_apply_pending` above — but it keeps the reported range honest regardless.
        let mut still_pending = VersionRange::default();
        for span in parked {
            let vv_end = self.vv().get(&span.peer).copied().unwrap_or(0);
            if vv_end < span.ctr_end() {
                let start = span.counter.start.max(vv_end);
                still_pending.extends_to_include_id_span(IdSpan::new(
                    span.peer,
                    start,
                    span.ctr_end(),
                ));
            }
        }

        still_pending
    }
}

impl OpLog {
    /// Try to apply pending changes.
    ///
    /// `new_ids` are the ID of the op that is just applied.
    pub(crate) fn try_apply_pending(
        &mut self,
        mut new_ids: Vec<ID>,
        mut would_affect: Option<&mut VersionRange>,
    ) {
        while let Some(id) = new_ids.pop() {
            let Some(tree) = self.pending_changes.changes.get_mut(&id.peer) else {
                continue;
            };

            let mut to_remove = Vec::new();
            for (cnt, _) in tree.range_mut(0..=id.counter) {
                to_remove.push(*cnt);
            }

            let mut pending_set = Vec::with_capacity(to_remove.len());
            for cnt in to_remove {
                let pending_changes = tree.remove(&cnt).unwrap();
                if let Some(rollback) = self.import_rollback.as_mut() {
                    rollback
                        .pending
                        .record_removed(id.peer, cnt, &pending_changes);
                }
                pending_set.push(pending_changes);
            }

            if tree.is_empty() {
                self.pending_changes.changes.remove(&id.peer);
            }

            for pending_changes in pending_set {
                for pending_change in pending_changes {
                    match remote_change_apply_state(
                        self.dag.vv(),
                        self.dag.shallow_since_vv(),
                        &pending_change,
                    ) {
                        ChangeState::CanApplyDirectly => {
                            new_ids.push(pending_change.id_last());
                            self.apply_change_from_remote(
                                pending_change,
                                would_affect.as_deref_mut(),
                            );
                        }
                        ChangeState::Applied => {}
                        ChangeState::AwaitingMissingDependency(miss_dep) => {
                            self.push_pending_change(miss_dep, pending_change)
                        }
                    }
                }
            }
        }
    }

    pub(super) fn apply_change_from_remote(
        &mut self,
        change: PendingChange,
        would_affect: Option<&mut VersionRange>,
    ) {
        let change = match change {
            PendingChange::Known(mut c) => {
                self.dag.calc_unknown_lamport_change(&mut c).unwrap();
                c
            }
            PendingChange::Unknown(mut c) => {
                self.dag.calc_unknown_lamport_change(&mut c).unwrap();
                c
            }
        };

        let Some(change) = self.trim_the_known_part_of_change(change) else {
            return;
        };

        if let Some(w) = would_affect {
            w.extends_to_include_id_span(change.id_span());
        }
        self.insert_new_change(change, false);
    }
}

enum ChangeState {
    Applied,
    CanApplyDirectly,
    // The id of first missing dep
    AwaitingMissingDependency(ID),
}

fn remote_change_apply_state(
    vv: &VersionVector,
    shallow_vv: &ImVersionVector,
    change: &Change,
) -> ChangeState {
    let peer = change.id.peer;
    let CounterSpan { start, end } = change.ctr_span();
    let vv_latest_ctr = vv.get(&peer).copied().unwrap_or(0);
    if vv_latest_ctr >= end {
        return ChangeState::Applied;
    }

    if vv_latest_ctr < start {
        return ChangeState::AwaitingMissingDependency(change.id.inc(-1));
    }

    for dep in change.deps.iter() {
        let dep_vv_latest_ctr = vv.get(&dep.peer).copied().unwrap_or(0);
        if dep_vv_latest_ctr - 1 < dep.counter {
            return ChangeState::AwaitingMissingDependency(dep);
        }

        // The dep is covered by the doc's version, but if it lies below the
        // shallow root its DAG node was trimmed and the change's lamport can
        // never be computed here. Claiming `CanApplyDirectly` would panic in
        // `apply_change_from_remote`'s lamport calculation (#1068); park the
        // change instead. Imports of such changes are normally rejected up
        // front by `import_deps_before_shallow_root`, so this only guards
        // changes that were parked as pending before the doc became shallow.
        if shallow_vv.includes_id(dep) {
            return ChangeState::AwaitingMissingDependency(dep);
        }
    }

    ChangeState::CanApplyDirectly
}

#[cfg(test)]
mod test {
    use crate::{cursor::PosType, loro::ExportMode, LoroDoc, ToJson, VersionVector};

    #[test]
    fn import_pending() {
        let a = LoroDoc::new_auto_commit();
        a.set_peer_id(1).unwrap();
        let b = LoroDoc::new_auto_commit();
        b.set_peer_id(2).unwrap();
        let text_a = a.get_text("text");
        text_a.insert(0, "a", PosType::Unicode).unwrap();
        let update1 = a
            .export(ExportMode::updates(&VersionVector::default()))
            .unwrap();
        let version1 = a.oplog_vv();
        text_a.insert(0, "b", PosType::Unicode).unwrap();
        let update2 = a.export(ExportMode::updates(&version1)).unwrap();
        let version2 = a.oplog_vv();
        text_a.insert(0, "c", PosType::Unicode).unwrap();
        let update3 = a.export(ExportMode::updates(&version2)).unwrap();
        let version3 = a.oplog_vv();
        text_a.insert(0, "d", PosType::Unicode).unwrap();
        let update4 = a.export(ExportMode::updates(&version3)).unwrap();
        // let version4 = a.oplog_vv();
        text_a.insert(0, "e", PosType::Unicode).unwrap();
        let update3_5 = a.export(ExportMode::updates(&version2)).unwrap();
        b.import(&update3_5).unwrap();
        b.import(&update4).unwrap();
        b.import(&update2).unwrap();
        b.import(&update3).unwrap();
        b.import(&update1).unwrap();
        assert_eq!(a.get_deep_value(), b.get_deep_value());
    }

    #[test]
    fn pending_import_snapshot() {
        let a = LoroDoc::new_auto_commit();
        a.set_peer_id(1).unwrap();
        let b = LoroDoc::new_auto_commit();
        b.set_peer_id(2).unwrap();
        let text_a = a.get_text("text");
        text_a.insert(0, "a", PosType::Unicode).unwrap();
        let update1 = a.export(ExportMode::Snapshot).unwrap();
        let version1 = a.oplog_vv();
        text_a.insert(0, "b", PosType::Unicode).unwrap();
        let update2 = a.export(ExportMode::updates(&version1)).unwrap();
        let _version2 = a.oplog_vv();
        b.import(&update2).unwrap();
        // snapshot will be converted to updates
        b.import(&update1).unwrap();
        assert_eq!(a.get_deep_value(), b.get_deep_value());
    }

    #[test]
    fn need_deps_pending_import() {
        // a:   a1 <--- a2
        //        \    /
        // b:       b1
        let a = LoroDoc::new_auto_commit();
        a.set_peer_id(1).unwrap();
        let b = LoroDoc::new_auto_commit();
        b.set_peer_id(2).unwrap();
        let c = LoroDoc::new_auto_commit();
        c.set_peer_id(3).unwrap();
        let d = LoroDoc::new_auto_commit();
        d.set_peer_id(4).unwrap();
        let text_a = a.get_text("text");
        let text_b = b.get_text("text");
        text_a.insert(0, "a", PosType::Unicode).unwrap();
        let version_a1 = a.oplog_vv();
        let update_a1 = a
            .export(ExportMode::updates(&VersionVector::default()))
            .unwrap();
        b.import(&update_a1).unwrap();
        text_b.insert(1, "b", PosType::Unicode).unwrap();
        let update_b1 = b.export(ExportMode::updates(&version_a1)).unwrap();
        a.import(&update_b1).unwrap();
        let version_a1b1 = a.oplog_vv();
        text_a.insert(2, "c", PosType::Unicode).unwrap();
        let update_a2 = a.export(ExportMode::updates(&version_a1b1)).unwrap();
        c.import(&update_a2).unwrap();
        assert_eq!(c.get_deep_value().to_json(), "{\"text\":\"\"}");
        c.import(&update_a1).unwrap();
        assert_eq!(c.get_deep_value().to_json(), "{\"text\":\"a\"}");
        c.import(&update_b1).unwrap();
        assert_eq!(a.get_deep_value(), c.get_deep_value());

        d.import(&update_a2).unwrap();
        assert_eq!(d.get_deep_value().to_json(), "{\"text\":\"\"}");
        d.import(&update_b1).unwrap();
        assert_eq!(d.get_deep_value().to_json(), "{\"text\":\"\"}");
        d.import(&update_a1).unwrap();
        assert_eq!(a.get_deep_value(), d.get_deep_value());
    }

    #[test]
    fn should_activate_pending_change_when() {
        // 0@a <- 0@b
        // 0@a <- 1@a, where 0@a and 1@a will be merged
        // In this case, c apply b's change first, then apply all the changes from a.
        // C is expected to have the same content as a, after a imported b's change
        let a = LoroDoc::new_auto_commit();
        a.set_peer_id(1).unwrap();
        let b = LoroDoc::new_auto_commit();
        b.set_peer_id(2).unwrap();
        let c = LoroDoc::new_auto_commit();
        c.set_peer_id(3).unwrap();
        let text_a = a.get_text("text");
        let text_b = b.get_text("text");
        text_a.insert(0, "1", PosType::Unicode).unwrap();
        b.import(&a.export(ExportMode::Snapshot).unwrap()).unwrap();
        text_b.insert(0, "1", PosType::Unicode).unwrap();
        let b_change = b.export(ExportMode::updates(&a.oplog_vv())).unwrap();
        text_a.insert(0, "1", PosType::Unicode).unwrap();
        c.import(&b_change).unwrap();
        c.import(&a.export(ExportMode::Snapshot).unwrap()).unwrap();
        a.import(&b_change).unwrap();
        assert_eq!(c.get_deep_value(), a.get_deep_value());
    }

    /// Regression: importing a blob can both apply ops that unlock previously pending
    /// changes *and* contain later ops that depended on those pending changes.
    ///
    /// Sequence:
    /// 1. D has A0.
    /// 2. Import A1 (deps B0) → A1 parked as pending.
    /// 3. Import a single blob with B0 (deps A0) + B1 (deps A1):
    ///    - B0 applies; B1 is deferred (A1 not yet in the DAG).
    ///    - `try_apply_pending` then applies A1 (unlocked by B0).
    ///    - B1 must now apply instead of hitting `unreachable!("can apply directly")`.
    #[test]
    fn pending_change_becomes_applicable_after_try_apply_pending() {
        let (u_a0, u_a1_only, u_b0_b1, expected) = concurrent_pending_unlock_fixture();

        let d = LoroDoc::new_auto_commit();
        d.set_peer_id(3).unwrap();
        d.import(&u_a0).unwrap();
        let status_a1 = d.import(&u_a1_only).unwrap();
        assert!(
            status_a1.pending.is_some(),
            "A1 should be pending without B0: {status_a1:?}"
        );
        assert!(status_a1.success.is_empty(), "{status_a1:?}");

        // This used to panic with unreachable!("can apply directly").
        let status_b = d.import(&u_b0_b1).unwrap();
        assert!(status_b.pending.is_none(), "{status_b:?}");
        assert_eq!(d.get_deep_value(), expected.get_deep_value());
        assert_eq!(d.oplog_vv(), expected.oplog_vv());
    }

    /// Same causal pattern via `import_batch` (path that hit WASM `unreachable`
    /// with the lody snapshot+updates fixture).
    #[test]
    fn import_batch_applies_pending_unlocked_within_blob() {
        let (u_a0, u_a1_only, u_b0_b1, expected) = concurrent_pending_unlock_fixture();

        let d = LoroDoc::new_auto_commit();
        d.set_peer_id(3).unwrap();
        d.import(&u_a0).unwrap();
        d.import(&u_a1_only).unwrap();
        d.import_batch(std::slice::from_ref(&u_b0_b1)).unwrap();
        // `import_batch` force-detaches for the duration of the batch and reattaches
        // after the loop; a panic inside used to unwind past the reattach and strand
        // the doc detached, failing every later import and export.
        assert!(!d.is_detached(), "import_batch must leave the doc attached");
        assert_eq!(d.get_deep_value(), expected.get_deep_value());
        assert_eq!(d.oplog_vv(), expected.oplog_vv());
    }

    /// Multi-blob `import_batch` (like lody drain): a large peer-A blob that
    /// leaves pending ops, then a peer-B blob that both unlocks them and depends
    /// on the previously pending range.
    #[test]
    fn import_batch_multi_blob_unlocks_and_applies_chained_pending() {
        let (u_a0, u_a1_only, u_b0_b1, expected) = concurrent_pending_unlock_fixture();

        let d = LoroDoc::new_auto_commit();
        d.set_peer_id(3).unwrap();
        d.import(&u_a0).unwrap();
        // Sorted by change_num inside import_batch; either order must work.
        d.import_batch(&[u_a1_only, u_b0_b1]).unwrap();
        assert!(!d.is_detached(), "import_batch must leave the doc attached");
        assert_eq!(d.get_deep_value(), expected.get_deep_value());
        assert_eq!(d.oplog_vv(), expected.oplog_vv());
    }

    fn concurrent_pending_unlock_fixture() -> (Vec<u8>, Vec<u8>, Vec<u8>, LoroDoc) {
        use loro_common::IdSpan;

        let a = LoroDoc::new_auto_commit();
        a.set_peer_id(1).unwrap();
        let text_a = a.get_text("text");
        text_a.insert(0, "A0", PosType::Unicode).unwrap();
        let u_a0 = a
            .export(ExportMode::updates(&VersionVector::default()))
            .unwrap();
        let vv_a0 = a.oplog_vv();
        let a_counter_after_a0 = *vv_a0.get(&1).unwrap();

        let b = LoroDoc::new_auto_commit();
        b.set_peer_id(2).unwrap();
        b.import(&u_a0).unwrap();
        b.get_text("text")
            .insert(2, "B0", PosType::Unicode)
            .unwrap();
        let u_b0 = b.export(ExportMode::updates(&vv_a0)).unwrap();

        a.import(&u_b0).unwrap();
        text_a.insert(4, "A1", PosType::Unicode).unwrap();
        let a_counter_after_a1 = *a.oplog_vv().get(&1).unwrap();
        // Peer-A ops only (A1), not B0 — so this stays pending on a doc that only has A0.
        let u_a1_only = a
            .export(ExportMode::updates_in_range(vec![IdSpan::new(
                1,
                a_counter_after_a0,
                a_counter_after_a1,
            )]))
            .unwrap();

        b.import(&u_a1_only).unwrap();
        b.get_text("text")
            .insert(6, "B1", PosType::Unicode)
            .unwrap();
        // Single blob containing B0 and B1. B1 depends on A1.
        let u_b0_b1 = b.export(ExportMode::updates(&vv_a0)).unwrap();

        (u_a0, u_a1_only, u_b0_b1, b)
    }

    // Change cannot be merged now
    // #[test]
    // fn pending_changes_may_deps_merged_change() {
    //     // a:  (a1 <-- a2 <-- a3) <-- a4       a1~a3 is a merged change
    //     //                \         /
    //     // b:                b1
    //     let a = LoroDoc::new();
    //     a.set_peer_id(1);
    //     let b = LoroDoc::new();
    //     b.set_peer_id(2);
    //     let c = LoroDoc::new();
    //     c.set_peer_id(3);
    //     let text_a = a.get_text("text");
    //     let text_b = b.get_text("text");
    //     a.with_txn(|txn| text_a.insert(txn, 0, "a")).unwrap();
    //     a.with_txn(|txn| text_a.insert(txn, 1, "b")).unwrap();
    //     let version_a12 = a.oplog_vv();
    //     let updates_a12 = a.export(ExportMode::Snapshot);
    //     a.with_txn(|txn| text_a.insert(txn, 2, "c")).unwrap();
    //     let updates_a123 = a.export(ExportMode::Snapshot);
    //     b.import(&updates_a12).unwrap();
    //     b.with_txn(|txn| text_b.insert(txn, 2, "d")).unwrap();
    //     let update_b1 = b.export(ExportMode::updates(&version_a12)).unwrap();
    //     a.import(&update_b1).unwrap();
    //     let version_a123_b1 = a.oplog_vv();
    //     a.with_txn(|txn| text_a.insert(txn, 4, "e")).unwrap();
    //     let update_a4 = a.export(ExportMode::updates(&version_a123_b1)).unwrap();
    //     c.import(&update_b1).unwrap();
    //     assert_eq!(c.get_deep_value().to_json(), "{\"text\":\"\"}");
    //     c.import(&update_a4).unwrap();
    //     assert_eq!(c.get_deep_value().to_json(), "{\"text\":\"\"}");
    //     c.import(&updates_a123).unwrap();
    //     assert_eq!(c.get_deep_value(), a.get_deep_value());
    // }
}

use std::{collections::BTreeMap, ops::Deref};

use crate::{
    change::Change,
    version::{ImVersionVector, VersionRange},
    OpLog, VersionVector,
};
use loro_common::{ContainerType, Counter, CounterSpan, HasCounterSpan, HasIdSpan, PeerID, ID};
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

#[derive(Debug, Default)]
pub(crate) struct PendingChangesRollback {
    added: Vec<(PeerID, Counter)>,
    removed: Vec<(PeerID, Counter, Vec<PendingChange>)>,
}

impl PendingChangesRollback {
    fn record_added(&mut self, id: ID) {
        self.added.push((id.peer, id.counter));
    }

    fn record_removed(&mut self, peer: PeerID, counter: Counter, changes: Vec<PendingChange>) {
        self.removed.push((peer, counter, changes));
    }

    pub(crate) fn rollback(self, pending_changes: &mut PendingChanges) {
        for (peer, counter) in self.added.into_iter().rev() {
            let Some(tree) = pending_changes.changes.get_mut(&peer) else {
                continue;
            };
            let Some(changes) = tree.get_mut(&counter) else {
                continue;
            };
            changes.pop();
            if changes.is_empty() {
                tree.remove(&counter);
            }
            if tree.is_empty() {
                pending_changes.changes.remove(&peer);
            }
        }

        for (peer, counter, changes) in self.removed.into_iter().rev() {
            pending_changes
                .changes
                .entry(peer)
                .or_default()
                .insert(counter, changes);
        }
    }
}

impl OpLog {
    fn push_pending_change(&mut self, missing_dep: ID, change: PendingChange) {
        if let Some(rollback) = self.import_rollback.as_mut() {
            rollback.pending.record_added(missing_dep);
        }

        self.pending_changes
            .changes
            .entry(missing_dep.peer)
            .or_default()
            .entry(missing_dep.counter)
            .or_default()
            .push(change);
    }

    /// Files the changes that [`import_changes_to_oplog`] could not apply into the
    /// long-lived pending store, keyed by the dependency they are still missing.
    ///
    /// `remote_changes` was classified while the oplog still held the pre-import
    /// version, but [`OpLog::try_apply_pending`] runs in between and applies changes
    /// that were already parked in the store. That advances the oplog version, so a
    /// change that was un-appliable at classification time can legitimately be
    /// applicable — or already applied — by the time it reaches this function. Both
    /// re-classifications are normal import outcomes, not invariant violations.
    ///
    /// Returns the ID range of the changes that are genuinely still pending, and
    /// extends `imported` with anything applied here.
    ///
    /// [`import_changes_to_oplog`]: crate::encoding::outdated_encode_reordered::import_changes_to_oplog
    pub(super) fn extend_pending_changes_with_unknown_lamport(
        &mut self,
        remote_changes: Vec<Change>,
        imported: &mut VersionRange,
    ) -> VersionRange {
        let mut filed = Vec::new();
        let mut newly_applied = Vec::new();
        for change in remote_changes {
            let local_change = PendingChange::Unknown(change);
            match remote_change_apply_state(self.vv(), self.shallow_since_vv(), &local_change) {
                ChangeState::AwaitingMissingDependency(miss_dep) => {
                    filed.push(local_change.id_span());
                    self.push_pending_change(miss_dep, local_change);
                }
                // `try_apply_pending` already brought this change in.
                ChangeState::Applied => {}
                // `try_apply_pending` supplied the dependency this change was waiting
                // on, so it can be applied right away instead of being parked.
                ChangeState::CanApplyDirectly => {
                    newly_applied.push(local_change.id_last());
                    self.apply_change_from_remote(local_change, Some(imported));
                }
            }
        }

        // Applying the changes above can in turn unlock changes parked in the store,
        // including ones filed earlier in this very loop.
        if !newly_applied.is_empty() {
            self.try_apply_pending(newly_applied, Some(imported));
        }

        let mut still_pending = VersionRange::default();
        for span in filed {
            let applied_ctr = self.vv().get(&span.peer).copied().unwrap_or(0);
            if applied_ctr < span.ctr_end() {
                still_pending.extends_to_include_id_span(span);
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
                        .record_removed(id.peer, cnt, pending_changes.clone());
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
    _shallow_vv: &ImVersionVector,
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

    /// Builds the D <- X <- Y dependency chain and returns
    /// `(x_update, dy_update, converged)` where `dy_update` carries D and Y but
    /// not X, and `converged` is the deep value once all three are applied.
    fn dep_chain_updates() -> (Vec<u8>, Vec<u8>, loro_common::LoroValue) {
        const D_PEER: u64 = 1;
        const X_PEER: u64 = 2;
        const Y_PEER: u64 = 3;

        let d = LoroDoc::new_auto_commit();
        d.set_peer_id(D_PEER).unwrap();
        d.get_map("m").insert("d", 1).unwrap();
        let d_update = d
            .export(ExportMode::updates(&VersionVector::default()))
            .unwrap();

        let x = LoroDoc::new_auto_commit();
        x.set_peer_id(X_PEER).unwrap();
        x.import(&d_update).unwrap();
        let before_x = x.oplog_vv();
        x.get_map("m").insert("x", 1).unwrap();
        let x_update = x.export(ExportMode::updates(&before_x)).unwrap();

        let y = LoroDoc::new_auto_commit();
        y.set_peer_id(Y_PEER).unwrap();
        y.import(&d_update).unwrap();
        y.import(&x_update).unwrap();
        y.get_map("m").insert("y", 1).unwrap();

        // Everything `y` knows except X's ops, so the blob holds D and Y only.
        let without_x = VersionVector::from_iter([(X_PEER, 1)]);
        let dy_update = y.export(ExportMode::updates(&without_x)).unwrap();

        (x_update, dy_update, y.get_deep_value())
    }

    /// `try_apply_pending` runs between the classification in
    /// `import_changes_to_oplog` and `extend_pending_changes_with_unknown_lamport`,
    /// so it can supply the dependency a still-unfiled change is waiting for. That
    /// change must then be applied instead of tripping an "impossible state" panic.
    #[test]
    fn pending_change_unlocked_by_try_apply_pending() {
        let (x_update, dy_update, converged) = dep_chain_updates();

        let t = LoroDoc::new_auto_commit();
        t.set_peer_id(9).unwrap();

        // X parks in the pending store: it depends on D, which has not arrived.
        let status = t.import(&x_update).unwrap();
        assert!(status.pending.is_some(), "X should be pending");

        // D unblocks X via `try_apply_pending`, which in turn unblocks Y.
        let status = t.import(&dy_update).unwrap();
        assert_eq!(status.pending, None, "nothing should be left pending");
        assert_eq!(
            t.get_deep_value(),
            converged,
            "all three changes must be applied"
        );
        assert_eq!(t.oplog().lock().pending_changes.len(), 0);
    }

    /// The same import through `import_batch`, which force-detaches the document
    /// for the duration of the batch and only reattaches after the loop. A panic in
    /// the loop used to skip the reattach and strand the document detached forever.
    #[test]
    fn pending_change_unlocked_during_batch_import_keeps_doc_attached() {
        let (x_update, dy_update, _) = dep_chain_updates();

        let t = LoroDoc::new_auto_commit();
        t.set_peer_id(9).unwrap();
        t.import(&x_update).unwrap();

        let unrelated = LoroDoc::new_auto_commit();
        unrelated.set_peer_id(77).unwrap();
        unrelated.get_map("m").insert("z", 1).unwrap();
        let z_update = unrelated
            .export(ExportMode::updates(&VersionVector::default()))
            .unwrap();

        // More than one blob, so this takes the real `import_batch` path.
        t.import_batch(&[dy_update, z_update]).unwrap();

        assert!(
            !t.is_detached(),
            "doc must stay attached after import_batch"
        );
        let loro_common::LoroValue::Map(root) = t.get_deep_value() else {
            panic!("root should be a map");
        };
        let Some(loro_common::LoroValue::Map(m)) = root.get("m").cloned() else {
            panic!("`m` should be a map");
        };
        let mut keys: Vec<&str> = m.keys().map(|k| k.as_str()).collect();
        keys.sort_unstable();
        assert_eq!(keys, ["d", "x", "y", "z"]);
    }
}

use std::{collections::BTreeMap, ops::Deref};

use crate::{change::Change, version::ImVersionVector, OpLog, VersionVector};
use fxhash::FxHashMap;
use loro_common::{Counter, CounterSpan, HasCounterSpan, HasIdSpan, LoroResult, PeerID, ID};

#[derive(Debug)]
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
    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }
}

impl OpLog {
    pub(super) fn extend_pending_changes_with_unknown_lamport(
        &mut self,
        remote_changes: Vec<Change>,
    ) -> LoroResult<()> {
        let mut result = Ok(());
        for change in remote_changes {
            let local_change = PendingChange::Unknown(change);
            match remote_change_apply_state(self.vv(), self.trimmed_vv(), &local_change) {
                ChangeState::AwaitingMissingDependency(miss_dep) => self
                    .pending_changes
                    .changes
                    .entry(miss_dep.peer)
                    .or_default()
                    .entry(miss_dep.counter)
                    .or_default()
                    .push(local_change),
                ChangeState::DependingOnTrimmedHistory(_ids) => {
                    result = LoroResult::Err(
                        loro_common::LoroError::ImportUpdatesThatDependsOnOutdatedVersion,
                    );
                }
                ChangeState::Applied => unreachable!("already applied"),
                ChangeState::CanApplyDirectly => unreachable!("can apply directly"),
            }
        }

        result
    }
}

impl OpLog {
    /// Try to apply pending changes.
    ///
    /// `new_ids` are the ID of the op that is just applied.
    pub(crate) fn try_apply_pending(&mut self, mut new_ids: Vec<ID>) {
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
                pending_set.push(tree.remove(&cnt).unwrap());
            }

            if tree.is_empty() {
                self.pending_changes.changes.remove(&id.peer);
            }

            for pending_changes in pending_set {
                for pending_change in pending_changes {
                    match remote_change_apply_state(
                        self.dag.vv(),
                        self.dag.trimmed_vv(),
                        &pending_change,
                    ) {
                        ChangeState::CanApplyDirectly => {
                            new_ids.push(pending_change.id_last());
                            self.apply_change_from_remote(pending_change);
                        }
                        ChangeState::Applied => {}
                        ChangeState::AwaitingMissingDependency(miss_dep) => self
                            .pending_changes
                            .changes
                            .entry(miss_dep.peer)
                            .or_default()
                            .entry(miss_dep.counter)
                            .or_default()
                            .push(pending_change),
                        ChangeState::DependingOnTrimmedHistory(_) => {
                            unreachable!()
                        }
                    }
                }
            }
        }
    }

    pub(super) fn apply_change_from_remote(&mut self, change: PendingChange) {
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

        self.insert_new_change(change, false);
    }
}

enum ChangeState {
    Applied,
    CanApplyDirectly,
    // The id of first missing dep
    AwaitingMissingDependency(ID),
    DependingOnTrimmedHistory(Box<Vec<ID>>),
}

fn remote_change_apply_state(
    vv: &VersionVector,
    _trimmed_vv: &ImVersionVector,
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

    for dep in change.deps.as_ref().iter() {
        let dep_vv_latest_ctr = vv.get(&dep.peer).copied().unwrap_or(0);
        if dep_vv_latest_ctr - 1 < dep.counter {
            return ChangeState::AwaitingMissingDependency(*dep);
        }
    }

    ChangeState::CanApplyDirectly
}

#[cfg(test)]
mod test {
    use crate::{LoroDoc, ToJson, VersionVector};

    #[test]
    fn import_pending() {
        let a = LoroDoc::new();
        a.set_peer_id(1).unwrap();
        let b = LoroDoc::new();
        b.set_peer_id(2).unwrap();
        let text_a = a.get_text("text");
        a.with_txn(|txn| text_a.insert_with_txn(txn, 0, "a"))
            .unwrap();

        let update1 = a.export_from(&VersionVector::default());
        let version1 = a.oplog_vv();
        a.with_txn(|txn| text_a.insert_with_txn(txn, 0, "b"))
            .unwrap();
        let update2 = a.export_from(&version1);
        let version2 = a.oplog_vv();
        a.with_txn(|txn| text_a.insert_with_txn(txn, 0, "c"))
            .unwrap();
        let update3 = a.export_from(&version2);
        let version3 = a.oplog_vv();
        a.with_txn(|txn| text_a.insert_with_txn(txn, 0, "d"))
            .unwrap();
        let update4 = a.export_from(&version3);
        // let version4 = a.oplog_vv();
        a.with_txn(|txn| text_a.insert_with_txn(txn, 0, "e"))
            .unwrap();
        let update3_5 = a.export_from(&version2);
        b.import(&update3_5).unwrap();
        b.import(&update4).unwrap();
        b.import(&update2).unwrap();
        b.import(&update3).unwrap();
        b.import(&update1).unwrap();
        assert_eq!(a.get_deep_value(), b.get_deep_value());
    }

    #[test]
    fn pending_import_snapshot() {
        let a = LoroDoc::new();
        a.set_peer_id(1).unwrap();
        let b = LoroDoc::new();
        b.set_peer_id(2).unwrap();
        let text_a = a.get_text("text");
        a.with_txn(|txn| text_a.insert_with_txn(txn, 0, "a"))
            .unwrap();
        let update1 = a.export_snapshot();
        let version1 = a.oplog_vv();
        a.with_txn(|txn| text_a.insert_with_txn(txn, 1, "b"))
            .unwrap();
        let update2 = a.export_from(&version1);
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
        let a = LoroDoc::new();
        a.set_peer_id(1).unwrap();
        let b = LoroDoc::new();
        b.set_peer_id(2).unwrap();
        let c = LoroDoc::new();
        c.set_peer_id(3).unwrap();
        let d = LoroDoc::new();
        d.set_peer_id(4).unwrap();
        let text_a = a.get_text("text");
        let text_b = b.get_text("text");
        a.with_txn(|txn| text_a.insert_with_txn(txn, 0, "a"))
            .unwrap();
        let version_a1 = a.oplog_vv();
        let update_a1 = a.export_from(&VersionVector::default());
        b.import(&update_a1).unwrap();
        b.with_txn(|txn| text_b.insert_with_txn(txn, 1, "b"))
            .unwrap();
        let update_b1 = b.export_from(&version_a1);
        a.import(&update_b1).unwrap();
        let version_a1b1 = a.oplog_vv();
        a.with_txn(|txn| text_a.insert_with_txn(txn, 2, "c"))
            .unwrap();
        let update_a2 = a.export_from(&version_a1b1);
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
        let a = LoroDoc::new();
        a.set_peer_id(1).unwrap();
        let b = LoroDoc::new();
        b.set_peer_id(2).unwrap();
        let c = LoroDoc::new();
        c.set_peer_id(3).unwrap();
        let text_a = a.get_text("text");
        let text_b = b.get_text("text");
        a.with_txn(|txn| text_a.insert_with_txn(txn, 0, "1"))
            .unwrap();
        b.import(&a.export_snapshot()).unwrap();
        b.with_txn(|txn| text_b.insert_with_txn(txn, 0, "1"))
            .unwrap();
        let b_change = b.export_from(&a.oplog_vv());
        a.with_txn(|txn| text_a.insert_with_txn(txn, 0, "1"))
            .unwrap();
        c.import(&b_change).unwrap();
        c.import(&a.export_snapshot()).unwrap();
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
    //     let updates_a12 = a.export_snapshot();
    //     a.with_txn(|txn| text_a.insert(txn, 2, "c")).unwrap();
    //     let updates_a123 = a.export_snapshot();
    //     b.import(&updates_a12).unwrap();
    //     b.with_txn(|txn| text_b.insert(txn, 2, "d")).unwrap();
    //     let update_b1 = b.export_from(&version_a12);
    //     a.import(&update_b1).unwrap();
    //     let version_a123_b1 = a.oplog_vv();
    //     a.with_txn(|txn| text_a.insert(txn, 4, "e")).unwrap();
    //     let update_a4 = a.export_from(&version_a123_b1);
    //     c.import(&update_b1).unwrap();
    //     assert_eq!(c.get_deep_value().to_json(), "{\"text\":\"\"}");
    //     c.import(&update_a4).unwrap();
    //     assert_eq!(c.get_deep_value().to_json(), "{\"text\":\"\"}");
    //     c.import(&updates_a123).unwrap();
    //     assert_eq!(c.get_deep_value(), a.get_deep_value());
    // }
}

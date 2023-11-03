use std::{collections::BTreeMap, ops::Deref};

use crate::{
    arena::OpConverter, change::Change, encoding::RemoteClientChanges, op::RemoteOp, OpLog,
    VersionVector,
};
use fxhash::FxHashMap;
use itertools::Itertools;
use loro_common::{
    Counter, CounterSpan, HasCounterSpan, HasIdSpan, HasLamportSpan, LoroError, PeerID, ID,
};
use rle::RleVec;
use smallvec::SmallVec;

#[derive(Debug)]
pub enum PendingChange {
    // The lamport of the change decoded by `enhanced` is unknown.
    // we need calculate it when the change can be applied
    Unknown(Change),
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
    changes: FxHashMap<PeerID, BTreeMap<Counter, SmallVec<[PendingChange; 1]>>>,
}

impl OpLog {
    // calculate all `id_last`(s) whose change can be applied
    pub(super) fn apply_appliable_changes_and_cache_pending(
        &mut self,
        remote_changes: RemoteClientChanges,
        converter: &mut OpConverter,
        mut latest_vv: VersionVector,
    ) -> Vec<ID> {
        let mut ans = Vec::new();
        for change in remote_changes
            .into_values()
            .filter(|c| !c.is_empty())
            .flat_map(|c| c.into_iter())
            .sorted_unstable_by_key(|c| c.lamport)
        {
            let local_change = to_local_op(change, converter);
            let local_change = PendingChange::Known(local_change);
            match remote_change_apply_state(&latest_vv, &local_change) {
                ChangeApplyState::CanApplyDirectly => {
                    latest_vv.set_end(local_change.id_end());
                    ans.push(local_change.id_last());
                    self.apply_local_change_from_remote(local_change);
                }
                ChangeApplyState::Applied => {}
                ChangeApplyState::AwaitingDependency(miss_dep) => self
                    .pending_changes
                    .changes
                    .entry(miss_dep.peer)
                    .or_default()
                    .entry(miss_dep.counter)
                    .or_default()
                    .push(local_change),
            }
        }
        ans
    }

    pub(super) fn extend_pending_changes_with_unknown_lamport(
        &mut self,
        remote_changes: Vec<Change<RemoteOp>>,
        converter: &mut OpConverter,
        latest_vv: &VersionVector,
    ) {
        for change in remote_changes {
            let local_change = to_local_op(change, converter);
            let local_change = PendingChange::Unknown(local_change);
            match remote_change_apply_state(latest_vv, &local_change) {
                ChangeApplyState::AwaitingDependency(miss_dep) => self
                    .pending_changes
                    .changes
                    .entry(miss_dep.peer)
                    .or_default()
                    .entry(miss_dep.counter)
                    .or_default()
                    .push(local_change),
                _ => unreachable!(),
            }
        }
    }
}

impl OpLog {
    pub(super) fn check_changes(&self, changes: &RemoteClientChanges) -> Result<(), LoroError> {
        for changes in changes.values() {
            if changes.is_empty() {
                continue;
            }
            // detect invalid id
            let mut last_end_counter = None;
            for change in changes.iter() {
                if change.id.counter < 0 {
                    return Err(LoroError::DecodeError(
                        "Invalid data. Negative id counter.".into(),
                    ));
                }
                if let Some(last_end_counter) = &mut last_end_counter {
                    if change.id.counter != *last_end_counter {
                        return Err(LoroError::DecodeError(
                            "Invalid data. Not continuous counter.".into(),
                        ));
                    }

                    *last_end_counter = change.id_end().counter;
                } else {
                    last_end_counter = Some(change.id_end().counter);
                }
            }
        }
        Ok(())
    }

    pub(super) fn try_apply_pending(
        &mut self,
        mut id_stack: Vec<ID>,
        latest_vv: &mut VersionVector,
    ) {
        while let Some(id) = id_stack.pop() {
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
                    match remote_change_apply_state(latest_vv, &pending_change) {
                        ChangeApplyState::CanApplyDirectly => {
                            id_stack.push(pending_change.id_last());
                            latest_vv.set_end(pending_change.id_end());
                            self.apply_local_change_from_remote(pending_change);
                        }
                        ChangeApplyState::Applied => {}
                        ChangeApplyState::AwaitingDependency(miss_dep) => self
                            .pending_changes
                            .changes
                            .entry(miss_dep.peer)
                            .or_default()
                            .entry(miss_dep.counter)
                            .or_default()
                            .push(pending_change),
                    }
                }
            }
        }
    }

    pub(super) fn apply_local_change_from_remote(&mut self, change: PendingChange) {
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
        self.next_lamport = self.next_lamport.max(change.lamport_end());
        // debug_dbg!(&change_causal_arr);
        self.dag.vv.extend_to_include_last_id(change.id_last());
        self.latest_timestamp = self.latest_timestamp.max(change.timestamp);
        let mark = self.insert_dag_node_on_new_change(&change);
        self.insert_new_change(change, mark);
    }
}

pub(super) fn to_local_op(change: Change<RemoteOp>, converter: &mut OpConverter) -> Change {
    let mut ops = RleVec::new();
    for op in change.ops {
        let lamport = change.lamport;
        let content = op.content;
        let op = converter.convert_single_op(
            &op.container,
            change.id.peer,
            op.counter,
            lamport,
            content,
        );
        ops.push(op);
    }
    Change {
        ops,
        id: change.id,
        deps: change.deps,
        lamport: change.lamport,
        timestamp: change.timestamp,
        has_dependents: false,
    }
}

enum ChangeApplyState {
    Applied,
    CanApplyDirectly,
    // The id of first missing dep
    AwaitingDependency(ID),
}

fn remote_change_apply_state(vv: &VersionVector, change: &Change) -> ChangeApplyState {
    let peer = change.id.peer;
    let CounterSpan { start, end } = change.ctr_span();
    let vv_latest_ctr = vv.get(&peer).copied().unwrap_or(0);
    if vv_latest_ctr < start {
        return ChangeApplyState::AwaitingDependency(change.id.inc(-1));
    }
    if vv_latest_ctr >= end {
        return ChangeApplyState::Applied;
    }
    for dep in change.deps.as_ref().iter() {
        let dep_vv_latest_ctr = vv.get(&dep.peer).copied().unwrap_or(0);
        if dep_vv_latest_ctr - 1 < dep.counter {
            return ChangeApplyState::AwaitingDependency(*dep);
        }
    }
    ChangeApplyState::CanApplyDirectly
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
        a.with_txn(|txn| text_a.insert(txn, 0, "a")).unwrap();

        let update1 = a.export_from(&VersionVector::default());
        let version1 = a.oplog_vv();
        a.with_txn(|txn| text_a.insert(txn, 0, "b")).unwrap();
        let update2 = a.export_from(&version1);
        let version2 = a.oplog_vv();
        a.with_txn(|txn| text_a.insert(txn, 0, "c")).unwrap();
        let update3 = a.export_from(&version2);
        let version3 = a.oplog_vv();
        a.with_txn(|txn| text_a.insert(txn, 0, "d")).unwrap();
        let update4 = a.export_from(&version3);
        // let version4 = a.oplog_vv();
        a.with_txn(|txn| text_a.insert(txn, 0, "e")).unwrap();
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
        a.with_txn(|txn| text_a.insert(txn, 0, "a")).unwrap();
        let update1 = a.export_snapshot();
        let version1 = a.oplog_vv();
        a.with_txn(|txn| text_a.insert(txn, 1, "b")).unwrap();
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
        a.with_txn(|txn| text_a.insert(txn, 0, "a")).unwrap();
        let version_a1 = a.oplog_vv();
        let update_a1 = a.export_from(&VersionVector::default());
        b.import(&update_a1).unwrap();
        b.with_txn(|txn| text_b.insert(txn, 1, "b")).unwrap();
        let update_b1 = b.export_from(&version_a1);
        a.import(&update_b1).unwrap();
        let version_a1b1 = a.oplog_vv();
        a.with_txn(|txn| text_a.insert(txn, 2, "c")).unwrap();
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
        a.with_txn(|txn| text_a.insert(txn, 0, "1")).unwrap();
        b.import(&a.export_snapshot()).unwrap();
        b.with_txn(|txn| text_b.insert(txn, 0, "1")).unwrap();
        let b_change = b.export_from(&a.oplog_vv());
        a.with_txn(|txn| text_a.insert(txn, 0, "1")).unwrap();
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

use crate::{
    change::Change,
    encoding::RemoteClientChanges,
    op::{Op, RemoteOp},
    OpLog, VersionVector,
};
use fxhash::{FxHashMap, FxHashSet};
use itertools::Itertools;
use loro_common::{CounterSpan, HasCounterSpan, HasIdSpan, PeerID};
use rle::RleVec;

type LocalChanges = FxHashMap<PeerID, Vec<Change>>;

pub(crate) struct PendingChanges {
    pending_changes: LocalChanges,
}

impl PendingChanges {
    pub(crate) fn try_apply_pending_changes(&mut self, op_log: &mut OpLog) {}

    fn convert_remote_to_pending_op(change: Change<RemoteOp>, op_log: &OpLog) -> Change {
        op_log.arena.with_op_converter(|converter| {
            let mut ops = RleVec::new();
            for op in change.ops {
                for content in op.contents.into_iter() {
                    let op = converter.convert_single_op(&op.container, op.counter, content);
                    ops.push(op);
                }
            }
            Change {
                ops,
                id: change.id,
                deps: change.deps,
                lamport: change.lamport,
                timestamp: change.timestamp,
            }
        })
    }

    pub(crate) fn filter_and_pending_remote_changes(
        &mut self,
        changes: RemoteClientChanges,
        op_log: &OpLog,
    ) -> LocalChanges {
        let mut latest_vv = op_log.vv().clone();
        let mut can_be_applied_changes = FxHashMap::default();

        let mut pending_peers = FxHashSet::default();
        // Changes will be sorted by lamport.
        for change in changes
            .into_values()
            .flat_map(|c| c.into_iter())
            .sorted_unstable_by_key(|c| c.lamport)
        {
            let peer = change.id.peer;
            let local_change = Self::convert_remote_to_pending_op(change, op_log);
            // If the first change cannot be applied, then all subsequent changes with the same client id cannot be applied either.
            if pending_peers.contains(&peer) {
                self.pending_changes
                    .entry(peer)
                    .or_insert_with(Vec::new)
                    .push(local_change);
                continue;
            }

            match remote_change_apply_state(&latest_vv, &local_change) {
                ChangeApplyState::Directly => {
                    latest_vv.set_end(local_change.id_end());
                    can_be_applied_changes
                        .entry(peer)
                        .or_insert_with(Vec::new)
                        .push(local_change);
                    self.try_apply_pending(&peer, &mut latest_vv, &mut can_be_applied_changes);
                }
                ChangeApplyState::Existing => {}
                ChangeApplyState::Future(this_dep_client) => {
                    pending_peers.insert(this_dep_client);
                    self.pending_changes
                        .entry(this_dep_client)
                        .or_insert_with(Vec::new)
                        .push(local_change);
                }
            }
        }

        can_be_applied_changes
    }

    fn try_apply_pending(
        &mut self,
        peer: &PeerID,
        latest_vv: &mut VersionVector,
        can_be_applied_changes: &mut LocalChanges,
    ) {
        if let Some(may_apply_changes) = self.pending_changes.remove(peer) {
            let mut may_apply_iter = may_apply_changes
                .into_iter()
                .sorted_by(|a, b| a.lamport.cmp(&b.lamport))
                .peekable();
            while let Some(peek_c) = may_apply_iter.peek() {
                match remote_change_apply_state(latest_vv, peek_c) {
                    ChangeApplyState::Directly => {
                        let c = may_apply_iter.next().unwrap();
                        let c_peer = c.id.peer;
                        latest_vv.set_end(c.id_end());
                        // other pending
                        can_be_applied_changes
                            .entry(c_peer)
                            .or_insert_with(Vec::new)
                            .push(c);
                        self.try_apply_pending(&c_peer, latest_vv, can_be_applied_changes);
                    }
                    ChangeApplyState::Existing => {
                        may_apply_iter.next();
                    }
                    ChangeApplyState::Future(this_dep_client) => {
                        self.pending_changes
                            .entry(this_dep_client)
                            .or_insert_with(Vec::new)
                            .extend(may_apply_iter);
                        break;
                    }
                }
            }
        }
    }
}

enum ChangeApplyState {
    Existing,
    Directly,
    // The client id of first missing dep
    Future(PeerID),
}

fn remote_change_apply_state(vv: &VersionVector, change: &Change) -> ChangeApplyState {
    let peer = change.id.peer;
    let CounterSpan { start, end } = change.ctr_span();
    let vv_latest_ctr = vv.get(&peer).copied().unwrap_or(0);
    if vv_latest_ctr < start {
        return ChangeApplyState::Future(peer);
    }
    if vv_latest_ctr >= end {
        return ChangeApplyState::Existing;
    }
    for dep in change.deps.as_ref().iter() {
        let dep_vv_latest_ctr = vv.get(&dep.peer).copied().unwrap_or(0);
        if dep_vv_latest_ctr - 1 < dep.counter {
            return ChangeApplyState::Future(dep.peer);
        }
    }
    ChangeApplyState::Directly
}

#[cfg(test)]
mod test {
    use crate::{LoroDoc, ToJson, VersionVector};

    #[test]
    fn import_pending() {
        let a = LoroDoc::new();
        a.set_peer_id(1);
        let b = LoroDoc::new();
        b.set_peer_id(2);
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
        a.set_peer_id(1);
        let b = LoroDoc::new();
        b.set_peer_id(2);
        let text_a = a.get_text("text");
        a.with_txn(|txn| text_a.insert(txn, 0, "a")).unwrap();
        let update1 = a.export_snapshot();
        let version1 = a.oplog_vv();
        a.with_txn(|txn| text_a.insert(txn, 1, "b")).unwrap();
        let update2 = a.export_from(&version1);
        let _version2 = a.oplog_vv();
        b.import(&update2).unwrap();
        b.import(&update1).unwrap();
        assert_eq!(a.get_deep_value(), b.get_deep_value());
    }

    #[test]
    fn need_deps_pending_import() {
        // a:   a1 <--- a2
        //        \    /
        // b:       b1
        let a = LoroDoc::new();
        a.set_peer_id(1);
        let b = LoroDoc::new();
        b.set_peer_id(2);
        let c = LoroDoc::new();
        c.set_peer_id(3);
        let d = LoroDoc::new();
        d.set_peer_id(4);
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
        assert_eq!(c.get_deep_value().to_json(), "{}");
        c.import(&update_a1).unwrap();
        assert_eq!(c.get_deep_value().to_json(), "{\"text\":\"a\"}");
        c.import(&update_b1).unwrap();
        assert_eq!(a.get_deep_value(), c.get_deep_value());

        d.import(&update_a2).unwrap();
        assert_eq!(d.get_deep_value().to_json(), "{}");
        d.import(&update_b1).unwrap();
        assert_eq!(d.get_deep_value().to_json(), "{}");
        d.import(&update_a1).unwrap();
        assert_eq!(a.get_deep_value(), d.get_deep_value());
    }

    // #[test]
    // fn should_activate_pending_change_when() {
    //     // 0@a <- 0@b
    //     // 0@a <- 1@a, where 0@a and 1@a will be merged
    //     // In this case, c apply b's change first, then apply all the changes from a.
    //     // C is expected to have the same content as a, after a imported b's change
    //     let a = LoroDoc::new(Default::default(), Some(1));
    //     let b = LoroDoc::new(Default::default(), Some(2));
    //     let c = LoroDoc::new(Default::default(), Some(3));
    //     let text_a = a.get_text("text");
    //     let text_b = b.get_text("text");
    //     text_a.insert(&a, 0, "1").unwrap();
    //     b.import(&a.export_snapshot()).unwrap();
    //     text_b.insert(&b, 0, "1").unwrap();
    //     let b_change = b.export_from(a.oplog_vv());
    //     text_a.insert(&a, 0, "1").unwrap();
    //     c.import(&b_change).unwrap();
    //     c.import(&a.export_snapshot()).unwrap();
    //     a.import(&b_change).unwrap();
    //     assert_eq!(c.get_deep_value(), a.get_deep_value());
    // }

    // #[test]
    // fn pending_changes_may_deps_merged_change() {
    //     // a:  (a1 <-- a2 <-- a3) <-- a4       a1~a3 is a merged change
    //     //                \         /
    //     // b:                b1
    //     let a = LoroDoc::new(Default::default(), Some(1));
    //     let b = LoroDoc::new(Default::default(), Some(2));
    //     let c = LoroDoc::new(Default::default(), Some(3));
    //     let text_a = a.get_text("text");
    //     let text_b = b.get_text("text");
    //     text_a.insert(&a, 0, "a").unwrap();
    //     text_a.insert(&a, 1, "b").unwrap();
    //     let version_a12 = a.oplog_vv();
    //     let updates_a12 = a.export_snapshot();
    //     text_a.insert(&a, 2, "c").unwrap();
    //     let updates_a123 = a.export_snapshot();
    //     b.import(&updates_a12).unwrap();
    //     text_b.insert(&b, 2, "d").unwrap();
    //     let update_b1 = b.export_from(version_a12);
    //     a.import(&update_b1).unwrap();
    //     let version_a123_b1 = a.oplog_vv();
    //     text_a.insert(&a, 4, "e").unwrap();
    //     let update_a4 = a.export_from(version_a123_b1);
    //     c.import(&update_b1).unwrap();
    //     assert_eq!(c.get_deep_value().get_deep_value(), "{}");
    //     c.import(&update_a4).unwrap();
    //     assert_eq!(c.get_deep_value().get_deep_value(), "{}");
    //     c.import(&updates_a123).unwrap();
    //     assert_eq!(c.get_deep_value(), a.get_deep_value());
    // }
}

use crate::{
    arena::OpConverter, change::Change, encoding::RemoteClientChanges, op::RemoteOp, OpLog,
    VersionVector,
};
use fxhash::FxHashMap;
use itertools::Itertools;
use loro_common::{
    Counter, CounterSpan, HasCounterSpan, HasIdSpan, HasLamportSpan, Lamport, LoroError, ID,
};
use rle::{HasLength, RleVec};

use super::AppDagNode;

#[derive(Debug, Default)]
pub(crate) struct PendingChanges {
    pub(crate) pending_changes: FxHashMap<ID, Vec<Change>>,
    pub(crate) pending_unknown_lamport_changes: FxHashMap<ID, Change>,
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

    pub(super) fn try_apply_pending(&mut self, id: ID, latest_vv: &mut VersionVector) {
        let mut id_stack = vec![id];
        while let Some(id) = id_stack.pop() {
            if let Some(may_apply_changes) = self.pending_changes.pending_changes.remove(&id) {
                let mut may_apply_iter = may_apply_changes
                    .into_iter()
                    .sorted_unstable_by_key(|a| a.lamport)
                    .peekable();
                while let Some(peek_c) = may_apply_iter.peek() {
                    match remote_change_apply_state(latest_vv, peek_c) {
                        ChangeApplyState::Directly => {
                            let c = may_apply_iter.next().unwrap();
                            let last_id = c.id_last();
                            latest_vv.set_end(c.id_end());
                            self.apply_local_change_from_remote(c);
                            // other pending
                            id_stack.push(last_id);
                        }
                        ChangeApplyState::Existing => {
                            may_apply_iter.next();
                        }
                        ChangeApplyState::Future(id) => {
                            self.pending_changes
                                .pending_changes
                                .entry(id)
                                .or_insert_with(Vec::new)
                                .extend(may_apply_iter);
                            break;
                        }
                    }
                }
            }
            if let Some(mut unknown_lamport_change) = self
                .pending_changes
                .pending_unknown_lamport_changes
                .remove(&id)
            {
                match remote_change_apply_state(latest_vv, &unknown_lamport_change) {
                    ChangeApplyState::Directly => {
                        let last_id = unknown_lamport_change.id_last();
                        latest_vv.set_end(unknown_lamport_change.id_end());
                        self.dag
                            .calc_unknown_lamport_change(&mut unknown_lamport_change)
                            .unwrap();
                        self.apply_local_change_from_remote(unknown_lamport_change);
                        id_stack.push(last_id);
                    }
                    ChangeApplyState::Existing => unreachable!(),
                    ChangeApplyState::Future(id) => {
                        self.pending_changes
                            .pending_unknown_lamport_changes
                            .insert(id, unknown_lamport_change);
                    }
                }
            }
        }
    }

    pub(super) fn apply_local_change_from_remote(&mut self, change: Change) {
        self.next_lamport = self.next_lamport.max(change.lamport_end());
        // debug_dbg!(&change_causal_arr);
        self.dag.vv.extend_to_include_last_id(change.id_last());
        self.latest_timestamp = self.latest_timestamp.max(change.timestamp);

        let len = change.content_len();
        if change.deps.len() == 1 && change.deps[0].peer == change.id.peer {
            // don't need to push new element to dag because it only depends on itself
            let nodes = self.dag.map.get_mut(&change.id.peer).unwrap();
            let last = nodes.vec_mut().last_mut().unwrap();
            assert_eq!(last.peer, change.id.peer);
            assert_eq!(last.cnt + last.len as Counter, change.id.counter);
            assert_eq!(last.lamport + last.len as Lamport, change.lamport);
            last.len = change.id.counter as usize + len - last.cnt as usize;
            last.has_succ = false;
        } else {
            let vv = self.dag.frontiers_to_im_vv(&change.deps);
            self.dag
                .map
                .entry(change.id.peer)
                .or_default()
                .push(AppDagNode {
                    vv,
                    peer: change.id.peer,
                    cnt: change.id.counter,
                    lamport: change.lamport,
                    deps: change.deps.clone(),
                    has_succ: false,
                    len,
                });
            for dep in change.deps.iter() {
                let target = self.dag.get_mut(*dep).unwrap();
                if target.ctr_last() == dep.counter {
                    target.has_succ = true;
                }
            }
        }
        self.changes.entry(change.id.peer).or_default().push(change);
    }
}

pub(super) fn to_local_op(change: Change<RemoteOp>, converter: &mut OpConverter) -> Change {
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
}

pub enum ChangeApplyState {
    Existing,
    Directly,
    // The id of first missing dep
    Future(ID),
}

pub(super) fn remote_change_apply_state(vv: &VersionVector, change: &Change) -> ChangeApplyState {
    let peer = change.id.peer;
    let CounterSpan { start, end } = change.ctr_span();
    let vv_latest_ctr = vv.get(&peer).copied().unwrap_or(0);
    if vv_latest_ctr < start {
        return ChangeApplyState::Future(change.id.inc(-1));
    }
    if vv_latest_ctr >= end {
        return ChangeApplyState::Existing;
    }
    for dep in change.deps.as_ref().iter() {
        let dep_vv_latest_ctr = vv.get(&dep.peer).copied().unwrap_or(0);
        if dep_vv_latest_ctr - 1 < dep.counter {
            return ChangeApplyState::Future(*dep);
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
        a.set_peer_id(1);
        let b = LoroDoc::new();
        b.set_peer_id(2);
        let c = LoroDoc::new();
        c.set_peer_id(3);
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

use crate::change::Change;
use crate::hierarchy::Hierarchy;
use crate::id::{ClientID, Counter, ID};
use crate::op::RemoteOp;
use crate::span::{CounterSpan, HasCounter, HasCounterSpan};
use crate::version::PatchedVersionVector;
use crate::LogStore;
use crate::{
    container::registry::ContainerIdx,
    event::{Diff, RawEvent},
    version::{Frontiers, IdSpanVector},
};
use itertools::Itertools;
use smallvec::{smallvec, SmallVec};
use std::sync::Arc;
use std::{collections::VecDeque, sync::MutexGuard};
use tracing::instrument;

use fxhash::{FxHashMap, FxHashSet};

use rle::{slice_vec_by, HasLength, RleVecWithIndex, Sliceable};

use crate::{
    container::{registry::ContainerInstance, ContainerID, ContainerTrait},
    dag::{remove_included_frontiers, DagUtils},
    op::RichOp,
    span::{HasIdSpan, HasLamportSpan, IdSpan},
    version::are_frontiers_eq,
    VersionVector,
};

use super::{ContainerGuard, RemoteClientChanges};

#[derive(Debug)]
pub struct ImportContext {
    pub old_frontiers: Frontiers,
    pub new_frontiers: Frontiers,
    pub old_vv: VersionVector,
    pub patched_old_vv: Option<PatchedVersionVector>,
    pub new_vv: VersionVector,
    pub spans: IdSpanVector,
    pub diff: Vec<(ContainerID, SmallVec<[Diff; 1]>)>,
}

impl ImportContext {
    pub fn push_diff(&mut self, id: &ContainerID, diff: Diff) {
        if let Some((last_id, vec)) = self.diff.last_mut() {
            if last_id == id {
                vec.push(diff);
                return;
            }
        }

        self.diff.push((id.clone(), smallvec![diff]));
    }

    pub fn push_diff_vec(&mut self, id: &ContainerID, mut diff: SmallVec<[Diff; 1]>) {
        if let Some((last_id, vec)) = self.diff.last_mut() {
            if last_id == id {
                vec.append(&mut diff);
                return;
            }
        }

        self.diff.push((id.clone(), diff));
    }
}

impl LogStore {
    /// Import remote clients' changes into the local log store.
    ///
    /// # How does it work
    ///
    /// > The core algorithm is in the [`LogStore::apply`] method.
    ///
    /// - First, we remove all the changes that are already included in the local log store.
    /// And cache the changes whose dependencies are not included.
    /// - Then, we append the changes to the local log store.
    /// - Apply
    ///   - Check whether we can apply the change directly by testing whether self.frontiers == common ancestors.
    ///     If so, we apply the change directly and return.
    ///   - Otherwise
    ///     - Stage 1: we iterate over the new changes by causal order, and record them to the tracker.
    ///     - Stage 2: we calculate the effects of the new changes, and apply them to the state.
    /// - Update the rest of the log store state.
    ///
    #[instrument(skip_all)]
    pub(crate) fn import(
        &mut self,
        hierarchy: &mut Hierarchy,
        changes: RemoteClientChanges,
    ) -> Vec<RawEvent> {
        let changes = self.process_and_queue_changes(changes);
        if changes.is_empty() {
            return vec![];
        }
        debug_log::debug_dbg!(&changes);
        let mut container_map: FxHashMap<ContainerID, ContainerGuard> = Default::default();
        self.lock_related_containers(&changes, &mut container_map);
        let (next_vv, next_frontiers) = self.push_changes(changes, &mut container_map);
        let container_map: FxHashMap<ContainerIdx, ContainerGuard> = container_map
            .into_iter()
            .map(|(k, v)| (self.reg.get_idx(&k).unwrap(), v))
            .collect();
        let mut context = ImportContext {
            old_frontiers: self.frontiers.iter().copied().collect(),
            new_frontiers: next_frontiers.get_frontiers(),
            old_vv: self.vv.clone(),
            spans: next_vv.diff(&self.vv).left,
            new_vv: next_vv,
            diff: Default::default(),
            patched_old_vv: None,
        };
        hierarchy.take_deleted();

        debug_log::group!("apply to {}", self.this_client_id);
        self.apply(hierarchy, container_map, &mut context);
        debug_log::group_end!();

        let events = self.get_events(hierarchy, &mut context);
        self.update_version_info(context.new_vv, next_frontiers);
        events
    }

    // TODO: add doc
    #[instrument(skip_all)]
    pub(crate) fn get_events(
        &mut self,
        hierarchy: &mut Hierarchy,
        context: &mut ImportContext,
    ) -> Vec<RawEvent> {
        let deleted = hierarchy.take_deleted();
        let mut events = Vec::with_capacity(context.diff.len());
        let h = hierarchy;
        let reg = &self.reg;
        for (id, diff) in std::mem::take(&mut context.diff)
            .into_iter()
            .filter(|x| !deleted.contains(&x.0))
        {
            let Some(abs_path) = h.get_abs_path(reg, &id) else {
                continue;
            };
            let raw_event = RawEvent {
                abs_path,
                diff,
                container_id: id,
                old_version: context.old_frontiers.clone(),
                new_version: context.new_frontiers.clone(),
                local: false,
                origin: None,
            };
            events.push(raw_event);
        }

        // notify event in the order of path length
        // otherwise, the paths to children may be incorrect when the parents are affected by some of the events
        events.sort_by_cached_key(|x| x.abs_path.len());
        events
    }

    fn update_version_info(&mut self, next_vv: VersionVector, next_frontiers: VersionVector) {
        self.vv = next_vv;
        self.frontiers = next_frontiers.get_frontiers();
        self.latest_lamport = self
            .changes
            .values()
            .map(|v| v.last().unwrap().lamport_last())
            .max()
            .unwrap();
        self.latest_timestamp = self
            .changes
            .values()
            .map(|v| v.last().unwrap().timestamp)
            .max()
            .unwrap();
    }

    fn push_changes(
        &mut self,
        changes: RemoteClientChanges,
        container_map: &mut FxHashMap<ContainerID, MutexGuard<ContainerInstance>>,
    ) -> (VersionVector, VersionVector) {
        let mut next_vv: VersionVector = self.vv.clone();
        let mut next_frontiers: VersionVector = self.frontiers.iter().copied().collect();
        for (_, changes) in changes.iter() {
            next_frontiers.set_end(changes.last().unwrap().id_end());
            next_vv.set_end(changes.last().unwrap().id_end());
        }
        // push changes to log stores
        let cfg = self.get_change_merge_cfg();
        for (client_id, changes) in changes.iter() {
            let mut inner_changes = Vec::with_capacity(changes.len());
            for change in changes.iter() {
                remove_included_frontiers(&mut next_frontiers, &change.deps);
                let change = self.change_to_imported_format(change, container_map);
                inner_changes.push(change);
            }

            let rle = self
                .changes
                .entry(*client_id)
                .or_insert_with(|| RleVecWithIndex::new_cfg(cfg.clone()));
            for change in inner_changes {
                // if let Some(last) = rle.last() {
                //     assert_eq!(
                //         last.id.counter + last.atom_len() as Counter,
                //         change.id.counter
                //     )
                // }
                rle.push(change);
            }
        }
        (next_vv, next_frontiers)
    }

    #[instrument(skip_all)]
    pub(crate) fn apply(
        &mut self,
        hierarchy: &mut Hierarchy,
        mut container_map: FxHashMap<ContainerIdx, MutexGuard<ContainerInstance>>,
        context: &mut ImportContext,
    ) {
        let latest_frontiers = &context.new_frontiers;
        let common_ancestors = self.find_common_ancestor(&self.frontiers, latest_frontiers);
        if are_frontiers_eq(&common_ancestors, &self.frontiers) {
            // we may apply changes directly into state
            let target_spans = context.new_vv.diff(&self.vv).left;
            if target_spans.len() == 1 {
                let (client_id, span) = target_spans.iter().next().unwrap();
                for op in self.iter_ops_at_id_span(IdSpan::new(*client_id, span.start, span.end)) {
                    let container = container_map.get_mut(&op.op().container).unwrap();
                    container.update_state_directly(hierarchy, &op, context);
                }
                return;
            }

            let can_skip = {
                // TODO: can reuse this path
                let causal_visit_path: Vec<_> =
                    self.iter_causal(&common_ancestors, target_spans).collect();
                if causal_visit_path
                    .iter()
                    .all(|x| x.retreat.is_empty() && x.forward.is_empty())
                {
                    // can update containers state directly without consulting CRDT
                    for iter in causal_visit_path {
                        let start = iter.slice.start;
                        let end = iter.slice.end;
                        let change = iter.data;

                        for op in change.ops.iter() {
                            let rich_op = RichOp::new_by_slice_on_change(change, start, end, op);
                            if rich_op.atom_len() == 0 {
                                continue;
                            }

                            let container = container_map.get_mut(&op.container).unwrap();
                            container.update_state_directly(hierarchy, &rich_op, context);
                        }
                    }
                    true
                } else {
                    false
                }
            };

            if can_skip {
                return;
            }
        }

        let mut common_ancestors_vv = self.vv.clone();
        common_ancestors_vv.retreat(&self.find_path(&common_ancestors, &self.frontiers).right);
        let iter_targets = context.new_vv.sub_vec(&common_ancestors_vv);
        let common_ancestors_vv = Arc::new(common_ancestors_vv);
        context.patched_old_vv = Some(PatchedVersionVector::from_version(
            &common_ancestors_vv,
            &context.old_vv,
        ));
        let common_ancestors_vv = PatchedVersionVector::new(common_ancestors_vv);
        for (_, container) in container_map.iter_mut() {
            container.tracker_init(&common_ancestors_vv);
        }

        let mut current_vv = common_ancestors_vv;
        let mut already_checkout = FxHashSet::default();
        for iter in self.iter_causal(&common_ancestors, iter_targets) {
            debug_log::debug_dbg!(&iter);
            debug_log::debug_dbg!(&current_vv);
            already_checkout.clear();
            let start = iter.slice.start;
            let end = iter.slice.end;
            let change = iter.data;
            current_vv.retreat(&iter.retreat);
            current_vv.forward(&iter.forward);

            debug_log::debug_dbg!(&current_vv);
            for op in change.ops.iter() {
                let rich_op = RichOp::new_by_slice_on_change(change, start, end, op);
                if rich_op.atom_len() == 0 {
                    continue;
                }

                debug_log::debug_dbg!(&rich_op);
                if let Some(container) = container_map.get_mut(&op.container) {
                    if !already_checkout.contains(&op.container) {
                        already_checkout.insert(op.container);
                        container.tracker_checkout(&current_vv);
                    }

                    container.track_apply(hierarchy, &rich_op, context);
                }
            }

            current_vv.set_end(ID::new(
                change.id.client_id,
                end as Counter + change.id.counter,
            ));
        }
        debug_log::group!("apply effects");
        let mut queue: VecDeque<_> = container_map.into_values().collect();
        let mut retries = 0;
        // only apply the effects of a container when it's registered to the hierarchy
        while let Some(mut container) = queue.pop_back() {
            if container.id().is_root() || hierarchy.contains(container.id()) {
                retries = 0;
                container.apply_tracked_effects_from(hierarchy, context);
            } else {
                retries += 1;
                queue.push_front(container);
                if retries > queue.len() {
                    // the left containers are deleted
                    debug_log::debug_log!("Left containers are deleted");
                    debug_log::debug_dbg!(&queue);
                    break;
                }
            }
        }

        for mut container in queue {
            container.apply_tracked_effects_from(hierarchy, context);
        }

        debug_log::group_end!();
    }

    /// get the locks of the containers to avoid repeated acquiring and releasing the locks
    fn lock_related_containers(
        &mut self,
        changes: &RemoteClientChanges,
        container_map: &mut FxHashMap<ContainerID, MutexGuard<ContainerInstance>>,
    ) {
        for (_, changes) in changes.iter() {
            for change in changes.iter() {
                for op in change.ops.iter() {
                    if !container_map.contains_key(&op.container) {
                        let guard = self.reg.get_or_create(&op.container).upgrade().unwrap();
                        let guard = guard.try_lock().unwrap();
                        container_map
                            // SAFETY: ignore lifetime issues here, because it's safe for us to store the mutex guard here
                            .insert(op.container.clone(), unsafe { std::mem::transmute(guard) });
                    }
                }
            }
        }
    }

    fn process_and_queue_changes(
        &mut self,
        mut changes: RemoteClientChanges,
    ) -> RemoteClientChanges {
        let mut latest_vv = self.get_vv().clone();
        let mut retain_changes = FxHashMap::default();

        if changes.values().map(|c| c.len()).sum::<usize>() == 0 {
            // snapshot
            let client_ids: Vec<_> = latest_vv.keys().copied().collect();
            for client_id in client_ids {
                // let counter = latest_vv.get_last(client_id).unwrap();
                self.try_apply_pending(&client_id, &mut latest_vv, &mut retain_changes)
            }
        } else {
            // Changes will be sorted by lamport. If the first change cannot be applied, then all subsequent changes with the same client id cannot be applied either.
            // we cache these client id.
            // self.tailor_changes(&mut changes);
            let mut pending_clients = FxHashSet::default();
            changes
                .into_values()
                .flat_map(|c| c.into_iter())
                // sort changes by lamport from small to large
                .sorted_by(|a, b| a.lamport.cmp(&b.lamport))
                .for_each(|mut c| {
                    let c_client_id = c.id.client_id;
                    if pending_clients.contains(&c_client_id) {
                        self.pending_changes.get_mut(&c_client_id).unwrap().push(c);
                        return;
                    }
                    match can_remote_change_be_applied(&latest_vv, &mut c) {
                        ChangeApplyState::Directly => {
                            latest_vv.set_end(c.id_end());
                            retain_changes
                                .entry(c_client_id)
                                .or_insert_with(Vec::new)
                                .push(c);
                            self.try_apply_pending(
                                &c_client_id,
                                &mut latest_vv,
                                &mut retain_changes,
                            );
                        }
                        ChangeApplyState::Existing => {}
                        ChangeApplyState::Future(this_dep_client) => {
                            pending_clients.insert(c_client_id);
                            self.pending_changes
                                .entry(this_dep_client)
                                .or_insert_with(Vec::new)
                                .push(c);
                        }
                    }
                });
        }
        retain_changes
    }

    fn try_apply_pending(
        &mut self,
        client_id: &ClientID,
        latest_vv: &mut VersionVector,
        retain_changes: &mut RemoteClientChanges,
    ) {
        if let Some(may_apply_changes) = self.pending_changes.remove(client_id) {
            let mut may_apply_iter = may_apply_changes
                .into_iter()
                .sorted_by(|a, b| a.lamport.cmp(&b.lamport))
                .peekable();
            while let Some(peek_c) = may_apply_iter.peek_mut() {
                match can_remote_change_be_applied(latest_vv, peek_c) {
                    ChangeApplyState::Directly => {
                        let c = may_apply_iter.next().unwrap();
                        let c_client_id = c.id.client_id;
                        latest_vv.set_end(c.id_end());
                        // other pending
                        retain_changes
                            .entry(c_client_id)
                            .or_insert_with(Vec::new)
                            .push(c);
                        self.try_apply_pending(&c_client_id, latest_vv, retain_changes);
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

    fn tailor_changes(&mut self, changes: &mut RemoteClientChanges) {
        changes.retain(|_, v| !v.is_empty());
        for (client_id, changes) in changes.iter_mut() {
            let self_end_ctr = self.vv.get(client_id).copied().unwrap_or(0);
            let other_start_ctr = changes.first().unwrap().ctr_start();
            match other_start_ctr.cmp(&self_end_ctr) {
                std::cmp::Ordering::Less => {
                    *changes = slice_vec_by(
                        changes,
                        |x| x.id.counter as usize,
                        self_end_ctr as usize,
                        usize::MAX,
                    );
                }
                std::cmp::Ordering::Equal => {}
                std::cmp::Ordering::Greater => {}
            }
        }
        changes.retain(|_, v| !v.is_empty());
    }
}

#[derive(Debug)]
enum ChangeApplyState {
    Existing,
    Directly,
    /// The client id of first missing dep
    Future(ClientID),
}

fn can_remote_change_be_applied(
    vv: &VersionVector,
    change: &mut Change<RemoteOp>,
) -> ChangeApplyState {
    let change_client_id = change.id.client_id;
    let CounterSpan { start, end } = change.ctr_span();
    let vv_latest_ctr = vv.get(&change_client_id).copied().unwrap_or(0);
    if vv_latest_ctr < start {
        return ChangeApplyState::Future(change_client_id);
    }
    if vv_latest_ctr >= end || start == end {
        return ChangeApplyState::Existing;
    }
    for dep in &change.deps {
        let dep_vv_latest_ctr = vv.get(&dep.client_id).copied().unwrap_or(0);
        if dep_vv_latest_ctr - 1 < dep.counter {
            return ChangeApplyState::Future(dep.client_id);
        }
    }

    if start < vv_latest_ctr {
        *change = change.slice((vv_latest_ctr - start) as usize, (end - start) as usize);
    }

    ChangeApplyState::Directly
}

#[cfg(test)]
mod test {
    use crate::{LoroCore, Transact, VersionVector};

    #[test]
    fn import_pending() {
        let mut a = LoroCore::new(Default::default(), Some(1));
        let mut b = LoroCore::new(Default::default(), Some(2));
        let mut text_a = a.get_text("text");
        text_a.insert(&a, 0, "a").unwrap();
        let update1 = a.encode_from(VersionVector::new());
        let version1 = a.vv_cloned();
        text_a.insert(&a, 0, "b").unwrap();
        let update2 = a.encode_from(version1);
        let version2 = a.vv_cloned();
        text_a.insert(&a, 0, "c").unwrap();
        let update3 = a.encode_from(version2.clone());
        let version3 = a.vv_cloned();
        text_a.insert(&a, 0, "d").unwrap();
        let update4 = a.encode_from(version3);
        // let version4 = a.vv_cloned();
        text_a.insert(&a, 0, "e").unwrap();
        let update3_5 = a.encode_from(version2);
        b.decode(&update3_5).unwrap();
        b.decode(&update4).unwrap();
        b.decode(&update2).unwrap();
        b.decode(&update3).unwrap();
        b.decode(&update1).unwrap();
        assert_eq!(a.to_json(), b.to_json());
    }

    #[test]
    fn pending_import_snapshot() {
        let mut a = LoroCore::new(Default::default(), Some(1));
        let mut b = LoroCore::new(Default::default(), Some(2));
        let mut text_a = a.get_text("text");

        text_a.insert(&a, 0, "a").unwrap();
        let update1 = a.encode_all();
        let version1 = a.vv_cloned();
        text_a.insert(&a, 1, "b").unwrap();
        let update2 = a.encode_from(version1);
        let _version2 = a.vv_cloned();
        b.decode(&update2).unwrap();
        b.decode(&update1).unwrap();
        assert_eq!(a.to_json(), b.to_json());
    }

    #[test]
    #[cfg(feature = "json")]
    fn need_deps_pending_import() {
        // a:   a1 <--- a2
        //        \    /
        // b:       b1
        let mut a = LoroCore::new(Default::default(), Some(1));
        let mut b = LoroCore::new(Default::default(), Some(2));
        let mut c = LoroCore::new(Default::default(), Some(3));
        let mut d = LoroCore::new(Default::default(), Some(4));
        let mut text_a = a.get_text("text");
        let mut text_b = b.get_text("text");
        text_a.insert(&a, 0, "a").unwrap();
        let version_a1 = a.vv_cloned();
        let update_a1 = a.encode_from(VersionVector::new());
        b.decode(&update_a1).unwrap();
        text_b.insert(&b, 1, "b").unwrap();
        let update_b1 = b.encode_from(version_a1);
        a.decode(&update_b1).unwrap();
        let version_a1b1 = a.vv_cloned();
        text_a.insert(&a, 2, "c").unwrap();
        let update_a2 = a.encode_from(version_a1b1);
        c.decode(&update_a2).unwrap();
        assert_eq!(c.to_json().to_json(), "{}");
        c.decode(&update_a1).unwrap();
        assert_eq!(c.to_json().to_json(), "{\"text\":\"a\"}");
        c.decode(&update_b1).unwrap();
        assert_eq!(a.to_json(), c.to_json());

        d.decode(&update_a2).unwrap();
        assert_eq!(d.to_json().to_json(), "{}");
        d.decode(&update_b1).unwrap();
        assert_eq!(d.to_json().to_json(), "{}");
        d.decode(&update_a1).unwrap();
        assert_eq!(a.to_json(), d.to_json());
    }

    #[test]
    fn should_activate_pending_change_when() {
        // 0@a <- 0@b
        // 0@a <- 1@a, where 0@a and 1@a will be merged
        // In this case, c apply b's change first, then apply all the changes from a.
        // C is expected to have the same content as a, after a imported b's change
        let mut a = LoroCore::new(Default::default(), Some(1));
        let mut b = LoroCore::new(Default::default(), Some(2));
        let mut c = LoroCore::new(Default::default(), Some(3));
        let mut text_a = a.get_text("text");
        let mut text_b = b.get_text("text");
        text_a.insert(&a, 0, "1").unwrap();
        b.decode(&a.encode_all()).unwrap();
        text_b.insert(&b, 0, "1").unwrap();
        let b_change = b.encode_from(a.vv_cloned());
        text_a.insert(&a, 0, "1").unwrap();
        c.decode(&b_change).unwrap();
        c.decode(&a.encode_all()).unwrap();
        a.decode(&b_change).unwrap();
        assert_eq!(c.to_json(), a.to_json());
    }

    #[test]
    #[cfg(feature = "json")]
    fn pending_changes_may_deps_merged_change() {
        // a:  (a1 <-- a2 <-- a3) <-- a4       a1~a3 is a merged change
        //                \         /
        // b:                b1
        let mut a = LoroCore::new(Default::default(), Some(1));
        let mut b = LoroCore::new(Default::default(), Some(2));
        let mut c = LoroCore::new(Default::default(), Some(3));
        let mut text_a = a.get_text("text");
        let mut text_b = b.get_text("text");
        text_a.insert(&a, 0, "a").unwrap();
        text_a.insert(&a, 1, "b").unwrap();
        let version_a12 = a.vv_cloned();
        let updates_a12 = a.encode_all();
        text_a.insert(&a, 2, "c").unwrap();
        let updates_a123 = a.encode_all();
        b.decode(&updates_a12).unwrap();
        text_b.insert(&b, 2, "d").unwrap();
        let update_b1 = b.encode_from(version_a12);
        a.decode(&update_b1).unwrap();
        let version_a123_b1 = a.vv_cloned();
        text_a.insert(&a, 4, "e").unwrap();
        let update_a4 = a.encode_from(version_a123_b1);
        c.decode(&update_b1).unwrap();
        assert_eq!(c.to_json().to_json(), "{}");
        c.decode(&update_a4).unwrap();
        assert_eq!(c.to_json().to_json(), "{}");
        c.decode(&updates_a123).unwrap();
        assert_eq!(c.to_json(), a.to_json());
    }

    #[test]
    fn applied_change_filter() {
        let mut a = LoroCore::new(Default::default(), Some(1));
        let mut b = LoroCore::new(Default::default(), Some(2));
        let mut list_a = a.get_list("list");
        let mut list_b = b.get_list("list");
        {
            let txn = a.transact();
            list_a.insert(&txn, 0, "1").unwrap();
            list_a.insert(&txn, 1, "1").unwrap();
        }
        b.decode(&a.encode_from(Default::default())).unwrap();
        {
            let txn = a.transact();
            list_a.insert(&txn, 2, "1").unwrap();
            list_a.insert(&txn, 3, "1").unwrap();
        }
        b.decode(&a.encode_from(Default::default())).unwrap();
    }
}

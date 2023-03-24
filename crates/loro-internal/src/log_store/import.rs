use crate::change::Change;
use crate::hierarchy::Hierarchy;
use crate::id::{ClientID, Counter, ID};
use crate::op::RemoteOp;
use crate::version::PatchedVersionVector;
use crate::LogStore;
use crate::{
    container::registry::ContainerIdx,
    event::{Diff, RawEvent},
    version::{Frontiers, IdSpanVector},
};
use smallvec::{smallvec, SmallVec};
use std::collections::BinaryHeap;
use std::sync::Arc;
use std::{collections::VecDeque, sync::MutexGuard};
use tracing::instrument;

use fxhash::{FxHashMap, FxHashSet};

use rle::{slice_vec_by, HasLength, RleVecWithIndex};

use crate::{
    container::{registry::ContainerInstance, ContainerID, ContainerTrait},
    dag::{remove_included_frontiers, DagUtils},
    op::RichOp,
    span::{HasCounter, HasIdSpan, HasLamportSpan, IdSpan},
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
        let changes = self.tailor_changes(changes);
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

    fn tailor_changes(&mut self, mut changes: RemoteClientChanges) -> RemoteClientChanges {
        // cancel filter empty changes, snapshot can use empty changes to check pending changes
        // changes.retain(|_, v| !v.is_empty());
        for (client_id, changes) in changes.iter_mut() {
            self.filter_changes(client_id, changes);
        }
        changes.retain(|_, v| !v.is_empty());
        changes
    }

    fn filter_changes(&mut self, client_id: &ClientID, changes: &mut Vec<Change<RemoteOp>>) {
        let self_end_ctr = self.vv.get(client_id).copied().unwrap_or(0);
        if let Some(first_change) = changes.first() {
            let other_start_ctr = first_change.ctr_start();
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
                std::cmp::Ordering::Greater => {
                    let pending_changes = std::mem::take(changes);
                    self.pending_changes
                        .entry(*client_id)
                        .or_insert_with(BinaryHeap::new)
                        .push(ChangesWithNegStartCounter {
                            start_ctr: pending_changes.first().unwrap().ctr_start(),
                            changes: pending_changes,
                        })
                }
            }
        }

        // check whether the pending changes can be imported
        let mut latest_end_ctr = self_end_ctr + changes.content_len() as i32;
        if let Some(pending_heap) = self.pending_changes.get_mut(client_id) {
            while let Some(ChangesWithNegStartCounter {
                start_ctr,
                changes: pending_changes,
            }) = pending_heap.pop()
            {
                match start_ctr.cmp(&latest_end_ctr) {
                    std::cmp::Ordering::Less => {
                        let rest_changes = slice_vec_by(
                            &pending_changes,
                            |x| x.id.counter as usize,
                            latest_end_ctr as usize,
                            usize::MAX,
                        );
                        latest_end_ctr += rest_changes.content_len() as i32;
                        changes.extend(rest_changes);
                    }
                    std::cmp::Ordering::Equal => {
                        latest_end_ctr += pending_changes.content_len() as i32;
                        changes.extend(pending_changes);
                    }
                    std::cmp::Ordering::Greater => {
                        pending_heap.push(ChangesWithNegStartCounter {
                            start_ctr,
                            changes: pending_changes,
                        });
                        break;
                    }
                }
            }
        }
    }
}

#[derive(Debug)]
pub(crate) struct ChangesWithNegStartCounter {
    start_ctr: i32,
    changes: Vec<Change<RemoteOp>>,
}

impl PartialEq for ChangesWithNegStartCounter {
    fn eq(&self, other: &Self) -> bool {
        self.start_ctr.eq(&other.start_ctr)
    }
}

impl Eq for ChangesWithNegStartCounter {}

impl PartialOrd for ChangesWithNegStartCounter {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        (-self.start_ctr).partial_cmp(&-other.start_ctr)
    }
}

impl Ord for ChangesWithNegStartCounter {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (-self.start_ctr).cmp(&-other.start_ctr)
    }
}

#[cfg(test)]
mod test {
    use crate::{LoroCore, VersionVector};

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
        b.decode(&update1).unwrap();
        b.decode(&update3).unwrap();
        b.decode(&update2).unwrap();
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
}

use crate::LogStore;
use crate::{
    container::registry::ContainerIdx,
    event::{Diff, RawEvent},
    version::{Frontiers, IdSpanVector},
};
use std::{collections::VecDeque, ops::ControlFlow, sync::MutexGuard};
use tracing::instrument;

use fxhash::FxHashMap;

use rle::{slice_vec_by, HasLength, RleVecWithIndex};

use crate::{
    container::{registry::ContainerInstance, Container, ContainerID},
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
    pub new_vv: VersionVector,
    pub spans: IdSpanVector,
    pub diff: Vec<(ContainerID, Vec<Diff>)>,
}

impl ImportContext {
    pub fn push_diff(&mut self, id: &ContainerID, diff: Diff) {
        if let Some((last_id, vec)) = self.diff.last_mut() {
            if last_id == id {
                vec.push(diff);
                return;
            }
        }

        self.diff.push((id.clone(), vec![diff]));
    }

    pub fn push_diff_vec(&mut self, id: &ContainerID, mut diff: Vec<Diff>) {
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
    pub fn import(&mut self, mut changes: RemoteClientChanges) -> Vec<RawEvent> {
        if let ControlFlow::Break(_) = self.tailor_changes(&mut changes) {
            return vec![];
        }

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
        };
        self.with_hierarchy(|_, h| {
            h.take_deleted();
        });

        debug_log::group!("apply");
        self.apply(container_map, &mut context);
        debug_log::group_end!();

        let events = self.get_events(&mut context);
        self.update_version_info(context.new_vv, next_frontiers);
        events
    }

    #[instrument(skip_all)]
    fn get_events(&mut self, context: &mut ImportContext) -> Vec<RawEvent> {
        let deleted = self.with_hierarchy(|_, h| h.take_deleted());
        let mut events = Vec::with_capacity(context.diff.len());
        let h = self.hierarchy.try_lock().unwrap();
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
            .iter()
            .map(|(_, v)| v.last().unwrap().lamport_last())
            .max()
            .unwrap();
        self.latest_timestamp = self
            .changes
            .iter()
            .map(|(_, v)| v.last().unwrap().timestamp)
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
                self.with_hierarchy(|store, hierarchy| {
                    for op in
                        store.iter_ops_at_id_span(IdSpan::new(*client_id, span.start, span.end))
                    {
                        let container = container_map.get_mut(&op.op().container).unwrap();
                        container.update_state_directly(hierarchy, &op, context);
                    }
                });
                return;
            }

            let can_skip = self.with_hierarchy(|store, hierarchy| {
                // TODO: can reuse this path
                let causal_visit_path: Vec<_> =
                    store.iter_causal(&common_ancestors, target_spans).collect();
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
                            let rich_op = RichOp::new_by_slice_on_change(change, op, start, end);
                            if rich_op.atom_len() == 0 {
                                continue;
                            }

                            let container = container_map.get_mut(&op.container).unwrap();
                            container.update_state_directly(hierarchy, &rich_op, context);
                        }
                    }
                    return true;
                }

                false
            });

            if can_skip {
                return;
            }
        }

        let mut common_ancestors_vv = self.vv.clone();
        common_ancestors_vv.retreat(&self.find_path(&common_ancestors, &self.frontiers).right);
        for (_, container) in container_map.iter_mut() {
            container.tracker_checkout(&common_ancestors_vv);
        }
        self.with_hierarchy(|store, hierarchy| {
            for iter in store.iter_causal(
                &common_ancestors,
                context.new_vv.diff(&common_ancestors_vv).left,
            ) {
                let start = iter.slice.start;
                let end = iter.slice.end;
                let change = iter.data;
                // TODO: perf: we can make iter_causal returns target vv and only
                // checkout the related container to the target vv
                for (_, container) in container_map.iter_mut() {
                    container.track_retreat(&iter.retreat);
                    container.track_forward(&iter.forward);
                }

                for op in change.ops.iter() {
                    let rich_op = RichOp::new_by_slice_on_change(change, op, start, end);
                    if rich_op.atom_len() == 0 {
                        continue;
                    }

                    if let Some(container) = container_map.get_mut(&op.container) {
                        container.track_apply(hierarchy, &rich_op, context);
                    }
                }
            }
        });
        debug_log::group!("apply effects");
        let mut queue: VecDeque<_> = container_map.into_iter().map(|(_, x)| x).collect();
        let mut retries = 0;
        let mut h = self.hierarchy.try_lock().unwrap();
        // only apply the effects of a container when it's registered to the hierarchy
        while let Some(mut container) = queue.pop_back() {
            if container.id().is_root() || h.contains(container.id()) {
                retries = 0;
                container.apply_tracked_effects_from(&mut h, context);
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
            container.apply_tracked_effects_from(&mut h, context);
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
                        let guard = self.reg.get_or_create(&op.container).lock().unwrap();
                        container_map
                            // SAFETY: ignore lifetime issues here, because it's safe for us to store the mutex guard here
                            .insert(op.container.clone(), unsafe { std::mem::transmute(guard) });
                    }
                }
            }
        }
    }

    fn tailor_changes(&mut self, changes: &mut RemoteClientChanges) -> ControlFlow<()> {
        changes.retain(|_, v| !v.is_empty());
        if changes.is_empty() {
            return ControlFlow::Break(());
        }
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
                std::cmp::Ordering::Greater => {
                    unimplemented!("cache pending changes");
                }
            }
        }
        changes.retain(|_, v| !v.is_empty());
        ControlFlow::Continue(())
    }
}

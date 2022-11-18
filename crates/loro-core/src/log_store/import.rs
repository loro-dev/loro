use crate::LogStore;
use std::{ops::ControlFlow, sync::MutexGuard};

use fxhash::FxHashMap;

use rle::{HasLength, RleVecWithIndex, Sliceable};

use crate::{
    container::{registry::ContainerInstance, Container, ContainerID},
    dag::{remove_included_frontiers, DagUtils},
    debug_log,
    id::ContainerIdx,
    op::RichOp,
    span::{HasCounter, HasIdSpan, HasLamportSpan, IdSpan},
    version::are_frontiers_eq,
    VersionVector,
};

use super::{ContainerGuard, RemoteClientChanges};

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
    pub fn import(&mut self, mut changes: RemoteClientChanges) {
        if let ControlFlow::Break(_) = self.tailor_changes(&mut changes) {
            return;
        }

        let mut container_map: FxHashMap<ContainerID, ContainerGuard> = Default::default();
        self.lock_related_containers(&changes, &mut container_map);
        let (next_vv, next_frontiers) = self.push_changes(changes, &mut container_map);
        let container_map: FxHashMap<ContainerIdx, ContainerGuard> = container_map
            .into_iter()
            .map(|(k, v)| (self.reg.get_idx(&k).unwrap(), v))
            .collect();

        self.apply(&next_frontiers, &next_vv, container_map);
        self.update_version_info(next_vv, next_frontiers);
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

    fn apply(
        &mut self,
        next_frontiers: &VersionVector,
        next_vv: &VersionVector,
        mut container_map: FxHashMap<u32, MutexGuard<ContainerInstance>>,
    ) {
        let latest_frontiers = next_frontiers.get_frontiers();
        debug_log!(
            "FIND COMMON ANCESTORS self={:?} latest={:?}",
            &self.frontiers,
            &latest_frontiers
        );
        let common_ancestors = self.find_common_ancestor(&self.frontiers, &latest_frontiers);
        if are_frontiers_eq(&common_ancestors, &self.frontiers) {
            // we may apply changes directly into state
            let target_spans = next_vv.diff(&self.vv).left;
            if target_spans.len() == 1 {
                let (client_id, span) = target_spans.iter().next().unwrap();
                for op in self.iter_ops_at_id_span(IdSpan::new(*client_id, span.start, span.end)) {
                    let container = container_map.get_mut(&op.op().container).unwrap();
                    container.update_state_directly(&op);
                }

                return;
            }

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
                        let rich_op = RichOp::new_by_slice_on_change(change, op, start, end);
                        if rich_op.atom_len() == 0 {
                            continue;
                        }

                        let container = container_map.get_mut(&op.container).unwrap();
                        container.update_state_directly(&rich_op);
                    }
                }

                return;
            }
        }

        let mut common_ancestors_vv = self.vv.clone();
        common_ancestors_vv.retreat(&self.find_path(&common_ancestors, &self.frontiers).right);
        for (_, container) in container_map.iter_mut() {
            container.tracker_checkout(&common_ancestors_vv);
        }
        for iter in self.iter_causal(&common_ancestors, next_vv.diff(&common_ancestors_vv).left) {
            let start = iter.slice.start;
            let end = iter.slice.end;
            let change = iter.data;
            debug_log!("iter {:#?}", &iter);
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
                    container.track_apply(&rich_op);
                }
            }
        }
        debug_log!("LOGSTORE STAGE 2",);
        let path = next_vv.diff(&self.vv).left;
        for (_, container) in container_map.iter_mut() {
            container.apply_tracked_effects_from(&self.vv, &path);
        }
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
                    *changes = changes.slice(
                        (self_end_ctr - other_start_ctr) as usize,
                        changes.atom_len(),
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

use std::{num::NonZeroU16, sync::Arc};

#[cfg(feature = "counter")]
mod counter;
#[cfg(feature = "counter")]
pub(crate) use counter::CounterDiffCalculator;
pub(super) mod tree;
mod unknown;
use either::Either;
use generic_btree::rle::{HasLength as _, Sliceable as _};
use itertools::Itertools;

use enum_dispatch::enum_dispatch;
use loro_common::{
    CompactIdLp, ContainerID, Counter, HasCounterSpan, IdFull, IdLp, IdSpan, LoroValue, PeerID, ID,
};
use loro_delta::DeltaRope;
use rustc_hash::{FxHashMap, FxHashSet};
use smallvec::SmallVec;
use tracing::{info_span, instrument};

use crate::{
    change::Lamport,
    container::{
        idx::ContainerIdx,
        list::list_op::InnerListOp,
        richtext::{
            richtext_state::{RichtextStateChunk, TextChunk},
            AnchorType, CrdtRopeDelta, RichtextChunk, RichtextChunkValue, RichtextTracker, StyleOp,
        },
    },
    cursor::AbsolutePosition,
    delta::{
        Delta, DeltaItem, DeltaValue, ElementDelta, MapDelta, MapValue, MovableListInnerDelta,
    },
    event::{DiffVariant, InternalDiff},
    op::{InnerContent, RichOp, SliceRange, SliceWithId},
    span::{HasId, HasLamport},
    version::Frontiers,
    InternalString, VersionVector,
};

use self::tree::TreeDiffCalculator;

use self::unknown::UnknownDiffCalculator;

use super::{event::InternalContainerDiff, oplog::OpLog};

/// Calculate the diff between two versions. given [OpLog][super::oplog::OpLog]
/// and [AppState][super::state::AppState].
///
/// TODO: persist diffCalculator and skip processed version
#[derive(Debug)]
pub struct DiffCalculator {
    /// ContainerIdx -> (depth, calculator)
    ///
    /// if depth is None, we need to calculate it again
    calculators: FxHashMap<ContainerIdx, (Option<NonZeroU16>, ContainerDiffCalculator)>,
    retain_mode: DiffCalculatorRetainMode,
}

#[derive(Debug)]
enum DiffCalculatorRetainMode {
    /// The diff calculator can only be used once.
    Once { used: bool },
    /// The diff calculator will be persisted and can be reused after the diff calc is done.
    Persist,
}

/// This mode defines how the diff is calculated and how it should be applied on the state.
#[derive(Debug, Clone, PartialEq, Eq, Copy)]
pub(crate) enum DiffMode {
    /// This is the most general mode of diff calculation.
    ///
    /// When applying `Checkout` diff, we already know the current state of the affected registers.
    /// So there is no need to compare the lamport values.
    ///
    /// It can be used whenever a user want to switch to a different version.
    /// But it is also the slowest mode. It relies on the `ContainerHistoryCache`, which is expensive to build and maintain in memory.
    Checkout,
    /// This mode is used when the user imports new updates.
    ///
    /// When applying `Import` diff, we may need to know the the current state.
    /// For example, we may need to compare the current register's lamport with the update's lamport to decide
    /// what's the new value.
    ///
    /// It has stricter requirements than `Checkout`:
    ///
    /// - The target version vector must be greater than the current version vector.
    Import,
    /// This mode is used when the user imports new updates and all the updates are guaranteed to greater than the current version.
    ///
    /// It has stricter requirements than `Import`.
    /// - All the updates are greater than the current version. No update is concurrent to the current version.
    /// - So LCA is always the `from` version
    ImportGreaterUpdates,
    /// This mode is used when we don't need to build CRDTs to calculate the difference. It is the fastest mode.
    ///
    /// It has stricter requirements than `ImportGreaterUpdates`.
    /// - In `ImportGreaterUpdates`, all the updates are guaranteed to be greater than the current version.
    /// - In `Linear`, all the updates are ordered, no concurrent update exists.
    Linear,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct DiffCalcVersionInfo<'a> {
    from_vv: &'a VersionVector,
    to_vv: &'a VersionVector,
    from_frontiers: &'a Frontiers,
    to_frontiers: &'a Frontiers,
    lca_vv: &'a VersionVector,
}

impl DiffCalculator {
    /// Create a new diff calculator.
    ///
    /// If `persist` is true, the diff calculator will be persisted after the diff calc is done.
    /// This is useful when we need to cache the diff calculator for future use. But it is slower
    /// for importing updates and requires more memory.
    pub fn new(persist: bool) -> Self {
        Self {
            calculators: Default::default(),
            retain_mode: if persist {
                DiffCalculatorRetainMode::Persist
            } else {
                DiffCalculatorRetainMode::Once { used: false }
            },
        }
    }

    #[allow(unused)]
    pub(crate) fn get_calc(&self, container: ContainerIdx) -> Option<&ContainerDiffCalculator> {
        self.calculators.get(&container).map(|(_, c)| c)
    }

    /// Calculate the diff between two versions.
    ///
    /// Return the diff and the origin diff mode (it's not the diff mode used by the diff calculator.
    /// It's the expected diff mode inferred from the two version, which can reflect the direction of the
    /// change).
    pub(crate) fn calc_diff_internal(
        &mut self,
        oplog: &super::oplog::OpLog,
        before: &crate::VersionVector,
        before_frontiers: &Frontiers,
        after: &crate::VersionVector,
        after_frontiers: &Frontiers,
        container_filter: Option<&dyn Fn(ContainerIdx) -> bool>,
    ) -> (Vec<InternalContainerDiff>, DiffMode) {
        if before == after {
            return (Vec::new(), DiffMode::Linear);
        }

        let s = tracing::span!(tracing::Level::INFO, "DiffCalc", ?before, ?after,);
        let _e = s.enter();

        let mut merged = before.clone();
        merged.merge(after);
        let (lca, origin_diff_mode, iter) =
            oplog.iter_from_lca_causally(before, before_frontiers, after, after_frontiers);
        let mut diff_mode = origin_diff_mode;
        match &mut self.retain_mode {
            DiffCalculatorRetainMode::Once { used } => {
                if *used {
                    panic!("DiffCalculator with retain_mode Once can only be used once");
                }
            }
            DiffCalculatorRetainMode::Persist => {
                diff_mode = DiffMode::Checkout;
            }
        }

        let affected_set = {
            loro_common::debug!("LCA: {:?} mode={:?}", &lca, diff_mode);
            let mut started_set = FxHashSet::default();
            for (change, (start_counter, end_counter), vv) in iter {
                let iter_start = change
                    .ops
                    .binary_search_by(|op| op.ctr_last().cmp(&start_counter))
                    .unwrap_or_else(|e| e);
                let mut visited = FxHashSet::default();
                for mut op in &change.ops.vec()[iter_start..] {
                    if op.counter >= end_counter {
                        break;
                    }

                    let idx = op.container;
                    if let Some(filter) = container_filter {
                        if !filter(idx) {
                            continue;
                        }
                    }

                    // slice the op if needed
                    // PERF: we can skip the slice by using the RichOp::new_slice
                    let stack_sliced_op;
                    if op.ctr_last() < start_counter {
                        continue;
                    }

                    if op.counter < start_counter || op.ctr_end() > end_counter {
                        stack_sliced_op = Some(op.slice(
                            (start_counter as usize).saturating_sub(op.counter as usize),
                            op.atom_len().min((end_counter - op.counter) as usize),
                        ));
                        op = stack_sliced_op.as_ref().unwrap();
                    }

                    let vv = &mut vv.borrow_mut();
                    vv.extend_to_include_end_id(ID::new(change.peer(), op.counter));
                    let container = op.container;
                    let depth = oplog.arena.get_depth(container);
                    let (old_depth, calculator) = self.get_or_create_calc(container, depth);
                    // checkout use the same diff_calculator, the depth of calculator is not updated
                    // That may cause the container to be considered deleted
                    if *old_depth != depth {
                        *old_depth = depth;
                    }

                    if !started_set.contains(&op.container) {
                        started_set.insert(container);
                        calculator.start_tracking(oplog, &lca, diff_mode);
                    }

                    if !vv.includes_vv(before) {
                        calculator.mark_source_not_in_op_context();
                    }

                    if visited.contains(&op.container) {
                        // don't checkout if we have already checked out this container in this round
                        calculator.apply_change(oplog, RichOp::new_by_change(&change, op), None);
                    } else {
                        calculator.apply_change(
                            oplog,
                            RichOp::new_by_change(&change, op),
                            Some(vv),
                        );
                        visited.insert(container);
                    }
                }
            }

            Some(started_set)
        };

        // Because we need to get correct `bring_back` value that indicates container is created during this round of diff calc,
        // we need to iterate from parents to children. i.e. from smaller depth to larger depth.
        let mut new_containers = FxHashSet::default();
        let mut container_id_to_depth = FxHashMap::default();
        let mut all: Vec<(Option<NonZeroU16>, ContainerIdx)> = if let Some(set) = affected_set {
            // only visit the affected containers
            set.into_iter()
                .map(|x| {
                    let (depth, _) = self.calculators.get_mut(&x).unwrap();
                    (*depth, x)
                })
                .collect()
        } else {
            self.calculators
                .iter_mut()
                .map(|(x, (depth, _))| (*depth, *x))
                .collect()
        };
        let mut ans = FxHashMap::default();
        let info = DiffCalcVersionInfo {
            from_vv: before,
            to_vv: after,
            from_frontiers: before_frontiers,
            to_frontiers: after_frontiers,
            lca_vv: &lca,
        };
        while !all.is_empty() {
            // sort by depth and lamport, ensure we iterate from top to bottom
            all.sort_by_key(|x| x.0);
            for (_, container_idx) in std::mem::take(&mut all) {
                if ans.contains_key(&container_idx) {
                    continue;
                }
                let (depth, calc) = self.calculators.get_mut(&container_idx).unwrap();
                if depth.is_none() {
                    let d = oplog.arena.get_depth(container_idx);
                    if d != *depth {
                        *depth = d;
                        all.push((*depth, container_idx));
                        continue;
                    }
                }
                let id = oplog.arena.idx_to_id(container_idx).unwrap();
                let bring_back = new_containers.remove(&id);

                info_span!("CalcDiff", ?id).in_scope(|| {
                    let (diff, diff_mode) = calc.calculate_diff(container_idx, oplog, info, |c| {
                        new_containers.insert(c.clone());
                        container_id_to_depth
                            .insert(c.clone(), depth.and_then(|d| d.checked_add(1)));
                        oplog.arena.register_container(c);
                    });
                    calc.finish_this_round();
                    if !diff.is_empty() || bring_back {
                        ans.insert(
                            container_idx,
                            (
                                *depth,
                                InternalContainerDiff {
                                    idx: container_idx,
                                    bring_back,
                                    diff: diff.into(),
                                    diff_mode,
                                },
                            ),
                        );
                    }
                });
            }
        }

        while !new_containers.is_empty() {
            for id in std::mem::take(&mut new_containers) {
                // Registration can be lazy; ensure it is registered so we can proceed
                let idx = oplog.arena.register_container(&id);
                if ans.contains_key(&idx) {
                    continue;
                }
                let depth = container_id_to_depth.remove(&id).unwrap();
                ans.insert(
                    idx,
                    (
                        depth,
                        InternalContainerDiff {
                            idx,
                            bring_back: true,
                            diff: DiffVariant::None,
                            diff_mode: DiffMode::Checkout,
                        },
                    ),
                );
            }
        }

        (
            ans.into_values().map(|x| x.1).collect_vec(),
            origin_diff_mode,
        )
    }

    // TODO: we may remove depth info
    pub(crate) fn get_or_create_calc(
        &mut self,
        idx: ContainerIdx,
        depth: Option<NonZeroU16>,
    ) -> &mut (Option<NonZeroU16>, ContainerDiffCalculator) {
        self.calculators
            .entry(idx)
            .or_insert_with(|| match idx.get_type() {
                crate::ContainerType::Text => (
                    depth,
                    ContainerDiffCalculator::Richtext(RichtextDiffCalculator::new()),
                ),
                crate::ContainerType::Map => (
                    depth,
                    ContainerDiffCalculator::Map(MapDiffCalculator::new(idx)),
                ),
                crate::ContainerType::List => (
                    depth,
                    ContainerDiffCalculator::List(ListDiffCalculator::new(idx)),
                ),
                crate::ContainerType::Tree => (
                    depth,
                    ContainerDiffCalculator::Tree(TreeDiffCalculator::new(idx)),
                ),
                crate::ContainerType::Unknown(_) => (
                    depth,
                    ContainerDiffCalculator::Unknown(unknown::UnknownDiffCalculator),
                ),
                crate::ContainerType::MovableList => (
                    depth,
                    ContainerDiffCalculator::MovableList(MovableListDiffCalculator::new(idx)),
                ),
                #[cfg(feature = "counter")]
                crate::ContainerType::Counter => (
                    depth,
                    ContainerDiffCalculator::Counter(CounterDiffCalculator::new(idx)),
                ),
            })
    }
}

/// DiffCalculator should track the history first before it can calculate the difference.
///
/// So we need it to first apply all the ops between the two versions.
///
/// NOTE: not every op between two versions are included in a certain container.
/// So there may be some ops that cannot be seen by the container.
///
#[enum_dispatch]
pub(crate) trait DiffCalculatorTrait {
    fn start_tracking(&mut self, oplog: &OpLog, vv: &crate::VersionVector, mode: DiffMode);
    fn apply_change(
        &mut self,
        oplog: &OpLog,
        op: crate::op::RichOp,
        vv: Option<&crate::VersionVector>,
    );
    fn calculate_diff(
        &mut self,
        idx: ContainerIdx,
        oplog: &OpLog,
        info: DiffCalcVersionInfo,
        on_new_container: impl FnMut(&ContainerID),
    ) -> (InternalDiff, DiffMode);
    /// This round of diff calc is finished, we can clear the cache
    fn finish_this_round(&mut self);
}

#[enum_dispatch(DiffCalculatorTrait)]
#[derive(Debug)]
pub(crate) enum ContainerDiffCalculator {
    Map(MapDiffCalculator),
    List(ListDiffCalculator),
    Richtext(RichtextDiffCalculator),
    Tree(TreeDiffCalculator),
    MovableList(MovableListDiffCalculator),
    #[cfg(feature = "counter")]
    Counter(counter::CounterDiffCalculator),
    Unknown(UnknownDiffCalculator),
}

impl ContainerDiffCalculator {
    fn mark_source_not_in_op_context(&mut self) {
        match self {
            Self::Richtext(calc) => calc.mark_source_not_in_op_context(),
            Self::List(calc) => calc.mark_source_not_in_op_context(),
            Self::MovableList(calc) => calc.mark_source_not_in_op_context(),
            _ => {}
        }
    }
}

trait RebuildOpVisitor {
    fn visit(&mut self, vv: &VersionVector, op: RichOp<'_>);
}

#[cold]
#[inline(never)]
fn replay_container_ops_between(
    idx: ContainerIdx,
    oplog: &OpLog,
    from_vv: &VersionVector,
    to_vv: &VersionVector,
    to_frontiers: &Frontiers,
    visitor: &mut dyn RebuildOpVisitor,
) {
    let from_frontiers = oplog.dag.vv_to_frontiers(from_vv);
    let (_, _, iter) = oplog.iter_from_lca_causally(from_vv, &from_frontiers, to_vv, to_frontiers);

    for (change, (start_counter, end_counter), vv) in iter {
        let iter_start = change
            .ops
            .binary_search_by(|op| op.ctr_last().cmp(&start_counter))
            .unwrap_or_else(|e| e);
        for mut op in &change.ops.vec()[iter_start..] {
            if op.counter >= end_counter {
                break;
            }

            if op.container != idx || op.ctr_last() < start_counter {
                continue;
            }

            let stack_sliced_op;
            if op.counter < start_counter || op.ctr_end() > end_counter {
                stack_sliced_op = Some(op.slice(
                    (start_counter as usize).saturating_sub(op.counter as usize),
                    op.atom_len().min((end_counter - op.counter) as usize),
                ));
                op = stack_sliced_op.as_ref().unwrap();
            }

            let vv = &mut vv.borrow_mut();
            vv.extend_to_include_end_id(ID::new(change.peer(), op.counter));
            visitor.visit(vv, RichOp::new_by_change(&change, op));
        }
    }
}

#[cold]
#[inline(never)]
fn replay_container_ops_from_empty(
    idx: ContainerIdx,
    oplog: &OpLog,
    vv: &VersionVector,
    visitor: &mut dyn RebuildOpVisitor,
) {
    let empty_vv = VersionVector::default();
    let target_frontiers = oplog.dag.vv_to_frontiers(vv);
    replay_container_ops_between(idx, oplog, &empty_vv, vv, &target_frontiers, visitor);
}

#[derive(Debug)]
pub(crate) struct MapDiffCalculator {
    container_idx: ContainerIdx,
    changed: FxHashMap<InternalString, Option<MapValue>>,
    current_mode: DiffMode,
}

impl MapDiffCalculator {
    pub(crate) fn new(container_idx: ContainerIdx) -> Self {
        Self {
            container_idx,
            changed: Default::default(),
            current_mode: DiffMode::Checkout,
        }
    }
}

impl DiffCalculatorTrait for MapDiffCalculator {
    fn start_tracking(
        &mut self,
        _oplog: &crate::OpLog,
        _vv: &crate::VersionVector,
        mode: DiffMode,
    ) {
        self.changed.clear();
        self.current_mode = mode;
    }

    fn apply_change(
        &mut self,
        _oplog: &crate::OpLog,
        op: crate::op::RichOp,
        _vv: Option<&crate::VersionVector>,
    ) {
        if matches!(self.current_mode, DiffMode::Checkout) {
            // We need to use history cache anyway
            return;
        }

        let map = op.raw_op().content.as_map().unwrap();
        let new_value = MapValue {
            value: map.value.clone(),
            peer: op.peer,
            lamp: op.lamport(),
        };
        match self.changed.get(&map.key) {
            Some(Some(old_value)) if old_value > &new_value => {}
            _ => {
                self.changed.insert(map.key.clone(), Some(new_value));
            }
        }
    }

    fn finish_this_round(&mut self) {
        self.changed.clear();
        self.current_mode = DiffMode::Checkout;
    }

    fn calculate_diff(
        &mut self,
        _idx: ContainerIdx,
        oplog: &super::oplog::OpLog,
        DiffCalcVersionInfo { from_vv, to_vv, .. }: DiffCalcVersionInfo,
        mut on_new_container: impl FnMut(&ContainerID),
    ) -> (InternalDiff, DiffMode) {
        match self.current_mode {
            DiffMode::Checkout | DiffMode::Import => oplog.with_history_cache(|h| {
                let checkout_index = &h.get_checkout_index().map;
                let mut changed = Vec::new();
                let from_map = checkout_index.get_container_latest_op_at_vv(
                    self.container_idx,
                    from_vv,
                    Lamport::MAX,
                    oplog,
                );
                let mut to_map = checkout_index.get_container_latest_op_at_vv(
                    self.container_idx,
                    to_vv,
                    Lamport::MAX,
                    oplog,
                );

                for (k, peek_from) in from_map.iter() {
                    let peek_to = to_map.remove(k);
                    match peek_to {
                        None => changed.push((k.clone(), None)),
                        Some(b) => {
                            if peek_from.value != b.value {
                                changed.push((k.clone(), Some(b)))
                            }
                        }
                    }
                }

                for (k, peek_to) in to_map.into_iter() {
                    changed.push((k, Some(peek_to)));
                }

                let mut updated =
                    FxHashMap::with_capacity_and_hasher(changed.len(), Default::default());
                for (key, value) in changed {
                    let value = value.map(|v| {
                        let value = v.value.clone();
                        if let Some(LoroValue::Container(c)) = &value {
                            on_new_container(c);
                        }

                        MapValue {
                            value,
                            lamp: v.lamport,
                            peer: v.peer,
                        }
                    });

                    updated.insert(key, value);
                }

                (InternalDiff::Map(MapDelta { updated }), DiffMode::Checkout)
            }),
            DiffMode::ImportGreaterUpdates | DiffMode::Linear => {
                let changed = std::mem::take(&mut self.changed);
                let mode = self.current_mode;
                // Reset this field to avoid we use `has_all` to cache the diff calc and use it next round
                // (In the next round we need to use the checkout mode)
                self.current_mode = DiffMode::Checkout;
                (InternalDiff::Map(MapDelta { updated: changed }), mode)
            }
        }
    }
}

use rle::{HasLength as _, Sliceable};

pub(crate) struct ListDiffCalculator {
    container_idx: ContainerIdx,
    start_vv: VersionVector,
    tracker: Box<RichtextTracker>,
    source_not_in_op_context: bool,
}

impl ListDiffCalculator {
    fn new(container_idx: ContainerIdx) -> Self {
        Self {
            container_idx,
            start_vv: VersionVector::default(),
            tracker: Box::new(RichtextTracker::new_with_unknown()),
            source_not_in_op_context: false,
        }
    }

    pub(crate) fn get_id_latest_pos(&self, id: ID) -> Option<crate::cursor::AbsolutePosition> {
        self.tracker.get_target_id_latest_index_at_new_version(id)
    }

    fn mark_source_not_in_op_context(&mut self) {
        self.source_not_in_op_context = true;
    }

    fn shallow_root_vv(oplog: &OpLog) -> VersionVector {
        oplog
            .dag
            .frontiers_to_vv(oplog.shallow_since_frontiers())
            .unwrap_or_else(|| oplog.shallow_since_vv().to_vv())
    }

    fn seed_tracker_from_shallow_root(
        idx: ContainerIdx,
        oplog: &OpLog,
        tracker: &mut RichtextTracker,
        vv: &VersionVector,
        include_dead_items: bool,
    ) -> VersionVector {
        let shallow_root_vv = Self::shallow_root_vv(oplog);
        let seed_vv = if vv.includes_vv(&shallow_root_vv) {
            shallow_root_vv
        } else {
            vv.clone()
        };
        let spans = oplog
            .with_history_cache(|h| h.list_shallow_root_spans_in_order(idx, include_dead_items));

        *tracker = RichtextTracker::new_empty();
        let mut pos = 0;
        for (id, len) in spans {
            if len == 0 || !seed_vv.includes_id(id.id()) {
                continue;
            }

            tracker.insert_seeded(pos, RichtextChunk::new_unknown(len as u32), id);
            pos += len;
        }
        tracker.mark_shallow_root_applied(&seed_vv);
        seed_vv
    }

    fn start_tracking_list(
        &mut self,
        oplog: &OpLog,
        vv: &crate::VersionVector,
        include_dead_items: bool,
    ) {
        self.source_not_in_op_context = false;
        if oplog.shallow_since_vv().is_empty() {
            if !vv.includes_vv(&self.start_vv) || !self.tracker.all_vv().includes_vv(vv) {
                *self.tracker = RichtextTracker::new_with_unknown();
                self.start_vv = vv.clone();
            }
        } else if !vv.includes_vv(&self.start_vv) || !self.tracker.all_vv().includes_vv(vv) {
            let seed_vv = Self::seed_tracker_from_shallow_root(
                self.container_idx,
                oplog,
                &mut *self.tracker,
                vv,
                include_dead_items,
            );
            let target_frontiers = oplog.dag.vv_to_frontiers(vv);
            struct ListStartTrackingVisitor<'a> {
                tracker: &'a mut RichtextTracker,
            }

            impl RebuildOpVisitor for ListStartTrackingVisitor<'_> {
                fn visit(&mut self, vv: &VersionVector, op: RichOp<'_>) {
                    self.tracker.checkout(vv);
                    ListDiffCalculator::apply_op_to_tracker(self.tracker, &op);
                }
            }

            let mut visitor = ListStartTrackingVisitor {
                tracker: &mut *self.tracker,
            };
            replay_container_ops_between(
                self.container_idx,
                oplog,
                &seed_vv,
                vv,
                &target_frontiers,
                &mut visitor,
            );
            self.start_vv = vv.clone();
        }

        self.tracker.checkout(vv);
    }

    #[inline(never)]
    fn apply_op_to_tracker(tracker: &mut RichtextTracker, op: &crate::op::RichOp<'_>) {
        match &op.op().content {
            crate::op::InnerContent::List(l) => match l {
                InnerListOp::Insert { slice, pos } => {
                    tracker.insert(op.id_full(), *pos, RichtextChunk::new_text(slice.0.clone()));
                }
                InnerListOp::Delete(del) => {
                    tracker.delete(
                        op.id_start(),
                        del.id_start,
                        del.start() as usize,
                        del.atom_len(),
                        del.is_reversed(),
                    );
                }
                _ => unreachable!(),
            },
            _ => unreachable!(),
        }
    }

    #[cold]
    #[inline(never)]
    fn build_full_tracker(idx: ContainerIdx, oplog: &OpLog, vv: &VersionVector) -> RichtextTracker {
        struct ListRebuildVisitor<'a> {
            tracker: &'a mut RichtextTracker,
        }

        impl RebuildOpVisitor for ListRebuildVisitor<'_> {
            fn visit(&mut self, vv: &VersionVector, op: RichOp<'_>) {
                self.tracker.checkout(vv);
                ListDiffCalculator::apply_op_to_tracker(self.tracker, &op);
            }
        }

        let mut tracker = RichtextTracker::new_with_unknown();
        if oplog.shallow_since_vv().is_empty() {
            let mut visitor = ListRebuildVisitor {
                tracker: &mut tracker,
            };
            replay_container_ops_from_empty(idx, oplog, vv, &mut visitor);
        } else {
            let seed_vv = Self::seed_tracker_from_shallow_root(idx, oplog, &mut tracker, vv, false);
            let target_frontiers = oplog.dag.vv_to_frontiers(vv);
            let mut visitor = ListRebuildVisitor {
                tracker: &mut tracker,
            };
            replay_container_ops_between(idx, oplog, &seed_vv, vv, &target_frontiers, &mut visitor);
        }

        tracker
    }
}

impl MovableListDiffCalculator {
    pub(crate) fn get_id_latest_pos(&self, id: ID) -> Option<crate::cursor::AbsolutePosition> {
        self.list
            .tracker
            .get_target_id_latest_index_at_new_version(id)
    }

    fn mark_source_not_in_op_context(&mut self) {
        self.list.mark_source_not_in_op_context();
    }
}

impl std::fmt::Debug for ListDiffCalculator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ListDiffCalculator")
            // .field("tracker", &self.tracker)
            .finish()
    }
}

impl DiffCalculatorTrait for ListDiffCalculator {
    fn start_tracking(&mut self, oplog: &OpLog, vv: &crate::VersionVector, _mode: DiffMode) {
        self.start_tracking_list(oplog, vv, false);
    }

    fn apply_change(
        &mut self,
        _oplog: &OpLog,
        op: crate::op::RichOp,
        vv: Option<&crate::VersionVector>,
    ) {
        if let Some(vv) = vv {
            self.tracker.checkout(vv);
        }

        Self::apply_op_to_tracker(&mut self.tracker, &op);
    }

    fn finish_this_round(&mut self) {}

    fn calculate_diff(
        &mut self,
        idx: ContainerIdx,
        oplog: &OpLog,
        info: DiffCalcVersionInfo,
        mut on_new_container: impl FnMut(&ContainerID),
    ) -> (InternalDiff, DiffMode) {
        let mut delta = Delta::new();
        let (mut retreat, _) = info.from_vv.diff_iter(info.to_vv);
        let has_retreat = retreat.next().is_some();
        let should_rebuild = matches!(idx.get_type(), crate::ContainerType::List)
            && (has_retreat || info.lca_vv != info.from_vv || self.source_not_in_op_context);
        let diff_items = if should_rebuild {
            let mut merged = info.from_vv.clone();
            merged.merge(info.to_vv);
            let mut full_tracker = Self::build_full_tracker(idx, oplog, &merged);
            let diff_items = full_tracker.diff(info.from_vv, info.to_vv).collect_vec();
            *self.tracker = full_tracker;
            diff_items
        } else {
            self.tracker.diff(info.from_vv, info.to_vv).collect_vec()
        };

        for item in diff_items {
            match item {
                CrdtRopeDelta::Retain(len) => {
                    delta = delta.retain(len);
                }
                CrdtRopeDelta::Insert {
                    chunk: value,
                    id,
                    lamport,
                } => match value.value() {
                    RichtextChunkValue::Text(range) => {
                        for i in range.clone() {
                            let v = oplog.arena.get_value(i as usize);
                            if let Some(LoroValue::Container(c)) = &v {
                                on_new_container(c);
                            }
                        }
                        delta = delta.insert(SliceWithId {
                            values: Either::Left(SliceRange(range)),
                            id: IdFull::new(id.peer, id.counter, lamport.unwrap()),
                            elem_id: None,
                        });
                    }
                    RichtextChunkValue::StyleAnchor { .. } => unreachable!(),
                    RichtextChunkValue::Unknown(len) => {
                        delta = handle_unknown(idx, id, oplog, len, &mut on_new_container, delta);
                    }
                    RichtextChunkValue::MoveAnchor => {
                        delta = handle_unknown(idx, id, oplog, 1, &mut on_new_container, delta);
                    }
                },
                CrdtRopeDelta::Delete(len) => {
                    delta = delta.delete(len);
                }
            }
        }

        /// Handle span with unknown content when calculating diff
        ///
        /// We can lookup the content of the span by the id in the oplog
        fn handle_unknown(
            idx: ContainerIdx,
            mut id: ID,
            oplog: &OpLog,
            len: u32,
            on_new_container: &mut dyn FnMut(&ContainerID),
            mut delta: Delta<SliceWithId>,
        ) -> Delta<SliceWithId> {
            // assert not unknown id
            assert_ne!(id.peer, PeerID::MAX);
            let mut acc_len = 0;
            let end = id.counter + len as Counter;
            let shallow_root = oplog.shallow_since_vv().get(&id.peer).copied().unwrap_or(0);
            if id.counter < shallow_root {
                // need to find the content between id.counter ~ target_end in gc state
                let target_end = shallow_root.min(end);
                delta = oplog.with_history_cache(|h| {
                    let chunks =
                        h.find_list_chunks_in(idx, IdSpan::new(id.peer, id.counter, target_end));
                    for c in chunks {
                        acc_len += c.length();
                        match &c.values {
                            Either::Left(_) => unreachable!(),
                            Either::Right(r) => {
                                if let LoroValue::Container(c) = r {
                                    on_new_container(c)
                                }
                            }
                        }
                        delta = delta.insert(c);
                    }

                    delta
                });
                id.counter = shallow_root;
            }

            if id.counter < end {
                for rich_op in oplog.iter_ops(IdSpan::new(id.peer, id.counter, end)) {
                    acc_len += rich_op.content_len();
                    let op = rich_op.op();
                    let lamport = rich_op.lamport();

                    if let InnerListOp::Insert { slice, pos: _ } = op.content.as_list().unwrap() {
                        let range = slice.clone();
                        for i in slice.0.clone() {
                            let v = oplog.arena.get_value(i as usize);
                            if let Some(LoroValue::Container(c)) = &v {
                                (on_new_container)(c);
                            }
                        }

                        delta = delta.insert(SliceWithId {
                            values: Either::Left(range),
                            id: IdFull::new(id.peer, op.counter, lamport),
                            elem_id: None,
                        });
                    } else if let InnerListOp::Move { elem_id, .. } = op.content.as_list().unwrap()
                    {
                        delta = delta.insert(SliceWithId {
                            // We do NOT need an actual value range,
                            // movable list container will only use the id info
                            values: Either::Right(LoroValue::Null),
                            id: IdFull::new(id.peer, op.counter, lamport),
                            elem_id: Some(elem_id.compact()),
                        });
                    }
                }
            }

            debug_assert_eq!(acc_len, len as usize);
            delta
        }

        (InternalDiff::ListRaw(delta), DiffMode::Checkout)
    }
}

#[derive(Debug)]
pub(crate) struct RichtextDiffCalculator {
    mode: Box<RichtextCalcMode>,
}

#[derive(Debug)]
enum RichtextCalcMode {
    Crdt {
        tracker: Box<RichtextTracker>,
        /// (op, end_pos)
        styles: Vec<(StyleOp, usize)>,
        start_vv: VersionVector,
        source_not_in_op_context: bool,
        shallow_root_seeded: bool,
    },
    Linear {
        diff: DeltaRope<RichtextStateChunk, ()>,
        last_style_start: Option<(Arc<StyleOp>, u32)>,
    },
}

impl RichtextDiffCalculator {
    pub fn new() -> Self {
        Self {
            mode: Box::new(RichtextCalcMode::Crdt {
                tracker: Box::new(RichtextTracker::new_with_unknown()),
                styles: Vec::new(),
                start_vv: VersionVector::new(),
                source_not_in_op_context: false,
                shallow_root_seeded: false,
            }),
        }
    }

    fn mark_source_not_in_op_context(&mut self) {
        if let RichtextCalcMode::Crdt {
            source_not_in_op_context,
            ..
        } = &mut *self.mode
        {
            *source_not_in_op_context = true;
        }
    }

    fn style_for_end_anchor(oplog: &OpLog, op: &RichOp) -> Option<(StyleOp, usize)> {
        let style_start_id = op.id().inc(-1);
        if let Some(start_op) = oplog.get_op_that_includes(style_start_id) {
            let InnerListOp::StyleStart {
                start: _,
                end,
                key,
                value,
                info,
            } = start_op.content.as_list().unwrap()
            else {
                unreachable!()
            };

            Some((
                StyleOp {
                    lamport: start_op.lamport(),
                    peer: style_start_id.peer,
                    cnt: style_start_id.counter,
                    key: key.clone(),
                    value: value.clone(),
                    info: *info,
                },
                *end as usize,
            ))
        } else {
            oplog.with_history_cache(|history_cache| {
                history_cache
                    .find_text_style_end_in_shallow_root(op.raw_op().container, style_start_id)
            })
        }
    }

    fn shallow_delete_range_matches_tracker(
        tracker: &RichtextTracker,
        target_start: ID,
        pos: usize,
        len: usize,
    ) -> bool {
        let mut remaining = len;
        let mut pos = pos;
        let mut expected = target_start;
        while remaining > 0 {
            let Some((real_id, available)) = tracker.active_real_span_at(pos) else {
                return false;
            };

            if real_id.peer != expected.peer || real_id.counter != expected.counter {
                return false;
            }

            let take = remaining.min(available);
            expected = expected.inc(take as Counter);
            pos += take;
            remaining -= take;
        }

        true
    }

    fn shallow_clamped_tracker_pos(oplog: &OpLog, tracker: &RichtextTracker, pos: usize) -> usize {
        if oplog.shallow_since_vv().is_empty() {
            pos
        } else {
            pos.min(tracker.len())
        }
    }

    /// This should be called after calc_diff
    ///
    /// TODO: Refactor, this can be simplified
    pub fn get_id_latest_pos(&self, id: ID) -> Option<AbsolutePosition> {
        match &*self.mode {
            RichtextCalcMode::Crdt { tracker, .. } => {
                tracker.get_target_id_latest_index_at_new_version(id)
            }
            RichtextCalcMode::Linear { .. } => unreachable!(),
        }
    }

    fn apply_crdt_op_to_tracker(
        oplog: &OpLog,
        tracker: &mut RichtextTracker,
        styles: &mut Vec<(StyleOp, usize)>,
        op: RichOp,
    ) {
        match &op.raw_op().content {
            crate::op::InnerContent::List(l) => match l {
                InnerListOp::Insert { .. } | InnerListOp::Move { .. } | InnerListOp::Set { .. } => {
                    unreachable!()
                }
                InnerListOp::InsertText {
                    slice: _,
                    unicode_start,
                    unicode_len: len,
                    pos,
                } => {
                    let pos = Self::shallow_clamped_tracker_pos(oplog, tracker, *pos as usize);
                    tracker.insert(
                        op.id_full(),
                        pos,
                        RichtextChunk::new_text(*unicode_start..*unicode_start + *len),
                    );
                }
                InnerListOp::Delete(del) => {
                    let is_shallow = !oplog.shallow_since_vv().is_empty();
                    let pos = del.start() as usize;
                    if is_shallow
                        && !Self::shallow_delete_range_matches_tracker(
                            tracker,
                            del.id_start,
                            pos,
                            del.atom_len(),
                        )
                    {
                        let atom_len = del.atom_len();
                        let mut segments =
                            tracker.active_segments_of_real_id_span(del.id_start, atom_len);
                        if segments.is_empty() {
                            return;
                        }

                        segments.sort_unstable_by_key(|(_, pos, _)| *pos);
                        if del.is_reversed() {
                            for (target_id, pos, len) in segments.into_iter().rev() {
                                let target_offset =
                                    (target_id.counter - del.id_start.counter) as usize;
                                debug_assert!(target_offset + len <= atom_len);
                                let op_offset = atom_len - target_offset - len;
                                tracker.delete(
                                    op.id_start().inc(op_offset as Counter),
                                    target_id,
                                    pos,
                                    len,
                                    true,
                                );
                            }
                        } else {
                            let mut deleted_before = 0;
                            for (target_id, pos, len) in segments {
                                let target_offset =
                                    (target_id.counter - del.id_start.counter) as usize;
                                debug_assert!(pos >= deleted_before);
                                tracker.delete(
                                    op.id_start().inc(target_offset as Counter),
                                    target_id,
                                    pos.saturating_sub(deleted_before),
                                    len,
                                    false,
                                );
                                deleted_before += len;
                            }
                        }

                        return;
                    }

                    tracker.delete(
                        op.id_start(),
                        del.id_start,
                        pos,
                        del.atom_len(),
                        del.is_reversed(),
                    );
                }
                InnerListOp::StyleStart {
                    start,
                    end,
                    key,
                    info,
                    value,
                } => {
                    debug_assert!(start < end, "start: {}, end: {}", start, end);
                    let style_id = styles.len();
                    styles.push((
                        StyleOp {
                            lamport: op.lamport(),
                            peer: op.peer,
                            cnt: op.id_start().counter,
                            key: key.clone(),
                            value: value.clone(),
                            info: *info,
                        },
                        *end as usize,
                    ));
                    let start = Self::shallow_clamped_tracker_pos(oplog, tracker, *start as usize);
                    tracker.insert(
                        op.id_full(),
                        start,
                        RichtextChunk::new_style_anchor(style_id as u32, AnchorType::Start),
                    );
                }
                InnerListOp::StyleEnd => {
                    let id = op.id();
                    if let Some(pos) = styles
                        .iter()
                        .rev()
                        .position(|(op, _pos)| op.peer == id.peer && op.cnt == id.counter - 1)
                    {
                        let style_id = styles.len() - pos - 1;
                        let (_start_op, end_pos) = &styles[style_id];
                        tracker.insert(
                            op.id_full(),
                            // need to shift 1 because we insert the start style anchor before this pos
                            (*end_pos + 1).min(tracker.len()),
                            RichtextChunk::new_style_anchor(style_id as u32, AnchorType::End),
                        );
                    } else {
                        let Some((style, end)) = Self::style_for_end_anchor(oplog, &op) else {
                            panic!("Unhandled checkout case")
                        };
                        styles.push((style, end));
                        let style_id = styles.len() - 1;
                        tracker.insert(
                            op.id_full(),
                            // need to shift 1 because we insert the start style anchor before this pos
                            (end + 1).min(tracker.len()),
                            RichtextChunk::new_style_anchor(style_id as u32, AnchorType::End),
                        );
                    }
                }
            },
            _ => unreachable!(),
        }
    }

    #[cold]
    #[inline(never)]
    fn seed_tracker_from_shallow_root(
        idx: ContainerIdx,
        oplog: &OpLog,
        tracker: &mut RichtextTracker,
        styles: &mut Vec<(StyleOp, usize)>,
        vv: &VersionVector,
    ) {
        if oplog.shallow_since_vv().is_empty() {
            return;
        }

        *tracker = RichtextTracker::new_empty();
        styles.clear();
        let shallow_root_vv = oplog
            .dag
            .frontiers_to_vv(oplog.shallow_since_frontiers())
            .unwrap_or_else(|| oplog.shallow_since_vv().to_vv());
        let seed_vv = if vv.includes_vv(&shallow_root_vv) {
            &shallow_root_vv
        } else {
            vv
        };

        #[derive(Debug, Clone, Copy)]
        struct SeedItem {
            order: usize,
            id: IdFull,
            content: RichtextChunk,
        }

        struct Fenwick {
            tree: Vec<usize>,
        }

        impl Fenwick {
            fn new(len: usize) -> Self {
                Self {
                    tree: vec![0; len + 1],
                }
            }

            fn add(&mut self, mut index: usize, value: usize) {
                index += 1;
                while index < self.tree.len() {
                    self.tree[index] += value;
                    index += index & index.wrapping_neg();
                }
            }

            fn prefix_sum(&self, mut end: usize) -> usize {
                let mut sum = 0;
                while end > 0 {
                    sum += self.tree[end];
                    end -= end & end.wrapping_neg();
                }

                sum
            }
        }

        let chunks = oplog.with_history_cache(|h| h.find_text_chunks_in_shallow_root_order(idx));
        let mut pos = 0;
        let mut seed_items = Vec::new();
        let mut style_id_to_index = FxHashMap::default();
        for chunk in chunks {
            match chunk {
                RichtextStateChunk::Text(text) => {
                    let id = text.id_full();
                    let vv_end = seed_vv.get(&id.peer).copied().unwrap_or(0);
                    if vv_end <= id.counter {
                        continue;
                    }

                    let end = vv_end.min(id.counter + text.unicode_len() as Counter);
                    let len = (end - id.counter) as usize;
                    if len == 0 {
                        continue;
                    }

                    seed_items.push(SeedItem {
                        order: seed_items.len(),
                        id,
                        content: RichtextChunk::new_unknown(len as u32),
                    });
                    pos += len;
                }
                RichtextStateChunk::Style { style, anchor_type } => {
                    let id = match anchor_type {
                        AnchorType::Start => style.id(),
                        AnchorType::End => style.id().inc(1),
                    };
                    if !seed_vv.includes_id(id) {
                        continue;
                    }

                    let style_id = if let Some(id) = style_id_to_index.get(&style.id()) {
                        *id
                    } else {
                        let id = styles.len();
                        styles.push((style.as_ref().clone(), pos));
                        style_id_to_index.insert(style.id(), id);
                        id
                    };

                    if anchor_type == AnchorType::End {
                        styles[style_id].1 = pos.saturating_sub(1);
                    }

                    seed_items.push(SeedItem {
                        order: seed_items.len(),
                        id: IdFull::new(
                            id.peer,
                            id.counter,
                            style.lamport + (id.counter - style.cnt) as u32,
                        ),
                        content: RichtextChunk::new_style_anchor(style_id as u32, anchor_type),
                    });
                    pos += 1;
                }
            }
        }

        seed_items.sort_unstable_by_key(|item| (item.id.peer, item.id.counter));
        let mut seen_end_by_peer: FxHashMap<PeerID, Counter> = FxHashMap::default();
        let mut normalized_items = Vec::with_capacity(seed_items.len());
        for mut item in seed_items {
            let end = item.id.counter + item.content.len() as Counter;
            let seen_end = seen_end_by_peer.entry(item.id.peer).or_default();
            if end <= *seen_end {
                continue;
            }

            if item.id.counter < *seen_end {
                let skip = (*seen_end - item.id.counter) as usize;
                item.id = item.id.inc(skip as Counter);
                item.content = item.content.slice(skip..item.content.len());
            }

            *seen_end = end;
            normalized_items.push(item);
        }

        let seed_items = normalized_items;
        let fenwick_len = seed_items
            .iter()
            .map(|item| item.order)
            .max()
            .map_or(0, |order| order + 1);
        let mut inserted = Fenwick::new(fenwick_len);
        for item in seed_items {
            let pos = inserted.prefix_sum(item.order);
            tracker.insert_seeded(pos, item.content, item.id);
            inserted.add(item.order, item.content.len());
        }

        tracker.mark_shallow_root_applied(seed_vv);
    }

    #[cold]
    #[inline(never)]
    fn build_full_crdt_tracker(
        idx: ContainerIdx,
        oplog: &OpLog,
        vv: &VersionVector,
    ) -> (RichtextTracker, Vec<(StyleOp, usize)>) {
        struct RichtextRebuildVisitor<'a> {
            oplog: &'a OpLog,
            tracker: &'a mut RichtextTracker,
            styles: &'a mut Vec<(StyleOp, usize)>,
        }

        impl RebuildOpVisitor for RichtextRebuildVisitor<'_> {
            fn visit(&mut self, vv: &VersionVector, op: RichOp<'_>) {
                self.tracker.checkout(vv);
                RichtextDiffCalculator::apply_crdt_op_to_tracker(
                    self.oplog,
                    self.tracker,
                    self.styles,
                    op,
                );
            }
        }

        let mut tracker = RichtextTracker::new_with_unknown();
        let mut styles = Vec::new();
        if !oplog.shallow_since_vv().is_empty() {
            Self::seed_tracker_from_shallow_root(idx, oplog, &mut tracker, &mut styles, vv);
        }

        let mut visitor = RichtextRebuildVisitor {
            oplog,
            tracker: &mut tracker,
            styles: &mut styles,
        };
        replay_container_ops_from_empty(idx, oplog, vv, &mut visitor);

        (tracker, styles)
    }
}

impl DiffCalculatorTrait for RichtextDiffCalculator {
    fn start_tracking(
        &mut self,
        _oplog: &super::oplog::OpLog,
        vv: &crate::VersionVector,
        mode: DiffMode,
    ) {
        match mode {
            DiffMode::Linear => {
                *self.mode = RichtextCalcMode::Linear {
                    diff: DeltaRope::new(),
                    last_style_start: None,
                };
            }
            _ => {
                if !matches!(&*self.mode, RichtextCalcMode::Crdt { .. }) {
                    unreachable!();
                }
            }
        }

        match &mut *self.mode {
            RichtextCalcMode::Crdt {
                tracker,
                styles,
                start_vv,
                source_not_in_op_context,
                shallow_root_seeded,
            } => {
                *source_not_in_op_context = false;
                if !vv.includes_vv(start_vv) || !tracker.all_vv().includes_vv(vv) {
                    **tracker = RichtextTracker::new_with_unknown();
                    styles.clear();
                    *start_vv = vv.clone();
                    *shallow_root_seeded = false;
                }

                tracker.checkout(vv);
            }
            RichtextCalcMode::Linear { .. } => {}
        }
    }

    fn apply_change(
        &mut self,
        oplog: &super::oplog::OpLog,
        op: crate::op::RichOp,
        vv: Option<&crate::VersionVector>,
    ) {
        match &mut *self.mode {
            RichtextCalcMode::Linear {
                diff,
                last_style_start,
            } => match &op.raw_op().content {
                crate::op::InnerContent::List(l) => match l {
                    InnerListOp::Insert { .. }
                    | InnerListOp::Move { .. }
                    | InnerListOp::Set { .. } => {
                        unreachable!()
                    }
                    InnerListOp::InsertText {
                        slice: _,
                        unicode_start,
                        unicode_len: len,
                        pos,
                    } => {
                        let s = oplog.arena.slice_by_unicode(
                            *unicode_start as usize..(*unicode_start + *len) as usize,
                        );
                        diff.insert_value(
                            *pos as usize,
                            RichtextStateChunk::new_text(s, op.id_full()),
                            (),
                        );
                    }
                    InnerListOp::Delete(del) => {
                        diff.delete(del.start() as usize, del.atom_len());
                    }
                    InnerListOp::StyleStart {
                        start,
                        end,
                        key,
                        info,
                        value,
                    } => {
                        debug_assert!(start < end, "start: {}, end: {}", start, end);
                        let style_op = Arc::new(StyleOp {
                            lamport: op.lamport(),
                            peer: op.peer,
                            cnt: op.id_start().counter,
                            key: key.clone(),
                            value: value.clone(),
                            info: *info,
                        });

                        *last_style_start = Some((style_op.clone(), *end));
                        diff.insert_value(
                            *start as usize,
                            RichtextStateChunk::new_style(style_op, AnchorType::Start),
                            (),
                        );
                    }
                    InnerListOp::StyleEnd => {
                        let (style_op, pos) = match last_style_start.take() {
                            Some((style_op, pos)) => (style_op, pos),
                            None => {
                                let Some((style_op, pos)) = Self::style_for_end_anchor(oplog, &op)
                                else {
                                    panic!("Unhandled checkout case")
                                };

                                (Arc::new(style_op), pos as u32)
                            }
                        };
                        assert_eq!(style_op.peer, op.peer);
                        assert_eq!(style_op.cnt, op.id_start().counter - 1);
                        diff.insert_value(
                            pos as usize + 1,
                            RichtextStateChunk::new_style(style_op, AnchorType::End),
                            (),
                        );
                    }
                },
                _ => unreachable!(),
            },
            RichtextCalcMode::Crdt {
                tracker,
                styles,
                start_vv,
                source_not_in_op_context: _,
                shallow_root_seeded,
            } => {
                if !*shallow_root_seeded && !oplog.shallow_since_vv().is_empty() {
                    Self::seed_tracker_from_shallow_root(
                        op.raw_op().container,
                        oplog,
                        tracker,
                        styles,
                        start_vv,
                    );
                    *shallow_root_seeded = true;
                }

                if let Some(vv) = vv {
                    tracker.checkout(vv);
                }
                Self::apply_crdt_op_to_tracker(oplog, tracker, styles, op);
            }
        }
    }

    fn calculate_diff(
        &mut self,
        idx: ContainerIdx,
        oplog: &OpLog,
        info: DiffCalcVersionInfo,
        _: impl FnMut(&ContainerID),
    ) -> (InternalDiff, DiffMode) {
        fn push_tracker_chunk(
            delta: &mut DeltaRope<RichtextStateChunk, ()>,
            idx: ContainerIdx,
            oplog: &OpLog,
            styles: &[(StyleOp, usize)],
            value: RichtextChunk,
            id: ID,
            lamport: Option<Lamport>,
        ) {
            match value.value() {
                RichtextChunkValue::Text(text) => {
                    delta.push_insert(
                        RichtextStateChunk::Text(
                            // PERF: can be speedup by acquiring lock on arena
                            TextChunk::new(
                                oplog
                                    .arena
                                    .slice_by_unicode(text.start as usize..text.end as usize),
                                IdFull::new(id.peer, id.counter, lamport.unwrap()),
                            ),
                        ),
                        (),
                    );
                }
                RichtextChunkValue::StyleAnchor { id, anchor_type } => {
                    delta.push_insert(
                        RichtextStateChunk::Style {
                            style: Arc::new(styles[id as usize].0.clone()),
                            anchor_type,
                        },
                        (),
                    );
                }
                RichtextChunkValue::Unknown(len) => {
                    // assert not unknown id
                    assert_ne!(id.peer, PeerID::MAX);
                    let mut id = id;
                    let mut acc_len = 0;
                    let end = id.counter + len as Counter;
                    let shallow_root = oplog.shallow_since_vv().get(&id.peer).copied().unwrap_or(0);
                    if id.counter < shallow_root {
                        // need to find the content between id.counter ~ target_end in gc state
                        let target_end = shallow_root.min(end);
                        oplog.with_history_cache(|h| {
                            let chunks = h.find_text_chunks_in(
                                idx,
                                IdSpan::new(id.peer, id.counter, target_end),
                            );
                            for c in chunks {
                                acc_len += c.rle_len();
                                delta.push_insert(c, ());
                            }
                        });
                        id.counter = shallow_root;
                    }

                    if id.counter < end {
                        for rich_op in oplog.iter_ops(IdSpan::new(id.peer, id.counter, end)) {
                            acc_len += rich_op.content_len();
                            let op = rich_op.op();
                            let lamport = rich_op.lamport();
                            let content = op.content.as_list().unwrap();
                            match content {
                                InnerListOp::InsertText { slice, .. } => {
                                    delta.push_insert(
                                        RichtextStateChunk::Text(TextChunk::new(
                                            slice.clone(),
                                            IdFull::new(id.peer, op.counter, lamport),
                                        )),
                                        (),
                                    );
                                }
                                _ => unreachable!("{:?}", content),
                            }
                        }
                    }

                    debug_assert_eq!(acc_len, len as usize);
                }
                RichtextChunkValue::MoveAnchor => unreachable!(),
            }
        }

        fn push_tracker_delta_item(
            delta: &mut DeltaRope<RichtextStateChunk, ()>,
            idx: ContainerIdx,
            oplog: &OpLog,
            styles: &[(StyleOp, usize)],
            item: CrdtRopeDelta,
        ) {
            match item {
                CrdtRopeDelta::Retain(len) => {
                    delta.push_retain(len, ());
                }
                CrdtRopeDelta::Insert {
                    chunk: value,
                    id,
                    lamport,
                } => push_tracker_chunk(delta, idx, oplog, styles, value, id, lamport),
                CrdtRopeDelta::Delete(len) => {
                    delta.push_delete(len);
                }
            }
        }

        match &mut *self.mode {
            RichtextCalcMode::Linear { diff, .. } => (
                InternalDiff::RichtextRaw(std::mem::take(diff)),
                DiffMode::Linear,
            ),
            RichtextCalcMode::Crdt {
                tracker,
                styles,
                source_not_in_op_context,
                ..
            } => {
                let (mut retreat, _) = info.from_vv.diff_iter(info.to_vv);
                let has_retreat = retreat.next().is_some();
                let should_rebuild = has_retreat
                    || info.lca_vv != info.from_vv
                    || *source_not_in_op_context
                    || !oplog.shallow_since_vv().is_empty();
                if should_rebuild {
                    // Richtext diffs can start from a tracker that only knows the LCA state as
                    // unknown spans. Expressing a rollback or an import from `lca != from` as local
                    // edits can target the wrong visible text when the source state contains
                    // concurrent inserts or sliced ops. The same risk exists when an op is replayed
                    // from a dependency version that does not include the visible source state.
                    // Preserve correctness by replacing the visible source state with the target
                    // state reconstructed from CRDT ids. Shallow docs seed this tracker from the
                    // shallow-root state and replay only the retained suffix of history.
                    let mut merged = info.from_vv.clone();
                    merged.merge(info.to_vv);
                    let (mut full_tracker, full_styles) =
                        Self::build_full_crdt_tracker(idx, oplog, &merged);

                    let mut delta = DeltaRope::new();
                    for item in full_tracker.diff(info.from_vv, info.to_vv) {
                        push_tracker_delta_item(&mut delta, idx, oplog, &full_styles, item);
                    }
                    **tracker = full_tracker;
                    *styles = full_styles;

                    return (InternalDiff::RichtextRaw(delta), DiffMode::Checkout);
                }

                let mut delta = DeltaRope::new();
                for item in tracker.diff(info.from_vv, info.to_vv) {
                    push_tracker_delta_item(&mut delta, idx, oplog, styles, item);
                }

                (InternalDiff::RichtextRaw(delta), DiffMode::Checkout)
            }
        }
    }

    fn finish_this_round(&mut self) {
        match &mut *self.mode {
            RichtextCalcMode::Crdt { .. } => {}
            RichtextCalcMode::Linear {
                diff,
                last_style_start,
            } => {
                *diff = DeltaRope::new();
                last_style_start.take();
            }
        }
    }
}

#[derive(Debug)]
pub(crate) struct MovableListDiffCalculator {
    list: Box<ListDiffCalculator>,
    inner: Box<MovableListInner>,
}

#[derive(Debug)]
struct MovableListInner {
    changed_elements: FxHashMap<CompactIdLp, ElementDelta>,
    move_id_to_elem_id: FxHashMap<ID, IdLp>,
    current_mode: DiffMode,
}

impl DiffCalculatorTrait for MovableListDiffCalculator {
    fn start_tracking(&mut self, oplog: &OpLog, vv: &crate::VersionVector, mode: DiffMode) {
        self.list.source_not_in_op_context = false;
        if oplog.shallow_since_vv().is_empty() {
            if !vv.includes_vv(&self.list.start_vv) || !self.list.tracker.all_vv().includes_vv(vv) {
                *self.list.tracker = RichtextTracker::new_with_unknown();
                self.list.start_vv = vv.clone();
            }
        } else if !vv.includes_vv(&self.list.start_vv)
            || !self.list.tracker.all_vv().includes_vv(vv)
        {
            let seed_vv = ListDiffCalculator::seed_tracker_from_shallow_root(
                self.list.container_idx,
                oplog,
                &mut *self.list.tracker,
                vv,
                true,
            );
            let target_frontiers = oplog.dag.vv_to_frontiers(vv);
            struct MovableListStartTrackingVisitor<'a> {
                oplog: &'a OpLog,
                tracker: &'a mut RichtextTracker,
                move_id_to_elem_id: &'a mut FxHashMap<ID, IdLp>,
            }

            impl RebuildOpVisitor for MovableListStartTrackingVisitor<'_> {
                fn visit(&mut self, vv: &VersionVector, op: RichOp<'_>) {
                    self.tracker.checkout(vv);
                    MovableListDiffCalculator::apply_op_to_tracker(
                        self.tracker,
                        self.move_id_to_elem_id,
                        self.oplog,
                        &op,
                        true,
                    );
                }
            }

            self.inner.move_id_to_elem_id.clear();
            let mut visitor = MovableListStartTrackingVisitor {
                oplog,
                tracker: &mut *self.list.tracker,
                move_id_to_elem_id: &mut self.inner.move_id_to_elem_id,
            };
            replay_container_ops_between(
                self.list.container_idx,
                oplog,
                &seed_vv,
                vv,
                &target_frontiers,
                &mut visitor,
            );
            self.list.start_vv = vv.clone();
        }

        self.list.tracker.checkout(vv);
        self.inner.current_mode = mode;
    }

    fn apply_change(
        &mut self,
        oplog: &OpLog,
        op: crate::op::RichOp,
        vv: Option<&crate::VersionVector>,
    ) {
        let InnerContent::List(l) = &op.raw_op().content else {
            unreachable!()
        };

        // collect the elements that are moved, updated, or inserted

        // If it's checkout mode, we don't need to track the changes
        // we only need the element ids
        match l {
            InnerListOp::Insert { slice, pos: _ } => {
                let op_id = op.id_full().idlp();
                for i in 0..slice.atom_len() {
                    let id = op_id.inc(i as Counter);
                    let value = oplog.arena.get_value(slice.0.start as usize + i).unwrap();

                    self.inner.changed_elements.insert(
                        id.compact(),
                        ElementDelta {
                            pos: Some(id),
                            value: value.clone(),
                            value_updated: true,
                            value_id: Some(id),
                        },
                    );
                }
            }
            InnerListOp::Delete(_) => {}
            InnerListOp::Move { elem_id, .. } => {
                let idlp = IdLp::new(op.peer, op.lamport());
                match self.inner.changed_elements.get_mut(&elem_id.compact()) {
                    Some(change) => {
                        if change.pos.is_some() && change.pos.as_ref().unwrap() > &idlp {
                        } else {
                            change.pos = Some(idlp);
                        }
                    }
                    None => {
                        self.inner.changed_elements.insert(
                            elem_id.compact(),
                            ElementDelta {
                                pos: Some(idlp),
                                value: LoroValue::Null,
                                value_updated: false,
                                value_id: None,
                            },
                        );
                    }
                }
            }
            InnerListOp::Set { elem_id, value } => {
                let idlp = IdLp::new(op.peer, op.lamport());
                match self.inner.changed_elements.get_mut(&elem_id.compact()) {
                    Some(change) => {
                        if change.value_id.is_some() && change.value_id.as_ref().unwrap() > &idlp {
                        } else {
                            change.value_id = Some(idlp);
                            change.value = value.clone();
                        }
                    }
                    None => {
                        self.inner.changed_elements.insert(
                            elem_id.compact(),
                            ElementDelta {
                                pos: None,
                                value: value.clone(),
                                value_updated: true,
                                value_id: Some(idlp),
                            },
                        );
                    }
                }
            }

            InnerListOp::StyleStart { .. } => unreachable!(),
            InnerListOp::StyleEnd => unreachable!(),
            InnerListOp::InsertText { .. } => unreachable!(),
        }

        let is_checkout = matches!(self.inner.current_mode, DiffMode::Checkout);

        {
            // Apply change on the list items
            if let Some(vv) = vv {
                self.list.tracker.checkout(vv);
            }
            Self::apply_op_to_tracker(
                &mut self.list.tracker,
                &mut self.inner.move_id_to_elem_id,
                oplog,
                &op,
                is_checkout,
            );
        };
    }

    fn finish_this_round(&mut self) {
        self.list.finish_this_round();
    }

    #[instrument(skip(self, oplog, on_new_container))]
    fn calculate_diff(
        &mut self,
        idx: ContainerIdx,
        oplog: &OpLog,
        info: DiffCalcVersionInfo,
        mut on_new_container: impl FnMut(&ContainerID),
    ) -> (InternalDiff, DiffMode) {
        let (mut retreat, _) = info.from_vv.diff_iter(info.to_vv);
        let has_retreat = retreat.next().is_some();
        if has_retreat || info.lca_vv != info.from_vv || self.list.source_not_in_op_context {
            let mut merged = info.from_vv.clone();
            merged.merge(info.to_vv);
            self.rebuild_full_tracker(idx, oplog, &merged);
        }

        let (InternalDiff::ListRaw(list_diff), diff_mode) =
            self.list.calculate_diff(idx, oplog, info, |_| {})
        else {
            unreachable!()
        };

        assert_eq!(diff_mode, DiffMode::Checkout);
        let is_checkout = matches!(
            self.inner.current_mode,
            DiffMode::Checkout | DiffMode::Import
        );
        let mut element_changes: FxHashMap<CompactIdLp, ElementDelta> = if is_checkout {
            FxHashMap::default()
        } else {
            std::mem::take(&mut self.inner.changed_elements)
        };

        if is_checkout {
            for id in self.inner.changed_elements.keys() {
                element_changes.insert(*id, ElementDelta::placeholder());
            }
        }

        let list_diff: Delta<SmallVec<[IdFull; 1]>, ()> = Delta {
            vec: list_diff
                .iter()
                .map(|x| match x {
                    &DeltaItem::Retain { retain, .. } => DeltaItem::Retain {
                        retain,
                        attributes: (),
                    },
                    DeltaItem::Insert { insert, .. } => {
                        let len = insert.length();
                        let id = insert.id;
                        let mut new_insert = SmallVec::with_capacity(len);
                        for i in 0..len {
                            let id = id.inc(i as i32);
                            let elem_id = self
                                .inner
                                .move_id_to_elem_id
                                .get(&id.id())
                                .map(|e| e.compact())
                                .or(insert.elem_id)
                                .or_else(|| {
                                    let elem_id = id.idlp().compact();
                                    self.inner
                                        .changed_elements
                                        .contains_key(&elem_id)
                                        .then_some(elem_id)
                                });
                            if is_checkout {
                                if let Some(elem_id) = elem_id {
                                    // add the related element id
                                    element_changes.insert(elem_id, ElementDelta::placeholder());
                                }
                            }
                            new_insert.push(id);
                        }

                        DeltaItem::Insert {
                            insert: new_insert,
                            attributes: (),
                        }
                    }
                    &DeltaItem::Delete { delete, .. } => DeltaItem::Delete {
                        delete,
                        attributes: (),
                    },
                })
                .collect(),
        };

        if is_checkout {
            oplog.with_history_cache(|history_cache| {
                let checkout_index = &history_cache.get_checkout_index().movable_list;
                element_changes.retain(|id, change| {
                    let id = id.to_id();
                    // It can be None if the target does not exist before the `to` version
                    // But we don't need to calc from, because the deletion is handled by the diff from list items

                    // TODO: PERF: Provide the lamport of to version
                    let Some(pos) = checkout_index.last_pos(id, info.to_vv, Lamport::MAX, oplog)
                    else {
                        return false;
                    };
                    // TODO: PERF: Provide the lamport of to version
                    let value = checkout_index
                        .last_value(id, info.to_vv, Lamport::MAX, oplog)
                        .unwrap();
                    // TODO: PERF: Provide the lamport of to version
                    let old_pos = checkout_index.last_pos(id, info.from_vv, Lamport::MAX, oplog);
                    // TODO: PERF: Provide the lamport of to version
                    let old_value =
                        checkout_index.last_value(id, info.from_vv, Lamport::MAX, oplog);
                    if old_pos.is_none() && old_value.is_none() {
                        if let LoroValue::Container(c) = &value.value {
                            on_new_container(c);
                        }
                        *change = ElementDelta {
                            pos: Some(pos.idlp()),
                            value: value.value.clone(),
                            value_id: Some(IdLp::new(value.peer, value.lamport)),
                            value_updated: true,
                        };
                    } else {
                        // TODO: PERF: can be filtered based on the list_diff and whether the pos/value are updated
                        *change = ElementDelta {
                            pos: Some(pos.idlp()),
                            value: value.value.clone(),
                            value_updated: old_value.unwrap().value != value.value,
                            value_id: Some(IdLp::new(value.peer, value.lamport)),
                        };
                    }

                    true
                });
            });
        }

        let diff = MovableListInnerDelta {
            list: list_diff,
            elements: element_changes,
        };

        (InternalDiff::MovableList(diff), self.inner.current_mode)
    }
}

impl MovableListDiffCalculator {
    fn new(container: ContainerIdx) -> MovableListDiffCalculator {
        MovableListDiffCalculator {
            list: Box::new(ListDiffCalculator::new(container)),
            inner: Box::new(MovableListInner {
                changed_elements: Default::default(),
                current_mode: DiffMode::Checkout,
                move_id_to_elem_id: Default::default(),
            }),
        }
    }

    fn apply_op_to_tracker(
        tracker: &mut RichtextTracker,
        move_id_to_elem_id: &mut FxHashMap<ID, IdLp>,
        oplog: &OpLog,
        op: &RichOp<'_>,
        is_checkout: bool,
    ) {
        let real_op = op.op();
        match &real_op.content {
            InnerContent::List(l) => match l {
                InnerListOp::Insert { .. } | InnerListOp::Delete(_) => {
                    ListDiffCalculator::apply_op_to_tracker(tracker, op);
                }
                InnerListOp::Move { from, elem_id, to } => {
                    move_id_to_elem_id.insert(op.id(), *elem_id);
                    if !tracker.current_vv().includes_id(op.id()) {
                        let last_pos = if is_checkout {
                            oplog.with_history_cache(|h| {
                                let list = &h.get_checkout_index().movable_list;
                                list.last_pos(*elem_id, tracker.current_vv(), Lamport::MAX, oplog)
                                    .expect("moved element should have a visible source position")
                                    .id()
                            })
                        } else {
                            // In import/linear mode this id is only needed if the tracker is later
                            // checked out before the source version, which those modes do not do.
                            ID::new(PeerID::MAX - 2, 0)
                        };
                        tracker.move_item(op.id_full(), last_pos, *from as usize, *to as usize);
                    }
                }
                InnerListOp::Set { .. } => {}
                InnerListOp::InsertText { .. }
                | InnerListOp::StyleStart { .. }
                | InnerListOp::StyleEnd => unreachable!(),
            },
            _ => unreachable!(),
        }
    }

    #[cold]
    #[inline(never)]
    fn rebuild_full_tracker(&mut self, idx: ContainerIdx, oplog: &OpLog, vv: &VersionVector) {
        struct MovableListRebuildVisitor<'a> {
            oplog: &'a OpLog,
            tracker: &'a mut RichtextTracker,
            move_id_to_elem_id: &'a mut FxHashMap<ID, IdLp>,
        }

        impl RebuildOpVisitor for MovableListRebuildVisitor<'_> {
            fn visit(&mut self, vv: &VersionVector, op: RichOp<'_>) {
                self.tracker.checkout(vv);
                MovableListDiffCalculator::apply_op_to_tracker(
                    self.tracker,
                    self.move_id_to_elem_id,
                    self.oplog,
                    &op,
                    true,
                );
            }
        }

        let mut tracker = RichtextTracker::new_with_unknown();
        let mut move_id_to_elem_id = FxHashMap::default();
        if oplog.shallow_since_vv().is_empty() {
            let mut visitor = MovableListRebuildVisitor {
                oplog,
                tracker: &mut tracker,
                move_id_to_elem_id: &mut move_id_to_elem_id,
            };
            replay_container_ops_from_empty(idx, oplog, vv, &mut visitor);
        } else {
            let seed_vv = ListDiffCalculator::seed_tracker_from_shallow_root(
                idx,
                oplog,
                &mut tracker,
                vv,
                true,
            );
            let target_frontiers = oplog.dag.vv_to_frontiers(vv);
            let mut visitor = MovableListRebuildVisitor {
                oplog,
                tracker: &mut tracker,
                move_id_to_elem_id: &mut move_id_to_elem_id,
            };
            replay_container_ops_between(idx, oplog, &seed_vv, vv, &target_frontiers, &mut visitor);
        }

        *self.list.tracker = tracker;
        self.inner.move_id_to_elem_id = move_id_to_elem_id;
    }
}

#[test]
fn test_size() {
    let text = RichtextDiffCalculator::new();
    let size = std::mem::size_of_val(&text);
    assert!(size < 50, "RichtextDiffCalculator size: {}", size);
    let list = MovableListDiffCalculator::new(ContainerIdx::from_index_and_type(
        0,
        loro_common::ContainerType::MovableList,
    ));
    let size = std::mem::size_of_val(&list);
    assert!(size < 50, "MovableListDiffCalculator size: {}", size);
    let calc = ContainerDiffCalculator::Richtext(text);
    let size = std::mem::size_of_val(&calc);
    assert!(size < 50, "ContainerDiffCalculator size: {}", size);
}
